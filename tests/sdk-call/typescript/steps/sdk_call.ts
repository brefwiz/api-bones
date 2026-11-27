// SPDX-License-Identifier: MIT
/**
 * The TypeScript answer to `tests/sdk-call/features/sdk_call.feature`.
 *
 * Every call goes through the generated wrapper of the test service
 * (`@brefwiz/api-bones-test-service-sdk`) over the real wasm Layer 2, against a
 * real server on loopback that records what each request carried. The steps
 * hold no generated Layer 1, no `createClient` and no `@connectrpc/connect`
 * import: the server is a separate process, reached over its control endpoint
 * (`./provider_process.ts`).
 */

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { setTimeout as delay } from "node:timers/promises";
import { setFlagsFromString } from "node:v8";
import { runInNewContext } from "node:vm";

import { After, AfterAll, Before, BeforeAll, Given, Then, When, World, setWorldConstructor } from "@cucumber/cucumber";
import { ROOT_CONTEXT, SpanKind, context, propagation, trace } from "@opentelemetry/api";
import type { Context, Span } from "@opentelemetry/api";
import { AsyncLocalStorageContextManager } from "@opentelemetry/context-async-hooks";
import { W3CTraceContextPropagator } from "@opentelemetry/core";
import { BasicTracerProvider, SamplingDecision } from "@opentelemetry/sdk-trace-base";
import type { ReadableSpan, Sampler, SpanProcessor } from "@opentelemetry/sdk-trace-base";
import type { Credential, CredentialAsk, SdkSession } from "@brefwiz/api-bones-sdk-runtime";
import type { Patch } from "@brefwiz/api-bones-test-service";
import { apiBonesSdk, clamp, init, maxPageSize } from "@brefwiz/api-bones-test-service-sdk";

import { Provider } from "./provider_process.ts";
import type { CallerChange } from "./provider_process.ts";

// --- the caller's tracer ---

/** Whether the tracer records; a span that is not recording gives no traceparent. */
const recording = { on: true };

const sampler: Sampler = {
  shouldSample: () => ({ decision: recording.on ? SamplingDecision.RECORD_AND_SAMPLED : SamplingDecision.NOT_RECORD }),
  toString: () => "scenario-toggle",
};

/** Every span the core's sink opened and ended through the global tracer. */
const spans: { started: ReadableSpan[]; ended: ReadableSpan[] } = { started: [], ended: [] };

const processor: SpanProcessor = {
  onStart: (span) => void spans.started.push(span as unknown as ReadableSpan),
  onEnd: (span) => void spans.ended.push(span),
  forceFlush: () => Promise.resolve(),
  shutdown: () => Promise.resolve(),
};

// The wrapper's span sink is module-level on the global tracer, so the global
// provider is registered once for the whole run.
trace.setGlobalTracerProvider(new BasicTracerProvider({ sampler, spanProcessors: [processor] }));

// --- the server process ---

const provider = new Provider();

BeforeAll({ timeout: 120_000 }, async function () {
  await provider.start();
});

AfterAll(async function () {
  await provider.stop();
});

// --- the world of one scenario ---

type Sdk = Awaited<ReturnType<typeof apiBonesSdk>>;
type NumbersList = ReturnType<Sdk["numbersClient"]["list"]>;
type NumbersPage = Awaited<ReturnType<NumbersList["firstPage"]>>;
type Chunk = readonly unknown[];

/** The outcome of a promise that did not throw into the step. */
type Settled<T> = { ok: true; value: T } | { ok: false; error: unknown };

function settle<T>(promise: Promise<T>): Promise<Settled<T>> {
  return promise.then(
    (value) => ({ ok: true as const, value }),
    (error: unknown) => ({ ok: false as const, error }),
  );
}

/** A refusal as a line for an assertion message: its class, code and text. */
export const describe = (error: unknown): string =>
  error instanceof Error ? `${error.name} ${(error as { code?: string }).code ?? ""}: ${error.message}` : String(error);

const numbersIn = (list: string): number[] => list.split(",").filter((number) => number !== "").map(Number);

/** What a scenario scripts on the server beyond the shared control patch. */
type ScriptedPatch = Patch & { unauthenticatedAfterTick?: boolean };

/** What the caller's credential source answers when a server refused its credential and the core asks it to refresh. */
type OnRefresh = { kind: "renew" } | { kind: "unchanged" } | { kind: "another"; subject: string; org: string };

/** Waits `millis`, unless the call that waits for the answer ended first. */
function pause(millis: number, signal: AbortSignal): Promise<void> {
  return new Promise<void>((resolve, reject) => {
    if (signal.aborted) {
      reject(signal.reason);
      return;
    }
    const onAbort = (): void => {
      clearTimeout(timer);
      reject(signal.reason);
    };
    const timer = setTimeout(() => {
      signal.removeEventListener("abort", onAbort);
      resolve();
    }, millis);
    signal.addEventListener("abort", onAbort, { once: true });
  });
}

const joinNumbers = (numbers: readonly number[]): string => numbers.join(",");

/** Everything one scenario holds: the caller, the clients it drives and what they gave back. */
export class SdkCallWorld extends World {
  readonly provider = provider;
  /** Who the caller is, as its credential source answers. */
  caller = { subject: "", org: "", authorization: "Bearer caller" };
  anonymous = false;
  /** The page size the numbers client asks for when a call names none. */
  defaultPageSize: number | undefined;
  sourceBroken = false;
  /** How long the credential source takes to answer, in milliseconds. */
  credentialDelayMs = 0;
  /** What the credential source answers when it is asked to refresh. */
  onRefresh: OnRefresh = { kind: "renew" };
  /** How many times the source was read, and how many times it was asked to refresh. */
  reads = 0;
  refreshes = 0;
  /** The credential the source answered last. */
  answered: Credential | undefined;
  changeBeforeSend: string | undefined;
  /** How many times the host was told a server refused the caller. */
  told = 0;
  /** Who the caller was when the revision cache was filled. */
  cacheOwner: { subject: string; org: string } | undefined;
  /** Every caller that filled the revision cache, in order. */
  cacheOwners: { subject: string; org: string }[] = [];
  /** The caller's trace, when the scenario puts it inside one. */
  trace: Context | undefined;

  /** The SDK the scenario opened from the caller's session, once it needs one. */
  opened: Sdk | undefined;
  /** An SDK of the same server at another address. */
  other: Sdk | undefined;

