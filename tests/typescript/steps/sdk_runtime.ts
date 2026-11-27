// SPDX-License-Identifier: MIT
/**
 * The TypeScript answer to `tests/features/sdk_runtime.feature`.
 *
 * The runtime is reached through its package entry alone. Every message the
 * Rust core hands it (`bones.sdk.v1.CallError`, `SpanStart`, `SpanEnd`) is
 * built here as protobuf bytes and read back from the bytes the runtime
 * answers: the runtime makes no network call.
 */

import assert from "node:assert/strict";
import { mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

import { create, fromBinary, toBinary, type MessageInitShape } from "@bufbuild/protobuf";
import { timestampFromMs } from "@bufbuild/protobuf/wkt";
import { After, Before, BeforeAll, Given, Then, When, World, type DataTable } from "@cucumber/cucumber";
import { ROOT_CONTEXT, SpanStatusCode, context, createTraceState, propagation, trace } from "@opentelemetry/api";
import type { Context, Span } from "@opentelemetry/api";
import { AsyncLocalStorageContextManager } from "@opentelemetry/context-async-hooks";
import { W3CTraceContextPropagator } from "@opentelemetry/core";
import { AlwaysOffSampler, BasicTracerProvider } from "@opentelemetry/sdk-trace-base";
import type { ReadableSpan } from "@opentelemetry/sdk-trace-base";
import {
  BonesError,
  FormatErrorOptionError,
  JsSpanSink,
  PageError,
  callCtx,
  formatError,
  isCursorRejected,
  lazyOnce,
  mapCallError,
  registerErrorCatalog,
  wasmInput,
  type CallCtx,
  type ErrorCatalog,
  type FormattedError,
} from "@brefwiz/api-bones-sdk-runtime";
import {
  CallErrorSchema,
  PageFailure_Kind,
  SpanEnd_Status,
  SpanEndSchema,
  SpanStart_Kind,
  SpanStartSchema,
  SpanStartedSchema,
} from "../generated/bones/sdk/v1/boundary_pb.js";
import type { ErrorArgSchema } from "../generated/bones/v1/errors_pb.js";

const FIXTURES = new URL("../../features/fixtures/sdk_runtime/", import.meta.url);
const CORPUS = new URL("../../features/fixtures/locale_catalogs/", import.meta.url);
const PLATFORM_CATALOGS = new URL("../../../api-bones-i18n/catalogs/errors/", import.meta.url);
const FEATURE = new URL("../../features/sdk_runtime.feature", import.meta.url);

async function jsonAt<T>(directory: URL, name: string): Promise<T> {
  return JSON.parse(await readFile(new URL(`${name}.json`, directory), "utf8")) as T;
}

const recorded = <T>(name: string): Promise<T> => jsonAt<T>(FIXTURES, name);

/** The locales a directory holds a catalog file for; a `.sources.json` sidecar is no catalog. */
async function localesOf(directory: URL): Promise<string[]> {
  const names = await readdir(directory);
  const catalogs = names.filter((name) => name.endsWith(".json") && !name.endsWith(".sources.json"));
  return catalogs.map((name) => name.slice(0, -".json".length)).sort();
}

/** The locale column of every Examples table of one scenario outline of the feature file. */
async function exampleLocales(outline: string): Promise<string[]> {
  const lines = (await readFile(FEATURE, "utf8")).split("\n").map((line) => line.trim());
  const from = lines.indexOf(`Scenario Outline: ${outline}`);
  assert.ok(from >= 0, `no scenario outline named ${outline}`);
  const rest = lines.slice(from + 1);
  const end = rest.findIndex((line) => line.startsWith("Scenario"));
  const tables = (end < 0 ? rest : rest.slice(0, end)).filter((line) => line.startsWith("|"));
  const rows = tables.map((line) => line.split("|").slice(1, -1).map((cell) => cell.trim()));
  const column = rows[0]?.indexOf("locale") ?? -1;
  assert.ok(column >= 0, `the Examples of ${outline} have no locale column`);
  return [...new Set(rows.slice(1).map((row) => row[column] ?? ""))].sort();
}

/** The span interface carries no parent or name; the finished and recording span objects do. */
function readableOf(span: Span): Partial<ReadableSpan> {
  return span;
}

/**
 * OpenTelemetry's own AsyncLocalStorage manager, plus the context the latest
 * Given step made active. A step calls the runtime directly, outside any
 * `with` scope, and still sees that context; inside a scope the scope wins.
 */
class SteppedContextManager extends AsyncLocalStorageContextManager {
  #entered: Context = ROOT_CONTEXT;

  enter(entered: Context): void {
    this.#entered = entered;
  }

  reset(): void {
    this.#entered = ROOT_CONTEXT;
  }

  override active(): Context {
    const scoped = super.active();
    return scoped === ROOT_CONTEXT ? this.#entered : scoped;
  }
}

/** The runtime objects a Given hands over (`sink`, `list`, `lazy`) are read by the When that calls them. */
interface RuntimeWorld extends World {
  callError?: CallErrorInit;
  rejection?: unknown;
  parsed?: unknown;
  error?: BonesError;
  formatted?: FormattedError;
  span?: Span;
  sink?: JsSpanSink;
  traceparent?: string | undefined;
  activeContext?: Context;
  abort?: AbortSignal;
  callContext?: CallCtx;
  counter?: { runs: number };
  start?: () => Promise<string>;
  firstRejected?: boolean;
  lazyResults?: string[];
  wasmDir?: string;
  wasmBytes?: Uint8Array;
  wasmInputResult?: URL | Uint8Array;
  refusal?: unknown;
  exampleLocales?: string[];
  corpusLocales?: string[];
  catalogLocales?: string[];
}

/** Names the runtime whose Intl data the expected texts were derived from, so a mismatch can be read against it. Diagnostic only. */
BeforeAll(function () {
  const { node, icu, cldr, tz } = process.versions;
  process.stderr.write(`sdk-runtime formatting runtime: node ${node} icu ${icu} cldr ${cldr} tz ${tz}\n`);
});

let lane = new SteppedContextManager();

/** Every scenario runs under OpenTelemetry's own context manager and W3C propagator, as a host registers them. */
Before({ tags: "@sdk-runtime" }, function () {
  lane = new SteppedContextManager().enable();
  context.setGlobalContextManager(lane);
  propagation.setGlobalPropagator(new W3CTraceContextPropagator());
});

After({ tags: "@sdk-runtime" }, async function (this: RuntimeWorld) {
  if (this.wasmDir !== undefined) await rm(this.wasmDir, { recursive: true, force: true });
  lane.reset();
  context.disable();
  propagation.disable();
});

// --- errors ---

type CallErrorInit = MessageInitShape<typeof CallErrorSchema>;
type ErrorArgInit = MessageInitShape<typeof ErrorArgSchema>;

/** The `google.protobuf.Timestamp` of an RFC 3339 UTC instant, keeping nanosecond digits. */
function timestampOf(text: string): { seconds: bigint; nanos: number } {
  const match = /^(.*?)(?:\.(\d{1,9}))?Z$/.exec(text);
  assert.ok(match !== null, `not a UTC instant: ${text}`);
  const millis = Date.parse(`${match[1] ?? ""}Z`);
  assert.ok(!Number.isNaN(millis), `not a UTC instant: ${text}`);
  return { seconds: BigInt(Math.floor(millis / 1000)), nanos: Number((match[2] ?? "").padEnd(9, "0")) };
}

/** The `bones.v1.ErrorArg` a kind and a value stand for; a kind naming no member of the oneof gives an argument with none set. */
function errorArg(kind: string, value: string | number): ErrorArgInit {
  switch (kind) {
    case "string":
      return { value: { case: "string", value: String(value) } };
    case "int64":
      return { value: { case: "int64", value: BigInt(value) } };
    case "double":
      return { value: { case: "double", value: Number(value) } };
    case "timestamp":
      return { value: { case: "timestamp", value: timestampOf(String(value)) } };
    default:
      return {};
  }
}

/** The arguments a feature row writes as JSON, `{"<name>":{"kind":"<kind>","value":<value>}}`. */
function argsOfJson(json: string): Record<string, ErrorArgInit> {
  const written = JSON.parse(json) as Record<string, { kind: string; value: string | number }>;
  return Object.fromEntries(Object.entries(written).map(([name, { kind, value }]) => [name, errorArg(kind, value)]));
}

function callErrorOf(world: RuntimeWorld): CallErrorInit {
  assert.ok(world.callError !== undefined, "no call error given");
  return world.callError;
}

Given(
  "a call error with code {string} from emitter {string} of platform {string}, connect code {string} and message {string}",
  function (this: RuntimeWorld, code: string, emitter: string, platform: string, connectCode: string, message: string) {
    this.callError = { code, emitter, platform, connectCode, message };
  },
);

Given("the call error carries the arguments:", function (this: RuntimeWorld, table: DataTable) {
  const args: Record<string, ErrorArgInit> = {};
  for (const row of table.hashes()) args[row["name"] ?? ""] = errorArg(row["kind"] ?? "", row["value"] ?? "");
  this.callError = { ...callErrorOf(this), args };
});

Given("the call error names a wait of {int} ms", function (this: RuntimeWorld, millis: number) {
  const retryAfter = { seconds: BigInt(Math.floor(millis / 1000)), nanos: (millis % 1000) * 1_000_000 };
  this.callError = { ...callErrorOf(this), retryAfter };
});

Given(
  "the call error names a list failure of kind {string} after {int} pages and {int} items",
  function (this: RuntimeWorld, kind: string, pages: number, items: number) {
    const kinds: Record<string, PageFailure_Kind> = {
      cursor_invalid: PageFailure_Kind.CURSOR_INVALID,
      cursor_expired: PageFailure_Kind.CURSOR_EXPIRED,
      cursor_query_mismatch: PageFailure_Kind.CURSOR_QUERY_MISMATCH,
      rpc: PageFailure_Kind.RPC,
    };
    const page = { kind: kinds[kind] ?? PageFailure_Kind.UNSPECIFIED, deliveredPages: pages, deliveredItems: BigInt(items) };
    this.callError = { ...callErrorOf(this), page };
  },
);

Given("the host rejected a call with an Error saying {string}", function (this: RuntimeWorld, text: string) {
  this.rejection = new Error(text);
});

When("the call error is revived", function (this: RuntimeWorld) {
  const revived = mapCallError(toBinary(CallErrorSchema, create(CallErrorSchema, callErrorOf(this))));
  this.parsed = revived;
});

When("the host rejection is mapped", function (this: RuntimeWorld) {
  const mapped = mapCallError(this.rejection);
  this.parsed = mapped;
});

Then(
  "the result is a BonesError that is not a PageError, with code {string}, emitter {string}, platform {string}, connect code {string} and message {string}",
  function (this: RuntimeWorld, code: string, emitter: string, platform: string, connectCode: string, message: string) {
    const error = this.parsed;
    assert.ok(error instanceof BonesError);
    assert.ok(!(error instanceof PageError));
    assert.equal(error.code, code);
    assert.equal(error.emitter, emitter);
    assert.equal(error.platform, platform);
    assert.equal(error.connectCode, connectCode);
    assert.equal(error.message, message);
  },
);

Then("the error names a wait of {int} ms", function (this: RuntimeWorld, millis: number) {
  assert.ok(this.parsed instanceof BonesError, "no call error revived");
  assert.equal(this.parsed.retryAfterMs, millis);
});

Then("the error names no wait", function (this: RuntimeWorld) {
  assert.ok(this.parsed instanceof BonesError, "no call error revived");
  assert.equal(this.parsed.retryAfterMs, null);
});

Then(
  "the result is a PageError of kind {string} after {int} pages and {int} items",
  function (this: RuntimeWorld, kind: string, pages: number, items: number) {
    const error = this.parsed;
    assert.ok(error instanceof PageError, "not a PageError");
    assert.equal(error.kind, kind);
    assert.equal(error.deliveredPages, pages);
    assert.equal(error.deliveredItems, items);
  },
);

Then("the mapped value is the very Error the host rejected with, and no BonesError", function (this: RuntimeWorld) {
  assert.ok(this.rejection instanceof Error, "no host rejection given");
  assert.equal(this.parsed, this.rejection);
  assert.ok(!(this.parsed instanceof BonesError));
});

function describeArg(value: unknown): string {
  if (typeof value === "string") return `string ${value}`;
  if (typeof value === "bigint") return `bigint ${value.toString()}`;
  if (typeof value === "number") return `number ${String(value)}`;
  if (value instanceof Date) return `date ${value.toISOString()}`;
  return "unrecognised";
}

Then("argument {string} is revived as {string}", function (this: RuntimeWorld, name: string, revived: string) {
  assert.ok(this.parsed instanceof BonesError, "no call error revived");
  const args = this.parsed.args;
  const value = Object.prototype.hasOwnProperty.call(args, name) ? args[name] : undefined;
  assert.equal(value === undefined ? "dropped" : describeArg(value), revived);
});

Then("the cursor rejection check answers {string}", function (this: RuntimeWorld, answer: string) {
  assert.ok(this.parsed instanceof BonesError, "no call error revived");
  const rejected = isCursorRejected(this.parsed);
  assert.equal(String(rejected), answer);
});

// --- error formatting ---

/** The emitters a recorded catalog holds messages for: the segment of each key before `.errors.`. */
function emittersOf(messages: ErrorCatalog): string[] {
  return [...new Set(Object.keys(messages).map((key) => key.split(".errors.")[0] ?? ""))];
}

Given("the error catalog for locale {string} is registered from recorded file {string}", async function (this: RuntimeWorld, locale: string, name: string) {
  const messages = await recorded<ErrorCatalog>(name);
  for (const emitter of emittersOf(messages)) registerErrorCatalog(emitter, locale, messages);
});

Given(
  "an error {string} from emitter {string} of platform {string} with the typed arguments {string}",
  function (this: RuntimeWorld, code: string, emitter: string, platform: string, args: string) {
    const init: CallErrorInit = { code, emitter, platform, connectCode: "unknown", message: "refused", args: argsOfJson(args) };
    const error = mapCallError(toBinary(CallErrorSchema, create(CallErrorSchema, init)));
    assert.ok(error instanceof BonesError, "the call error was not revived");
    this.error = error;
  },
);

Given("the platform catalog for locale {string} is registered", async function (locale: string) {
  const messages = await jsonAt<ErrorCatalog>(PLATFORM_CATALOGS, locale);
  registerErrorCatalog("bones", locale, messages);
});

Given("the corpus catalog for locale {string} is registered", async function (locale: string) {
  const messages = await jsonAt<ErrorCatalog>(CORPUS, locale);
  registerErrorCatalog("corpus", locale, messages);
});

When("the error is formatted for locale {string}", function (this: RuntimeWorld, locale: string) {
  assert.ok(this.error !== undefined, "no error given");
  const formatted = formatError(this.error, locale);
  this.formatted = formatted;
});

When("the error is formatted for locale {string} in the time zone {string}", function (this: RuntimeWorld, locale: string, timeZone: string) {
  assert.ok(this.error !== undefined, "no error given");
  const formatted = formatError(this.error, locale, { timeZone });
  this.formatted = formatted;
});

When("the error is formatted for locale {string} in the unknown time zone {string}", function (this: RuntimeWorld, locale: string, timeZone: string) {
  assert.ok(this.error !== undefined, "no error given");
  try {
    const formatted = formatError(this.error, locale, { timeZone });
    this.formatted = formatted;
  } catch (refusal) {
    this.refusal = refusal;
  }
});

/** Zs other than the plain space, and Cf: characters a reader cannot tell apart, so a row writes them as an escape. */
const INVISIBLE = /[\p{Zs}\p{Cf}]/u;
const ESCAPE = /\\u\{([0-9A-F]{1,6})\}/g;

Then("the formatted text is {string}", function (this: RuntimeWorld, text: string) {
  for (const character of text) {
    assert.ok(character === " " || !INVISIBLE.test(character), `the expected text holds the invisible character U+${character.codePointAt(0)?.toString(16).toUpperCase()}; write it as an escape`);
  }
  const expected = text.replace(ESCAPE, (_, hex: string) => String.fromCodePoint(parseInt(hex, 16)));
  assert.equal(this.formatted?.text, expected);
});

Then("the formatted text is the platform text of the error in the catalog of locale {string}", async function (this: RuntimeWorld, locale: string) {
  assert.ok(this.error !== undefined && this.formatted !== undefined, "no error was formatted");
  const messages = await jsonAt<ErrorCatalog>(PLATFORM_CATALOGS, locale);
  const expected = messages[`bones.errors.${this.error.platform}.title`];
  assert.ok(expected !== undefined, `the ${locale} catalog has no text for platform ${this.error.platform}`);
  assert.equal(this.formatted.text, expected);
});

Then(
  "the time zone is refused as {string} with rule {string} and message {string}",
  function (this: RuntimeWorld, name: string, rule: string, message: string) {
    const refusal = this.refusal;
    assert.ok(refusal instanceof FormatErrorOptionError, "the time zone was not refused");
    assert.equal(refusal.name, name);
    assert.equal(refusal.rule, rule);
    assert.equal(refusal.message, message);
  },
);

// --- locale coverage ---

Given("the locales of the examples of the scenario outline {string}", async function (this: RuntimeWorld, outline: string) {
  this.exampleLocales = await exampleLocales(outline);
});

Given("the locales of the corpus files", async function (this: RuntimeWorld) {
  this.corpusLocales = await localesOf(CORPUS);
});

Given("the locales of the platform catalog files", async function (this: RuntimeWorld) {
  this.catalogLocales = await localesOf(PLATFORM_CATALOGS);
});

Then("the locales of the examples, the corpus files and the platform catalog files are the same set", function (this: RuntimeWorld) {
  assert.deepEqual(this.exampleLocales, this.catalogLocales);
  assert.deepEqual(this.corpusLocales, this.catalogLocales);
});

// --- call context ---

/** Makes a remote span the active context of the steps that follow, and returns that context. */
function activate(traceId: string, spanId: string, traceState?: string): Context {
  const entered = trace.setSpanContext(ROOT_CONTEXT, {
    traceId,
    spanId,
    traceFlags: 1,
    isRemote: true,
    ...(traceState === undefined ? {} : { traceState: createTraceState(traceState) }),
  });
  lane.enter(entered);
  return entered;
}

Given("no trace is active", function () {
  assert.equal(trace.getSpanContext(context.active()), undefined);
});

Given("a remote span with trace id {string} and span id {string} is active", function (this: RuntimeWorld, traceId: string, spanId: string) {
  const entered = activate(traceId, spanId);
  this.activeContext = entered;
});

Given(
  "a remote span with trace id {string}, span id {string} and trace state {string} is active",
  function (this: RuntimeWorld, traceId: string, spanId: string, traceState: string) {
    const entered = activate(traceId, spanId, traceState);
    this.activeContext = entered;
  },
);

Given("the caller holds an abort signal", function (this: RuntimeWorld) {
  this.abort = new AbortController().signal;
});

When("the call context is requested", function (this: RuntimeWorld) {
  const callContext = callCtx(this.abort);
  this.callContext = callContext;
});

Then("the call context holds the traceparent {string}", function (this: RuntimeWorld, traceparent: string) {
  assert.equal(this.callContext?.traceparent, traceparent);
});

Then("the call context holds the tracestate {string}", function (this: RuntimeWorld, tracestate: string) {
  assert.equal(this.callContext?.tracestate, tracestate);
});

Then("the call context holds the caller's abort signal", function (this: RuntimeWorld) {
  assert.ok(this.abort !== undefined);
  assert.equal(this.callContext?.signal, this.abort);
});

Then("the call context holds nothing", function (this: RuntimeWorld) {
  assert.deepEqual(this.callContext, {});
});

// --- spans ---

Given("spans are recorded by an OpenTelemetry tracer", function (this: RuntimeWorld) {
  const provider = new BasicTracerProvider();
  this.sink = new JsSpanSink(provider.getTracer("sdk-runtime-contract"));
});

Given("spans are sampled out", function (this: RuntimeWorld) {
  const provider = new BasicTracerProvider({ sampler: new AlwaysOffSampler() });
  this.sink = new JsSpanSink(provider.getTracer("sdk-runtime-contract"));
});

type SpanStartExtras = Pick<MessageInitShape<typeof SpanStartSchema>, "parent" | "tracestate" | "attributes">;

/** The `bones.sdk.v1.SpanStart` bytes the core reports, as the sink takes them. */
function startBytes(name: string, extras: SpanStartExtras = {}): Uint8Array {
  return toBinary(SpanStartSchema, create(SpanStartSchema, { name, startTime: timestampFromMs(Date.now()), kind: SpanStart_Kind.CLIENT, ...extras }));
}

/** What the sink answered at a start: the traceparent of the open span, or none when the span is not recording. */
function traceparentOf(answer: Uint8Array): string | undefined {
  const started = fromBinary(SpanStartedSchema, answer);
  return started.recording ? started.traceparent : undefined;
}

const END_STATUS: Record<string, SpanEnd_Status> = { ok: SpanEnd_Status.OK, error: SpanEnd_Status.ERROR, unset: SpanEnd_Status.UNSET };

/** The `bones.sdk.v1.SpanEnd` bytes the core reports, as the sink takes them. */
function endBytes(traceparent: string, status: string, attributes?: Record<string, number>): Uint8Array {
  const recorded = Object.entries(attributes ?? {}).map(([name, value]) => [name, { value: { case: "int64" as const, value: BigInt(value) } }] as const);
  return toBinary(
    SpanEndSchema,
    create(SpanEndSchema, {
      traceparent,
      endTime: timestampFromMs(Date.now()),
      status: END_STATUS[status] ?? SpanEnd_Status.UNSPECIFIED,
      ...(status === "error" ? { error: "unavailable SERVICE_UNAVAILABLE" } : {}),
      attributes: Object.fromEntries(recorded),
    }),
  );
}

/** Makes the started span the world's current one when it is recording. */
function adoptStarted(world: RuntimeWorld, answer: Uint8Array): void {
  const traceparent = traceparentOf(answer);
  world.traceparent = traceparent;
  world.span = traceparent === undefined || world.sink === undefined ? undefined : world.sink.lookup(traceparent);
}

function opened(span: Span | undefined): Span {
  assert.ok(span !== undefined, "no open span for the traceparent");
  return span;
}

When("the core starts the client span {string}", function (this: RuntimeWorld, name: string) {
  assert.ok(this.sink !== undefined, "spans are not recorded");
  const started = this.sink.start(startBytes(name));
  const traceparent = traceparentOf(started);
  this.traceparent = traceparent;
  this.span = traceparent === undefined ? undefined : this.sink.lookup(traceparent);
});

When("the core starts the client span {string} under the parent {string}", function (this: RuntimeWorld, name: string, parent: string) {
  assert.ok(this.sink !== undefined, "spans are not recorded");
  const started = this.sink.start(startBytes(name, { parent }));
  adoptStarted(this, started);
});

When(
  "the core starts the client span {string} under the parent {string} with the trace state {string}",
  function (this: RuntimeWorld, name: string, parent: string, tracestate: string) {
    assert.ok(this.sink !== undefined, "spans are not recorded");
    const started = this.sink.start(startBytes(name, { parent, tracestate }));
    adoptStarted(this, started);
  },
);

Given("a client span {string} is open", function (this: RuntimeWorld, name: string) {
  assert.ok(this.sink !== undefined, "spans are not recorded");
  const started = this.sink.start(startBytes(name));
  adoptStarted(this, started);
  assert.notEqual(this.traceparent, undefined, "the span was not recording");
});

Given(
  "a client span {string} is open with the attribute {string} set to {int}",
  function (this: RuntimeWorld, name: string, attribute: string, value: number) {
    const attributes = { [attribute]: { value: { case: "int64" as const, value: BigInt(value) } } };
    assert.ok(this.sink !== undefined, "spans are not recorded");
    const started = this.sink.start(startBytes(name, { attributes }));
    adoptStarted(this, started);
    assert.notEqual(this.traceparent, undefined, "the span was not recording");
  },
);

Then("the start answered the traceparent of a span that is still open", function (this: RuntimeWorld) {
  const traceparent = this.traceparent;
  assert.ok(traceparent !== undefined && /^00-[0-9a-f]{32}-[0-9a-f]{16}-[0-9a-f]{2}$/.test(traceparent), `not a traceparent: ${String(traceparent)}`);
  assert.ok(this.sink !== undefined, "spans are not recorded");
  const own = opened(this.sink.lookup(traceparent)).spanContext();
  assert.equal(traceparent, `00-${own.traceId}-${own.spanId}-${own.traceFlags.toString(16).padStart(2, "0")}`);
  assert.equal(readableOf(opened(this.span)).ended, false);
});

Then("the start answered that the span is not recording", function (this: RuntimeWorld) {
  assert.equal(this.traceparent, undefined);
  assert.equal(this.span, undefined);
});

Then("the started span is a new root outside the active trace", function (this: RuntimeWorld) {
  assert.ok(this.sink !== undefined && this.traceparent !== undefined, "no span started");
  const open = opened(this.sink.lookup(this.traceparent));
  assert.equal(readableOf(open).parentSpanContext, undefined);
  const active = this.activeContext === undefined ? undefined : trace.getSpanContext(this.activeContext);
  assert.ok(active !== undefined, "no trace is active");
  assert.notEqual(open.spanContext().traceId, active.traceId);
});

Then(
  "the started span joins the trace {string} as a child of the span {string}",
  function (this: RuntimeWorld, traceId: string, parentSpanId: string) {
    assert.ok(this.sink !== undefined && this.traceparent !== undefined, "no span started");
    const open = opened(this.sink.lookup(this.traceparent));
    assert.ok(this.traceparent.startsWith(`00-${traceId}-`), `the returned traceparent is outside the trace: ${this.traceparent}`);
    assert.equal(open.spanContext().traceId, traceId);
    assert.equal(readableOf(open).parentSpanContext?.spanId, parentSpanId);
    assert.notEqual(open.spanContext().spanId, parentSpanId);
  },
);

Then("the started span carries the trace state {string}", function (this: RuntimeWorld, traceState: string) {
  assert.ok(this.sink !== undefined && this.traceparent !== undefined, "no span started");
  const open = opened(this.sink.lookup(this.traceparent));
  assert.equal(open.spanContext().traceState?.serialize(), traceState);
});

Then("the client span {string} is still open", function (this: RuntimeWorld, name: string) {
  assert.ok(this.sink !== undefined && this.traceparent !== undefined, "no span started");
  const open = opened(this.sink.lookup(this.traceparent));
  assert.equal(readableOf(open).name, name);
});

When("the core ends the span with status {string}", function (this: RuntimeWorld, status: string) {
  assert.ok(this.sink !== undefined && this.traceparent !== undefined, "no span started");
  this.sink.end(endBytes(this.traceparent, status));
});

When(
  "the core ends the span with status {string} and the attribute {string} set to {int}",
  function (this: RuntimeWorld, status: string, attribute: string, value: number) {
    assert.ok(this.sink !== undefined && this.traceparent !== undefined, "no span started");
    this.sink.end(endBytes(this.traceparent, status, { [attribute]: value }));
  },
);

When("the core ends a span named by the unknown traceparent {string}", function (this: RuntimeWorld, traceparent: string) {
  assert.ok(this.sink !== undefined, "spans are not recorded");
  this.sink.end(endBytes(traceparent, "ok"));
});

/** The span the start returned, read as the recorded span once the core has ended it. */
function endedSpan(world: RuntimeWorld, name: string): Partial<ReadableSpan> {
  assert.ok(world.span !== undefined, "no span started");
  const recorded = readableOf(world.span);
  assert.equal(recorded.name, name);
  assert.equal(recorded.ended, true, `the span ${name} has not ended`);
  return recorded;
}

Then("the finished span {string} has status {string}", function (this: RuntimeWorld, name: string, recordedStatus: string) {
  const code = endedSpan(this, name).status?.code;
  assert.equal(code === undefined ? undefined : SpanStatusCode[code], recordedStatus);
});

Then("the finished span {string} has the status message {string}", function (this: RuntimeWorld, name: string, message: string) {
  assert.equal(endedSpan(this, name).status?.message, message);
});

Then(
  "the finished span {string} has the attribute {string} set to {int}",
  function (this: RuntimeWorld, name: string, attribute: string, value: number) {
    assert.equal(endedSpan(this, name).attributes?.[attribute], value);
  },
);

Then("no span has finished", function (this: RuntimeWorld) {
  assert.equal(readableOf(opened(this.span)).ended, false);
});

// --- lazy start ---

/** A start that counts its runs and, when told to, fails its first run. */
function countedStart(counter: { runs: number }, failFirst: boolean): () => Promise<string> {
  return async () => {
    counter.runs += 1;
    await Promise.resolve();
    if (failFirst && counter.runs === 1) throw new Error("the first start failed");
    return "started";
  };
}

/** Asks one lazy start `times` times together and gathers what each caller received. */
function askConcurrently<T>(lazy: () => Promise<T>, times: number): Promise<T[]> {
  return Promise.all(Array.from({ length: times }, () => lazy()));
}

Given("a start that counts how often it runs", function (this: RuntimeWorld) {
  const counter = { runs: 0 };
  this.counter = counter;
  this.start = countedStart(counter, false);
});

When("the lazy start is called {int} times at once", async function (this: RuntimeWorld, times: number) {
  assert.ok(this.start !== undefined, "no start given");
  const results = await askConcurrently(lazyOnce(this.start), times);
  this.lazyResults = results;
});

Given("a start that fails its first run", function (this: RuntimeWorld) {
  const counter = { runs: 0 };
  this.counter = counter;
  this.start = countedStart(counter, true);
});

When("the lazy start is called twice in turn", async function (this: RuntimeWorld) {
  assert.ok(this.start !== undefined, "no start given");
  const lazy = lazyOnce(this.start);
  await assert.rejects(lazy(), /the first start failed/);
  this.firstRejected = true;
  this.lazyResults = [await lazy()];
});

Then(
  "the start ran twice, the first call was rejected and the second received {string}",
  function (this: RuntimeWorld, result: string) {
    assert.equal(this.counter?.runs, 2);
    assert.equal(this.firstRejected, true);
    assert.deepEqual(this.lazyResults, [result]);
  },
);

Then("the start ran once and every call received its one result", function (this: RuntimeWorld) {
  assert.equal(this.counter?.runs, 1);
  assert.deepEqual(this.lazyResults, ["started", "started", "started"]);
});

// --- wasm loader input ---

Given("a wasm module file on disk", async function (this: RuntimeWorld) {
  this.wasmDir = await mkdtemp(join(tmpdir(), "sdk-runtime-wasm-"));
  this.wasmBytes = Uint8Array.from([0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]);
  await writeFile(join(this.wasmDir, "module.wasm"), this.wasmBytes);
});

When("the loader input is requested for that file", async function (this: RuntimeWorld) {
  if (this.wasmDir === undefined) throw new Error("no wasm file given");
  const input = await wasmInput(pathToFileURL(join(this.wasmDir, "module.wasm")));
  this.wasmInputResult = input;
});

Then("the loader input is the bytes of the file", function (this: RuntimeWorld) {
  const input = this.wasmInputResult;
  assert.ok(input instanceof Uint8Array, "the input is not bytes");
  assert.deepEqual(Array.from(input), Array.from(this.wasmBytes ?? []));
});