  widget: { id: string; name: string } | undefined;
  /** What each of the widget calls the caller made at once gave back. */
  concurrent: PromiseSettledResult<{ id: string; name: string }>[] = [];
  batch: Chunk | undefined;
  /** The batch the caller opened and has not read yet. */
  openedBatch: AsyncIterator<Chunk> | undefined;
  list: NumbersList | undefined;
  walk: AsyncIterator<NumbersPage> | undefined;
  walkSpan: Span | undefined;
  pending: Promise<Settled<IteratorResult<NumbersPage>>> | undefined;
  pagesRead: number[][] = [];
  pagesCached: boolean[] = [];
  partPages: string[][] = [];
  /** The parts of those pages as the SDK returned them. */
  receivedParts: Array<{ $typeName: string; kind: number }> = [];
  /** The pages of the walk that filled the revision cache, while the caller still holds them. */
  held: unknown[] = [];
  walkOver = false;
  /** How many ticks the events server streams. */
  tickCount = 0;
  ticks: number[] = [];
  watch: AsyncIterator<{ sequence: number }> | undefined;
  watchOver = false;
  watched = 0;
  /** Milliseconds from the start of the watch to each tick it delivered. */
  tickAt: number[] = [];
  refusal: unknown;
  /** What the last static function or constant the caller used answered. */
  staticAnswer: number | undefined;
  /** What a fresh process answered to a static call made before the SDK's init. */
  fresh: { before: string } | undefined;
  /** The items the caller walked, and the first page it read alone. */
  items: number[] = [];
  first: NumbersPage | undefined;
  /** The walk the caller left, which must give nothing more. */
  left: AsyncIterator<unknown> | undefined;

  /** The caller's session: the credential source every client of an SDK opened from it asks, and the notice that a server refused it. */
  session(): SdkSession {
    return {
      credential: this.anonymous ? "anonymous" : (ask) => this.answer(ask),
      onUnauthenticated: () => {
        this.told += 1;
      },
    };
  }

  /** What the credential source answers to one ask of the core. */
  async answer({ refresh, signal }: CredentialAsk): Promise<Credential> {
    if (this.credentialDelayMs > 0) await pause(this.credentialDelayMs, signal);
    if (this.sourceBroken) throw new Error("the credential source is down");
    if (refresh) {
      this.refreshes += 1;
      const same = this.answered;
      if (this.onRefresh.kind === "unchanged" && same !== undefined) return this.answers(same.subject, same.org, same.headers.authorization!);
      this.caller.authorization = "Bearer refreshed";
      if (this.onRefresh.kind === "another") {
        this.caller.subject = this.onRefresh.subject;
        this.caller.org = this.onRefresh.org;
      }
      return this.answers(this.caller.subject, this.caller.org, this.caller.authorization);
    }
    const read = this.reads;
    this.reads += 1;
    const subject = this.changeBeforeSend !== undefined && read >= 1 ? this.changeBeforeSend : this.caller.subject;
    return this.answers(subject, this.caller.org, this.caller.authorization);
  }

  /** A fresh credential object, so a host never sees one it answered before mutated in place. */
  private answers(subject: string, org: string, authorization: string): Credential {
    this.answered = { subject, org, headers: { authorization } };
    return { subject, org, headers: { authorization } };
  }

  /**
   * A caller other than the one that filled the revision cache before is
   * another session of it: it gets an SDK of its own, so the earlier caller's
   * slot is not the one this session drops when its credential moves on.
   */
  startedAnew(): void {
    const earlier = this.cacheOwners.at(-1);
    if (earlier !== undefined && (earlier.subject !== this.caller.subject || earlier.org !== this.caller.org)) this.opened = undefined;
  }

  /** Applies the change of the caller the server scripted while it handled a request. */
  adopt(change: CallerChange): void {
    if (change.subject !== undefined) this.caller.subject = change.subject;
    if (change.authorization !== undefined) this.caller.authorization = change.authorization;
  }
}

setWorldConstructor(SdkCallWorld);

/** The context manager and propagator a host registers, and a clean record of the tracer. */
Before({ tags: "@sdk-call" }, async function (this: SdkCallWorld, { pickle }) {
  await provider.configure({ scenario: pickle.name });
  context.setGlobalContextManager(new AsyncLocalStorageContextManager().enable());
  propagation.setGlobalPropagator(new W3CTraceContextPropagator());
  spans.started.length = 0;
  spans.ended.length = 0;
  recording.on = true;
  provider.onCallerChange = (change) => this.adopt(change);
});

After({ tags: "@sdk-call" }, async function (this: SdkCallWorld) {
  await this.walk?.return?.().catch(() => undefined);
  await this.watch?.return?.().catch(() => undefined);
  provider.onCallerChange = undefined;
  await provider.reset();
  recording.on = true;
  context.disable();
  propagation.disable();
});

// --- scripting the server and the caller ---

Given("the widgets server has the widget {string} named {string}", async function (this: SdkCallWorld, id: string, name: string) {
  await this.provider.configure({ widgets: { [id]: name } });
});

Given(
  "the widgets server refuses the first {int} requests as resource exhausted with a Retry-After of {int} seconds",
  async function (this: SdkCallWorld, refusals: number, seconds: number) {
    await this.provider.configure({ exhausted: { count: refusals, retryAfterSeconds: seconds } });
  },
);

Given("the numbers server has the pages {string}", async function (this: SdkCallWorld, pages: string) {
  await this.provider.configure({ pages: pages.split("|").map(numbersIn) });
});

Given("the numbers server takes {int} ms to answer each page", async function (this: SdkCallWorld, millis: number) {
  await this.provider.configure({ pageDelayMs: millis });
});

Given("the widgets server fails the first {int} requests as internal", async function (this: SdkCallWorld, failures: number) {
  await this.provider.configure({ failing: failures });
});

Given("the widgets server refuses the first {int} requests as unavailable", async function (this: SdkCallWorld, refusals: number) {
  await this.provider.configure({ unavailable: refusals });
});

Given(
  "the {word} server refuses the first {int} requests as unauthenticated",
  async function (this: SdkCallWorld, _service: string, refusals: number) {
    await this.provider.configure({ unauthenticated: refusals });
  },
);

Given("the numbers server refuses the cursor of page {int} as expired", async function (this: SdkCallWorld, page: number) {
  await this.provider.configure({ expireAtPage: { page: page - 1, count: 1 } });
});

Given("the numbers server answers pages that cannot be decoded", async function (this: SdkCallWorld) {
  await this.provider.configure({ garble: true });
});

Given("the SDK caller is {string} of organization {string}", function (this: SdkCallWorld, subject: string, org: string) {
  this.caller = { subject, org, authorization: "Bearer caller" };
});

Given("the numbers client defaults to the page size {int}", function (this: SdkCallWorld, size: number) {
  this.defaultPageSize = size;
});

Given("the caller is anonymous", function (this: SdkCallWorld) {
  this.anonymous = true;
});

Given("the caller switches to {string} of organization {string}", function (this: SdkCallWorld, subject: string, org: string) {
  this.caller.subject = subject;
  this.caller.org = org;
});

Given("the caller's token is refreshed", function (this: SdkCallWorld) {
  this.caller.authorization = "Bearer refreshed";
});

Given(
  "the server refreshes the caller's token to {string} while it handles a request",
  async function (this: SdkCallWorld, authorization: string) {
    await this.provider.configure({ refreshTo: authorization });
  },
);

Given("the caller is inside the trace {string} at the span {string}", function (this: SdkCallWorld, traceId: string, spanId: string) {
  this.trace = trace.setSpanContext(ROOT_CONTEXT, { traceId, spanId, traceFlags: 1, isRemote: true });
});

Given("the caller's identity changes to {string} before the first send", function (this: SdkCallWorld, subject: string) {
  this.changeBeforeSend = subject;
});

Given("the server switches the caller to {string} while it handles a request", async function (this: SdkCallWorld, subject: string) {
  await this.provider.configure({ switchTo: subject });
});

Given(
  "the server switches the caller to {string} while it handles the request for page {int}",
  async function (this: SdkCallWorld, subject: string, page: number) {
    await this.provider.configure({ switchOnPage: { index: page - 1, subject } });
  },
);

Given("the caller's credential source cannot answer", function (this: SdkCallWorld) {
  this.sourceBroken = true;
});

/** A source that rejects and one that cannot answer are one failure to the core: it answers nothing. */
Given("the caller's credential source rejects", function (this: SdkCallWorld) {
  this.sourceBroken = true;
});

Given("the caller's credential source answers after {int} ms", function (this: SdkCallWorld, millis: number) {
  this.credentialDelayMs = millis;
});

Given("the caller's credential source answers the same credential when asked to refresh", function (this: SdkCallWorld) {
  this.onRefresh = { kind: "unchanged" };
});

Given(
  "the caller's credential source answers {string} of organization {string} when asked to refresh",
  function (this: SdkCallWorld, subject: string, org: string) {
    this.onRefresh = { kind: "another", subject, org };
  },
);

Given("the events server refuses the watch as unauthenticated after its first tick", async function (this: SdkCallWorld) {
  const patch: ScriptedPatch = { unauthenticatedAfterTick: true };
  await this.provider.configure(patch);
});

Given("the numbers server has {int} pages of {int} numbers each", async function (this: SdkCallWorld, pages: number, size: number) {
  await this.provider.configure({
    pages: Array.from({ length: pages }, (_, page) => Array.from({ length: size }, (_, at) => page * size + at + 1)),
  });
});

Given("the tracer does not record", function () {
  recording.on = false;
});

/**
 * Fills the client's revision cache the way a caller does: one complete walk of
 * the numbers list through the wrapper, its pages still held. The walk that
 * fills it is not the call under test, so what it sent and recorded is forgotten.
 */
Given("the revision cache holds the numbers list", fillNumbers);

/**
 * The same walk, as the one session whose end the next steps drive: the
 * scenario's SDK is that session.
 */
Given("the caller's session holds the numbers list", fillNumbers);

/** Records who owns the cache the walk is about to fill. */
function noteCacheOwner(world: SdkCallWorld): void {
  world.cacheOwner = { subject: world.caller.subject, org: world.caller.org };
  world.cacheOwners = [...world.cacheOwners, world.cacheOwner];
}

/** The fill is setup: the provider's request log and the read and refresh counts start clean after it. */
async function forgetFill(world: SdkCallWorld): Promise<void> {
  await world.provider.clearSeen();
  world.reads = 0;
  world.refreshes = 0;
}

/** The probe is not part of the call under test: the tracer records none of it. */
async function probeFirstPage(sdk: Sdk) {
  recording.on = false;
  const first = await sdk.numbersClient.list().firstPage();
  recording.on = true;
  return first;
}

async function fillNumbers(this: SdkCallWorld): Promise<void> {
  this.startedAnew();
  const sdk = this.opened ?? (await apiBonesSdk(this.session(), { baseUrl: this.provider.url, defaultPageSize: this.defaultPageSize }));
  this.opened = sdk;
  noteCacheOwner(this);
  for await (const page of sdk.numbersClient.list().pages()) this.held = [...this.held, page];
  await forgetFill(this);
  spans.started.length = 0;
  spans.ended.length = 0;
}

Given("the caller signs out", function (this: SdkCallWorld) {
  this.sourceBroken = true;
});

When("the caller signs in again", function (this: SdkCallWorld) {
  this.sourceBroken = false;
});

When("the caller reads the numbers list through the same session", async function (this: SdkCallWorld) {
  try {
    const sdk = this.opened ?? (await apiBonesSdk(this.session(), { baseUrl: this.provider.url, defaultPageSize: this.defaultPageSize }));
    this.opened = sdk;
    await sdk.numbersClient.list().firstPage();
  } catch (error) {
    this.refusal = error;
  }
});

/**
 * One complete walk of the numbers list that is not the call under test and
 * need not leave anything in the revision cache: what it sent, read and
 * recorded is forgotten.
 */
Given("the caller has walked the numbers list through the SDK", async function (this: SdkCallWorld) {
  const sdk = this.opened ?? (await apiBonesSdk(this.session(), { baseUrl: this.provider.url, defaultPageSize: this.defaultPageSize }));
  this.opened = sdk;
  for await (const page of sdk.numbersClient.list().pages()) void page;
  await forgetFill(this);
  spans.started.length = 0;
  spans.ended.length = 0;
});

/** What the caller held of the walk is gone; the revision cache alone keeps the chain. */
Given("the caller dropped every page and the host collected its garbage", async function (this: SdkCallWorld) {
  // The lane runs under plain cucumber-js: the collector is exposed here, and
  // a fresh context hands it over.
  setFlagsFromString("--expose-gc");
  const collect = runInNewContext("gc") as () => void;
  this.held = [];
  this.first = undefined;
  for (let round = 0; round < 3; round += 1) {
    collect();
    await delay(10);
  }
});

// --- calls ---

async function getWidget(world: SdkCallWorld, id: string): Promise<void> {
  const sdk = world.opened ?? (await apiBonesSdk(world.session(), { baseUrl: world.provider.url, defaultPageSize: world.defaultPageSize }));
  world.opened = sdk;
  try {
    world.widget = world.trace === undefined ? await sdk.widgetsClient.getWidget(id) : await context.with(world.trace, () => sdk.widgetsClient.getWidget(id));
  } catch (error) {
    world.refusal = error;
  }
}

When("the caller gets the widget {string} through the SDK", async function (this: SdkCallWorld, id: string) {
  await getWidget(this, id);
});

When("the caller gets the widget {string} through the SDK in {int} concurrent calls", async function (this: SdkCallWorld, id: string, calls: number) {
  const sdk = this.opened ?? (await apiBonesSdk(this.session(), { baseUrl: this.provider.url, defaultPageSize: this.defaultPageSize }));
  this.opened = sdk;
  this.concurrent = await Promise.allSettled(Array.from({ length: calls }, () => sdk.widgetsClient.getWidget(id)));
});

// --- the SDK opened from the caller's session ---

When("the caller opens the Api Bones SDK from the caller's session at the address of the server", async function (this: SdkCallWorld) {
  try {
    this.opened = await apiBonesSdk(this.session(), { baseUrl: this.provider.url, defaultPageSize: this.defaultPageSize });
  } catch (error) {
    this.refusal = error;
  }
});

When("the caller opens the Api Bones SDK from the caller's session without a base address", async function (this: SdkCallWorld) {
  try {
    this.opened = await apiBonesSdk(this.session(), { baseUrl: undefined, defaultPageSize: this.defaultPageSize });
  } catch (error) {
    this.refusal = error;
  }
});

When("the caller gets the widget {string} through the opened SDK", async function (this: SdkCallWorld, id: string) {
  assert.ok(this.opened, "a When opened the SDK");
  await getWidget(this, id);
});

When("the caller lists the numbers through the opened SDK", async function (this: SdkCallWorld) {
  assert.ok(this.opened, "a When opened the SDK");
  try {
    for await (const page of this.opened.numbersClient.list().pages()) this.pagesRead = [...this.pagesRead, page.items.map((number) => number.value)];
  } catch (error) {
    this.refusal = error;
  }
});

Then("the refusal says {string}", function (this: SdkCallWorld, text: string) {
  assert.ok(this.refusal, "the call was refused");
  assert.ok(describe(this.refusal).includes(text), `refused with ${describe(this.refusal)}`);
});

When("the caller gets the widgets {string} through the SDK", async function (this: SdkCallWorld, ids: string) {
  const sdk = this.opened ?? (await apiBonesSdk(this.session(), { baseUrl: this.provider.url, defaultPageSize: this.defaultPageSize }));
  this.opened = sdk;
  try {
    const items = sdk.widgetsClient.batchGetWidgets(ids.split(","))[Symbol.asyncIterator]();
    const outcome = await items.next();
    this.batch = outcome.done ? undefined : outcome.value;
    await items.return?.();
  } catch (error) {
    this.refusal = error;
  }
  if (this.refusal === undefined) assert.ok(this.batch, "the batch delivered no chunk");
});

Given("the caller has opened a batch of the widgets {string} through the SDK", async function (this: SdkCallWorld, ids: string) {
  const sdk = this.opened ?? (await apiBonesSdk(this.session(), { baseUrl: this.provider.url, defaultPageSize: this.defaultPageSize }));
  this.opened = sdk;
  this.openedBatch = sdk.widgetsClient.batchGetWidgets(ids.split(","))[Symbol.asyncIterator]();
});

When("the caller reads the batch", async function (this: SdkCallWorld) {
  const items = this.openedBatch;
  assert.ok(items, "a Given opened the batch");
  try {
    const outcome = await items.next();
    this.batch = outcome.done ? undefined : outcome.value;
    await items.return?.();
  } catch (error) {
    this.refusal = error;
  }
  if (this.refusal === undefined) assert.ok(this.batch, "the batch delivered no chunk");
});

Given("the widget {string} has the parts {string}", async function (this: SdkCallWorld, id: string, pages: string) {
  await this.provider.configure({ parts: { [id]: pages.split("|").map((page) => page.split(",").filter((part) => part !== "")) } });
});

type PartPage = { fromCache: boolean; items: Array<{ name: string; $typeName: string; kind: number }> };

/** Walks the pages to the end or the first refusal, returning what arrived. */
async function walkParts(pages: AsyncIterable<PartPage>) {
  const walked = { cached: [] as boolean[], names: [] as string[][], parts: [] as PartPage["items"], refusal: undefined as unknown };
  try {
    for await (const page of pages) {
      walked.cached.push(page.fromCache);
      walked.names.push(page.items.map((part) => part.name));
      walked.parts.push(...page.items);
    }
  } catch (error) {
    walked.refusal = error;
  }
  return walked;
}

/** Walks the parts of a widget by pages until the list ends or is refused. */
When(
  /^the caller lists the parts of the widget "([^"]*)"(?: with the page size (\d+))? through the SDK$/,
  async function (this: SdkCallWorld, id: string, size?: string) {
    const sdk = this.opened ?? (await apiBonesSdk(this.session(), { baseUrl: this.provider.url, defaultPageSize: this.defaultPageSize }));
    this.opened = sdk;
    const pages = sdk.widgetsClient.listWidgetParts(id, size === undefined ? undefined : { pageSize: Number(size) }).pages();
    const walked = await walkParts(pages);
    this.pagesCached = [...this.pagesCached, ...walked.cached];
    this.partPages = [...this.partPages, ...walked.names];
    this.receivedParts = [...this.receivedParts, ...walked.parts];
    this.refusal = walked.refusal ?? this.refusal;
  },
);

Given("the widgets server refuses the cursor of part page {int} as expired", async function (this: SdkCallWorld, page: number) {
  await this.provider.configure({ expireAtPage: { page: page - 1, count: 1 } });
});

/** One complete walk fills the revision cache; the walk that fills it is not the call under test. */
Given("the revision cache holds the parts of the widget {string}", async function (this: SdkCallWorld, id: string) {
  this.startedAnew();
  const sdk = this.opened ?? (await apiBonesSdk(this.session(), { baseUrl: this.provider.url, defaultPageSize: this.defaultPageSize }));
  this.opened = sdk;
  noteCacheOwner(this);
  for await (const page of sdk.widgetsClient.listWidgetParts(id).pages()) this.held = [...this.held, page];
  await forgetFill(this);
});

Then("every request the server saw asked for the part page size {int}", async function (this: SdkCallWorld, size: number) {
  const asked = (await seenRequests(this)).map((request) => request.pageSize);
  assert.ok(asked.length > 0, "the server saw no request");
  assert.deepEqual(asked, asked.map(() => size));
});

Then("the caller received the part pages {string}", function (this: SdkCallWorld, expected: string) {
  assert.equal(this.partPages.map((page) => page.join(",")).join("|"), expected);
});

Given("the caller has opened a list of the numbers at another address of the server through the SDK", async function (this: SdkCallWorld) {
  const other = await apiBonesSdk(this.session(), { baseUrl: await this.provider.alias(), defaultPageSize: this.defaultPageSize });
  this.other = other;
  const list = other.numbersClient.list();
  this.list = list;
  this.walk = list.pages()[Symbol.asyncIterator]();
});

Then("no page the caller received came from the revision cache", function (this: SdkCallWorld) {
  assert.ok(this.pagesCached.length > 0, "the caller received no page");
  assert.ok(this.pagesCached.every((cached) => !cached), `pages from the cache: ${this.pagesCached}`);
});

Given("the caller has opened a list of the numbers through the SDK", async function (this: SdkCallWorld) {
  const sdk = this.opened ?? (await apiBonesSdk(this.session(), { baseUrl: this.provider.url, defaultPageSize: this.defaultPageSize }));
  this.opened = sdk;
  const list = sdk.numbersClient.list();
  this.list = list;
  this.walk = list.pages()[Symbol.asyncIterator]();
});

/** The caller chooses that no page is fetched ahead of the one it reads. */
Given("the caller has opened a list of the numbers without fetching ahead through the SDK", async function (this: SdkCallWorld) {
  const sdk = this.opened ?? (await apiBonesSdk(this.session(), { baseUrl: this.provider.url, defaultPageSize: this.defaultPageSize }));
  this.opened = sdk;
  const list = sdk.numbersClient.list({ prefetch: 0 });
  this.list = list;
  this.walk = list.pages()[Symbol.asyncIterator]();
});

/** The list's own choice to fetch ahead replaces what the call chose. */
Given(
  "the caller has opened a list of the numbers without fetching ahead and then asked it to fetch ahead through the SDK",
  async function (this: SdkCallWorld) {
    const sdk = this.opened ?? (await apiBonesSdk(this.session(), { baseUrl: this.provider.url, defaultPageSize: this.defaultPageSize }));
    this.opened = sdk;
    const list = sdk.numbersClient.list({ prefetch: 0 }).withPrefetch(1);
    this.list = list;
    this.walk = list.pages()[Symbol.asyncIterator]();
  },
);

When("the caller waits {int} ms", async function (this: SdkCallWorld, millis: number) {
  await delay(millis);
});

Given("the caller has read the first/next page of the list", async function (this: SdkCallWorld) {
  const sdk = this.opened ?? (await apiBonesSdk(this.session(), { baseUrl: this.provider.url, defaultPageSize: this.defaultPageSize }));
  this.opened = sdk;
  this.walk ??= sdk.numbersClient.list().pages()[Symbol.asyncIterator]();
  const step = await this.walk.next();
  if (step.done) this.walkOver = true;
  else {
    this.pagesCached = [...this.pagesCached, step.value.fromCache];
    this.pagesRead = [...this.pagesRead, step.value.items.map((number) => number.value)];
  }
  this.walkSpan ??= this.list?.span;
});

When("the caller reads the next page of the list", async function (this: SdkCallWorld) {
  try {
    const sdk = this.opened ?? (await apiBonesSdk(this.session(), { baseUrl: this.provider.url, defaultPageSize: this.defaultPageSize }));
    this.opened = sdk;
    this.walk = this.walk ?? sdk.numbersClient.list().pages()[Symbol.asyncIterator]();
    const step = await this.walk.next();
    if (step.done) this.walkOver = true;
    else {
      this.pagesRead = [...this.pagesRead, step.value.items.map((number) => number.value)];
      this.pagesCached = [...this.pagesCached, step.value.fromCache];
    }
    this.walkSpan = this.walkSpan ?? this.list?.span;
  } catch (error) {
    this.refusal = error;
    this.walkSpan = this.walkSpan ?? this.list?.span;
  }
});

/** Starts a read of the next page that the caller does not wait for, and returns once the server has the request. */
Given("a read of the next page of the list is pending", async function (this: SdkCallWorld) {
  assert.ok(this.walk, "a Given opened the list");
  this.pending = settle(this.walk.next());
  let reached = false;
  for (let waited = 0; waited < 500 && !reached; waited += 1) {
    if ((await this.provider.seen()).length > 0) reached = true;
    else await delay(10);
  }
  assert.ok(reached, "the pending read never reached the numbers server");
  this.walkSpan ??= this.list?.span;
});

Given("the caller has cancelled the list", async function (this: SdkCallWorld) {
  assert.ok(this.walk, "a Given opened the list");
  await this.walk.return?.();
});

When("the caller cancels the list", async function (this: SdkCallWorld) {
  assert.ok(this.walk, "a Given opened the list");
  await this.walk.return?.();
});

/** The page after the first is fetched ahead once the spans of both pages have ended. */
Given("the page after it has been fetched ahead", { timeout: 15_000 }, async function (this: SdkCallWorld) {
  let fetched = false;
  for (let waited = 0; waited < 500 && !fetched; waited += 1) {
    if (spans.ended.length >= 2) fetched = true;
    else await delay(10);
  }
  const seen = await this.provider.seen();
  const named = (list: ReadableSpan[]): string => list.map((span) => span.name).join(",");
  assert.ok(
    fetched,
    `the page after the first was never fetched ahead; spans started: ${named(spans.started)}; spans ended: ${named(spans.ended)}; requests the server saw: ${JSON.stringify(seen)}`,
  );
});

/** Walks the numbers list by pages until it ends or is refused. */
When("the caller walks the numbers list by pages through the SDK", async function (this: SdkCallWorld) {
  const sdk = this.opened ?? (await apiBonesSdk(this.session(), { baseUrl: this.provider.url, defaultPageSize: this.defaultPageSize }));
  this.opened = sdk;
  try {
    const pages = sdk.numbersClient.list().pages();
    this.left = pages[Symbol.asyncIterator]();
    for await (const page of pages) {
      this.pagesCached = [...this.pagesCached, page.fromCache];
      this.pagesRead = [...this.pagesRead, page.items.map((number) => number.value)];
    }
    this.walkOver = true;
  } catch (error) {
    this.refusal = error;
  }
});

/** Walks the numbers list by items until it ends or is refused. */
When("the caller walks the numbers list by items through the SDK", async function (this: SdkCallWorld) {
  const sdk = this.opened ?? (await apiBonesSdk(this.session(), { baseUrl: this.provider.url, defaultPageSize: this.defaultPageSize }));
  this.opened = sdk;
  try {
    const items = sdk.numbersClient.list().items();
    this.left = items[Symbol.asyncIterator]();
    for await (const item of items) this.items = [...this.items, item.value];
  } catch (error) {
    this.refusal = error;
  }
});

/** Walks the numbers list by items and leaves once `limit` items were read. */
When("the caller walks the numbers list by items and leaves after {int} item(s)", async function (this: SdkCallWorld, limit: number) {
  const sdk = this.opened ?? (await apiBonesSdk(this.session(), { baseUrl: this.provider.url, defaultPageSize: this.defaultPageSize }));
  this.opened = sdk;
  try {
    const walk = sdk.numbersClient.list().items()[Symbol.asyncIterator]();
    this.left = walk;
    while (this.items.length < limit) {
      const step = await walk.next();
      if (step.done) break;
      this.items = [...this.items, step.value.value];
    }
    if (this.items.length >= limit) await walk.return?.();
  } catch (error) {
    this.refusal = error;
  }
});

When("the caller lists the numbers with the page size {int} through the SDK", async function (this: SdkCallWorld, size: number) {
  const sdk = this.opened ?? (await apiBonesSdk(this.session(), { baseUrl: this.provider.url, defaultPageSize: this.defaultPageSize }));
  this.opened = sdk;
  try {
    for await (const page of sdk.numbersClient.list({ pageSize: size }).pages()) this.pagesRead = [...this.pagesRead, page.items.map((number) => number.value)];
  } catch (error) {
    this.refusal = error;
  }
});

/** The read of the first page starts and waits for the caller's credential, which has not been answered: nothing has reached the server. */
Given("a read of the first page of the list is waiting for the caller's credential", async function (this: SdkCallWorld) {
  assert.ok(this.walk, "a Given opened the list");
  this.pending = settle(this.walk.next());
  await delay(200);
  assert.equal((await this.provider.seen()).length, 0, "a request reached the server before the credential was answered");
  this.walkSpan ??= this.list?.span;
});

When("the caller reads the first page of the numbers list through the SDK", async function (this: SdkCallWorld) {
  const sdk = this.opened ?? (await apiBonesSdk(this.session(), { baseUrl: this.provider.url, defaultPageSize: this.defaultPageSize }));
  this.opened = sdk;
  try {
    this.first = await sdk.numbersClient.list().firstPage();
  } catch (error) {
    this.refusal = error;
  }
});

/** Reading one page alone wins over a choice to fetch ahead. */
When(
  "the caller reads the first page of the numbers list through the SDK having asked to fetch ahead",
  async function (this: SdkCallWorld) {
    const sdk = this.opened ?? (await apiBonesSdk(this.session(), { baseUrl: this.provider.url, defaultPageSize: this.defaultPageSize }));
    this.opened = sdk;
    try {
      this.first = await sdk.numbersClient.list().withPrefetch(1).firstPage();
    } catch (error) {
      this.refusal = error;
    }
  },
);

Given("the events server streams {int} ticks", async function (this: SdkCallWorld, count: number) {
  this.tickCount = count;
  await this.provider.configure({ ticks: count });
});

Given("the events server waits {int} ms before each tick", async function (this: SdkCallWorld, millis: number) {
  await this.provider.configure({ tickMs: millis });
});

When("the caller watches the events through the SDK", { timeout: 120_000 }, async function (this: SdkCallWorld) {
  const sdk = this.opened ?? (await apiBonesSdk(this.session(), { baseUrl: this.provider.url, defaultPageSize: this.defaultPageSize }));
  this.opened = sdk;
  const started = Date.now();
  try {
    const watch = sdk.eventsClient.watch(this.tickCount)[Symbol.asyncIterator]();
    this.watch = watch;
    for (;;) {
      const step = await watch.next();
      if (step.done) break;
      this.ticks = [...this.ticks, step.value.sequence];
      this.tickAt = [...this.tickAt, Date.now() - started];
    }
  } catch (error) {
    this.refusal = error;
  }
  this.watched = Date.now() - started;
});

When(
  "the caller watches the events through the SDK and leaves after {int} ticks",
  { timeout: 120_000 },
  async function (this: SdkCallWorld, limit: number) {
    const began = Date.now();
    const sdk = this.opened ?? (await apiBonesSdk(this.session(), { baseUrl: this.provider.url, defaultPageSize: this.defaultPageSize }));
    this.opened = sdk;
    try {
      const watch = sdk.eventsClient.watch(this.tickCount)[Symbol.asyncIterator]();
      this.watch = watch;
      while (this.ticks.length < limit) {
        const step = await watch.next();
        if (step.done) break;
        this.ticks = [...this.ticks, step.value.sequence];
      }
      if (this.ticks.length >= limit) await watch.return?.();
    } catch (error) {
      this.refusal = error;
    }
    this.watched = Date.now() - began;
  },
);

Given("the caller has opened a watch of the events through the SDK", async function (this: SdkCallWorld) {
  const sdk = this.opened ?? (await apiBonesSdk(this.session(), { baseUrl: this.provider.url, defaultPageSize: this.defaultPageSize }));
  this.opened = sdk;
  this.watch = sdk.eventsClient.watch(this.tickCount)[Symbol.asyncIterator]();
});

Given("the caller has read the first/next tick of the watch", async function (this: SdkCallWorld) {
  assert.ok(this.watch, "a Given opened the watch");
  const step = await this.watch.next();
  if (step.done) this.watchOver = true;
  else this.ticks = [...this.ticks, step.value.sequence];
});

When("the caller reads the next tick of the watch", async function (this: SdkCallWorld) {
  assert.ok(this.watch, "a Given opened the watch");
  try {
    const step = await this.watch.next();
    if (step.done) this.watchOver = true;
    else this.ticks = [...this.ticks, step.value.sequence];
  } catch (error) {
    this.refusal = error;
    const after = await settle(this.watch.next());
    this.watchOver = after.ok && after.value.done === true;
  }
});

// --- what the caller got ---

Then("the response is the widget {string} named {string}", function (this: SdkCallWorld, id: string, name: string) {
  assert.ok(this.widget, "the call succeeded");
  assert.equal(this.widget.id, id);
  assert.equal(this.widget.name, name);
});

Then("every concurrent call answered the widget {string} named {string}", function (this: SdkCallWorld, id: string, name: string) {
  assert.ok(this.concurrent.length > 0, "a When made the calls");
  for (const outcome of this.concurrent) {
    assert.equal(outcome.status, "fulfilled", "the call succeeded");
    assert.equal(outcome.value.id, id);
    assert.equal(outcome.value.name, name);
  }
});

Then("the batch delivered the widgets {string} in request order", function (this: SdkCallWorld, expected: string) {
  assert.ok(this.batch, "the batch delivered a chunk");
  const delivered = (this.batch as readonly { ok?: { id: string; name: string } }[]).map((item) => {
    assert.ok(item.ok, "the widget was delivered");
    return `${item.ok.id}=${item.ok.name}`;
  });
  assert.equal(delivered.join(","), expected);
});

Then("the caller received the pages {string}", function (this: SdkCallWorld, expected: string) {
  assert.equal(this.pagesRead.map(joinNumbers).join("|"), expected);
});

Then("the caller received the items {string}", function (this: SdkCallWorld, expected: string) {
  assert.equal(this.items.join(","), expected);
});

Then("every page the caller received came from the revision cache", async function (this: SdkCallWorld) {
  assert.ok(this.pagesCached.length > 0, "the caller received no page");
  assert.ok(this.pagesCached.every(Boolean), `pages from the cache: ${this.pagesCached}; ${await seenNote(this)}`);
});

Then("the walk is over", function (this: SdkCallWorld) {
  assert.ok(this.walkOver, "the walk gave more after it ended");
});

Then("the walk ended without refusal", function (this: SdkCallWorld) {
  assert.equal(this.refusal, undefined, `the walk was refused: ${String(this.refusal)}`);
});

Then("the walk is refused", function (this: SdkCallWorld) {
  assert.ok(this.refusal, "the walk was not refused");
});

Then(
  "the walk is refused with a PageError of kind {string} after {int} pages and {int} items",
  function (this: SdkCallWorld, kind: string, pages: number, items: number) {
    const refusal = this.refusal as { name?: string; kind?: string; deliveredPages?: number; deliveredItems?: number } | undefined;
    assert.ok(refusal, "the walk was not refused");
    assert.equal(refusal.name, "PageError");
    assert.deepEqual([refusal.kind, refusal.deliveredPages, refusal.deliveredItems], [kind, pages, items]);
  },
);

Then("the list gives nothing after the caller left", async function (this: SdkCallWorld) {
  assert.ok(this.left, "the caller walked the list");
  assert.equal((await this.left.next()).done, true);
});

Then("the first page holds the numbers {string}", function (this: SdkCallWorld, expected: string) {
  assert.ok(this.first, "the first page was read");
  assert.equal(joinNumbers(this.first.items.map((number) => number.value)), expected);
});

Then("the list ends after its first page", async function (this: SdkCallWorld) {
  assert.ok(this.first, "the first page was read");
  assert.equal((await this.provider.seen()).length, 1, "the list asked for more than its first page");
});

Then("the first page says a next page follows", function (this: SdkCallWorld) {
  assert.equal(this.first?.hasNext, true);
});

Then("the first page says no next page follows", function (this: SdkCallWorld) {
  assert.equal(this.first?.hasNext, false);
});

Then("the numbers list exposes the span of its walk", function (this: SdkCallWorld) {
  assert.ok(this.walkSpan, "the list exposed no span");
  assert.equal(this.walkSpan.isRecording(), true);
});

Then("the numbers list exposes no span", function (this: SdkCallWorld) {
  assert.equal(this.walkSpan, undefined);
  assert.equal(this.list?.span, undefined);
});

Then("the caller received the ticks {string}", function (this: SdkCallWorld, expected: string) {
  const note = `ticks reached the caller at ${this.tickAt} ms; the watch lasted ${this.watched} ms; refusal: ${this.refusal === undefined ? "none" : describe(this.refusal)}; watch over: ${this.watchOver}`;
  assert.equal(this.ticks.join(","), expected, note);
});

Then("the watch lasted longer than the call budget", function (this: SdkCallWorld) {
  assert.ok(this.watched > 30_000, `the watch lasted ${this.watched} ms, within the budget`);
});

Then("the watch was not refused", function (this: SdkCallWorld) {
  assert.equal(this.refusal, undefined, `the watch was refused: ${String(this.refusal)}`);
});

Then(
  "the wrapper lists {string} as a {string} method of the surface {string}",
  function (this: SdkCallWorld, method: string, kind: string, surface: string) {
    const snapshot = JSON.parse(
      readFileSync(new URL("../../../../api-bones-test-service-sdk-ts/l2-methods.json", import.meta.url), "utf8"),
    ) as { methods: { rust: string; kind: string; surface: string }[] };
    const listed = snapshot.methods.filter((m) => m.rust === method);
    assert.equal(listed.length, 1, `${method} is listed once`);
    assert.equal(listed[0].kind, kind);
    assert.equal(listed[0].surface, surface);
  },
);

Then("the watch has ended", function (this: SdkCallWorld) {
  assert.ok(this.watchOver, "the watch gave more after it ended");
});

Then("the watch gives nothing after the caller left", async function (this: SdkCallWorld) {
  assert.ok(this.watch, "the caller watched the events");
  assert.equal((await this.watch.next()).done, true);
});

// --- refusals ---

type Refusal = { code?: string; platform?: string };

const refusalOf = (world: SdkCallWorld): Refusal => {
  assert.ok(world.refusal, "the call was refused");
  return world.refusal as Refusal;
};

Then("the call is refused with the code {string}", function (this: SdkCallWorld, code: string) {
  assert.equal(refusalOf(this).code, code, `refused with ${String(this.refusal)}`);
});

Then("the call is refused with the platform code {string}", function (this: SdkCallWorld, platform: string) {
  assert.equal(refusalOf(this).platform, platform, `refused with ${String(this.refusal)}`);
});

Then("the refusal names a wait of {int} ms", function (this: SdkCallWorld, millis: number) {
  assert.equal((this.refusal as { retryAfterMs?: number | null }).retryAfterMs, millis, `refused with ${String(this.refusal)}`);
});

Then("the call is refused as unauthenticated", function (this: SdkCallWorld) {
  assert.equal(refusalOf(this).platform, "UNAUTHORIZED", `refused with ${String(this.refusal)}`);
});

Then("the pending read is refused with the code {string}", async function (this: SdkCallWorld, code: string) {
  assert.ok(this.pending, "a Given left a read pending");
  const outcome = await this.pending;
  assert.ok(!outcome.ok, "the pending read was not refused");
  assert.equal((outcome.error as Refusal).code, code);
});

Then("the pending read completes with the page {string}", async function (this: SdkCallWorld, expected: string) {
  assert.ok(this.pending, "a Given left a read pending");
  const outcome = await this.pending;
  assert.ok(outcome.ok, "the pending read was refused");
  assert.ok(!outcome.value.done, "the list had a page");
  assert.equal(joinNumbers(outcome.value.value.items.map((number) => number.value)), expected);
});

// --- what the server saw ---

/** What the server saw and who the caller is, for the message of an assertion about the revision cache. */
async function seenNote(world: SdkCallWorld): Promise<string> {
  const caller = `${world.caller.subject}/${world.caller.org}`;
  return `caller ${caller}; cache owners ${world.cacheOwners.map((owner) => `${owner.subject}/${owner.org}`)}; requests the server saw: ${JSON.stringify(await world.provider.seen())}`;
}

Then("the server saw {int} request\\(s)", async function (this: SdkCallWorld, count: number) {
  assert.equal((await this.provider.seen()).length, count);
});

const parts = (traceparent: string): string[] => traceparent.split("-");
const isTraceparent = (traceparent: string): boolean => /^00-[0-9a-f]{32}-[0-9a-f]{16}-[0-9a-f]{2}$/.test(traceparent);

async function seenRequests(world: SdkCallWorld) {
  const seen = await world.provider.seen();
  assert.ok(seen.length > 0, "the server saw no request");
  return seen;
}

Then("every request the server saw carried a valid traceparent", async function (this: SdkCallWorld) {
  for (const request of await seenRequests(this)) assert.ok(isTraceparent(request.traceparent), `not a traceparent: ${request.traceparent}`);
});

Then(
  "every request the server saw carried a traceparent in the trace {string} under a span other than {string}",
  async function (this: SdkCallWorld, traceId: string, spanId: string) {
    for (const request of await seenRequests(this)) {
      assert.ok(isTraceparent(request.traceparent), `not a traceparent: ${request.traceparent}`);
      assert.equal(parts(request.traceparent)[1], traceId);
      assert.notEqual(parts(request.traceparent)[2], spanId);
    }
  },
);

Then(
  "every request the server saw stated a call budget above {int} ms and at most {int} ms",
  async function (this: SdkCallWorld, above: number, atMost: number) {
    for (const request of await seenRequests(this)) {
      assert.ok(request.timeoutMs > above && request.timeoutMs <= atMost, `connect-timeout-ms was ${request.timeoutMs}`);
    }
  },
);

Then("the request the server saw stated no call budget", async function (this: SdkCallWorld) {
  const seen = await seenRequests(this);
  assert.equal(seen.length, 1, `the server saw ${seen.length} requests`);
  assert.equal(seen[0]!.timeoutMs, 0, `connect-timeout-ms was ${seen[0]!.timeoutMs}`);
});

Then("the call budget the server saw shrank from each request to the next", async function (this: SdkCallWorld) {
  const seen = await seenRequests(this);
  assert.ok(seen.length > 1, "the call was not retried");
  for (let at = 1; at < seen.length; at += 1) {
    assert.ok(seen[at]!.timeoutMs < seen[at - 1]!.timeoutMs, `the budget did not shrink from ${seen[at - 1]!.timeoutMs} to ${seen[at]!.timeoutMs}`);
  }
});

Then("every request the server saw asked for the page size {int}", async function (this: SdkCallWorld, size: number) {
  const asked = (await seenRequests(this)).map((request) => request.pageSize);
  assert.deepEqual(asked, asked.map(() => size));
  if (this.first !== undefined) assert.equal(this.first.items.length, size);
});

Then("every request the server saw carried the caller's credential", async function (this: SdkCallWorld) {
  for (const request of await seenRequests(this)) assert.equal(request.authorization, this.caller.authorization);
});

Then("the requests the server saw carried the credentials {string} in order", async function (this: SdkCallWorld, expected: string) {
  assert.equal((await seenRequests(this)).map((request) => request.authorization).join(","), expected);
});

Then("no request the server saw carried a credential", async function (this: SdkCallWorld) {
  for (const request of await seenRequests(this)) assert.equal(request.authorization, "", "the request carried a credential");
});

// --- what the tracer recorded ---

const statusName = (span: ReadableSpan): string => ["unset", "ok", "error"][span.status.code] ?? "unknown";
const parentOf = (span: ReadableSpan): string | undefined => (span as unknown as { parentSpanContext?: { spanId: string } }).parentSpanContext?.spanId;

Then("the tracer recorded {int} client spans named {string} under the span of the walk", function (this: SdkCallWorld, count: number, name: string) {
  assert.ok(this.walkSpan, "the walk has a recorded span");
  const clients = spans.started.filter((span) => span.kind === SpanKind.CLIENT && span.name === name);
  assert.equal(clients.length, count, `client spans: ${spans.started.map((span) => span.name)}`);
  for (const span of clients) assert.equal(parentOf(span), this.walkSpan.spanContext().spanId, `the span ${span.name} is not under the walk`);
});

Then("every span the tracer recorded ended", function () {
  assert.ok(spans.started.length > 0, "the tracer recorded no span");
  assert.equal(spans.ended.length, spans.started.length);
});

Then("the span of the walk ended once with the status {string}", function (this: SdkCallWorld, status: string) {
  assert.ok(this.walkSpan, "the walk has a recorded span");
  const walk = this.walkSpan;
  const ends = spans.ended.filter((span) => span.spanContext().spanId === walk.spanContext().spanId);
  assert.equal(ends.length, 1, `spans ended: ${spans.ended.map((span) => span.name)}`);
  assert.equal(statusName(ends[0]!), status);
});

Then("the credential source was asked to refresh {int} time\\(s\\)", function (this: SdkCallWorld, times: number) {
  assert.equal(this.refreshes, times);
});

Then("the revision cache still holds the numbers list", async function (this: SdkCallWorld) {
  const sdk = this.opened ?? (await apiBonesSdk(this.session(), { baseUrl: this.provider.url, defaultPageSize: this.defaultPageSize }));
  this.opened = sdk;
  const first = await probeFirstPage(sdk);
  assert.equal(first.fromCache, true, `the revision cache lost the caller's chain; ${await seenNote(this)}`);
});

Then("the tracer started {int} client span(s) named {string}", function (count: number, name: string) {
  assert.equal(spans.started.length, count, `spans started: ${spans.started.map((span) => span.name)}`);
  for (const span of spans.started) {
    assert.equal(span.kind, SpanKind.CLIENT);
    assert.equal(span.name, name);
  }
});

Then("the tracer ended every span it started with the status {string}", function (status: string) {
  assert.ok(spans.started.length > 0, "the tracer started no span");
  assert.equal(spans.ended.length, spans.started.length, `spans ended: ${spans.ended.map((span) => span.name)}`);
  for (const span of spans.ended) assert.equal(statusName(span), status, `span: ${span.name}`);
});

Then("every request the server saw carried the traceparent of a span the tracer ended", async function (this: SdkCallWorld) {
  const ended = new Set(spans.ended.map((span) => span.spanContext().spanId));
  for (const request of await seenRequests(this)) assert.ok(ended.has(parts(request.traceparent)[2]!), `no ended span is ${request.traceparent}`);
});

// --- the host's view of the revision cache and of unauthenticated refusals ---

Then("the host was told {int} time\\(s) that a server refused the caller as unauthenticated", function (this: SdkCallWorld, times: number) {
  assert.equal(this.told, times);
});

/**
 * The wrapper offers no inspection of the cache: it is empty when the caller
 * that filled it, reading the list again, is no longer answered from it.
 */
Then("the caller's revision cache is empty", async function (this: SdkCallWorld) {
  await this.provider.clearSeen();
  await this.provider.configure({ switchTo: null, unauthenticated: 0 });
  this.changeBeforeSend = undefined;
  assert.ok(this.cacheOwner, "a Given filled the revision cache");
  this.caller.subject = this.cacheOwner.subject;
  this.caller.org = this.cacheOwner.org;
  const sdk = this.opened ?? (await apiBonesSdk(this.session(), { baseUrl: this.provider.url, defaultPageSize: this.defaultPageSize }));
  this.opened = sdk;
  const first = await probeFirstPage(sdk);
  assert.equal(first.fromCache, false, `the cache kept a chain; ${await seenNote(this)}`);
});

/**
 * The last caller that filled the cache, whose walk was refused, reads the list
 * again: its slot is gone when the first page is no longer replayed.
 */
Then("the revision cache no longer holds the numbers list of the caller who was replaying", async function (this: SdkCallWorld) {
  const replaying = this.cacheOwners.at(-1);
  assert.ok(replaying, "a Given filled the revision cache");
  await this.provider.clearSeen();
  await this.provider.configure({ switchTo: null, unauthenticated: 0 });
  this.changeBeforeSend = undefined;
  this.caller.subject = replaying.subject;
  this.caller.org = replaying.org;
  const sdk = this.opened ?? (await apiBonesSdk(this.session(), { baseUrl: this.provider.url, defaultPageSize: this.defaultPageSize }));
  this.opened = sdk;
  const first = await sdk.numbersClient.list().firstPage();
  assert.equal(first.fromCache, false, `the cache kept a chain of the caller whose walk was refused; ${await seenNote(this)}`);
});

/**
 * The first caller that filled the cache reads the list again: the cache is
 * intact when its first page is still replayed from it.
 */
Then("the revision cache still holds the numbers list of the other caller", async function (this: SdkCallWorld) {
  const other = this.cacheOwners[0];
  assert.ok(other, "a Given filled the revision cache");
  this.caller.subject = other.subject;
  this.caller.org = other.org;
  const sdk = this.opened ?? (await apiBonesSdk(this.session(), { baseUrl: this.provider.url, defaultPageSize: this.defaultPageSize }));
  this.opened = sdk;
  const first = await sdk.numbersClient.list().firstPage();
  assert.equal(first.fromCache, true, `the refusal emptied the other caller's cache; ${await seenNote(this)}`);
});

// --- the static exports of the SDK ---

When("the caller clamps {int} between {int} and {int} through the SDK", async function (this: SdkCallWorld, value: number, low: number, high: number) {
  await init();
  const answer = clamp(value, low, high);
  this.staticAnswer = answer;
});

When("the caller reads the largest page size through the SDK", async function (this: SdkCallWorld) {
  await init();
  const largest = maxPageSize();
  this.staticAnswer = largest;
});

Then("the static answer is {int}", function (this: SdkCallWorld, expected: number) {
  assert.equal(this.staticAnswer, expected);
});
