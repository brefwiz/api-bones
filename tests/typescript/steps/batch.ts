// SPDX-License-Identifier: MIT
import assert from "node:assert/strict";
import { generateKeyPairSync, sign, type KeyObject } from "node:crypto";
import { EventEmitter, once } from "node:events";
import { readFileSync, rmSync } from "node:fs";
import { type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { type DescService } from "@bufbuild/protobuf";
import { Code, ConnectError, type CallOptions, type Transport } from "@connectrpc/connect";
import { After, AfterAll, Given, Then, When, World } from "@cucumber/cucumber";
import { startWidgetsProvider, stopWidgetsProvider, stopWidgetPostgres, storedWidgetOwner, type WidgetDatabase } from "../provider/widgets.ts";
import { trace } from "@opentelemetry/api";
import { BasicTracerProvider } from "@opentelemetry/sdk-trace-base";
import { batchedClient, createPlatformClient, errorInfoOf, PageError, serveBatchRequest, type BatchResults, type GeneratedMethodPolicyDocument, type ListCall } from "@brefwiz/api-bones-connect";
import { configureConnectTransport } from "@brefwiz/api-bones-connect/web";
import { compileContract } from "./descriptor_contract.ts";
import { listSpans } from "./list-observability.ts";

AfterAll(stopWidgetPostgres);
let activeState: BatchState | undefined;
const consoleInfo = console.info;
console.info = (...args: unknown[]) => {
  if (typeof args[0] === "string") {
    try {
      const completion = JSON.parse(args[0]);
      if (completion.target === "bones.batch") activeState?.completions.push(completion);
    } catch { /* Non-JSON application logging is outside this recorder. */ }
  }
  consoleInfo(...args);
};
const spanProvider = new BasicTracerProvider({ spanProcessors: [{
  onStart() {},
  onEnd(span) {
    if (span.name === "bones.list") listSpans.push({ name: span.name, attributes: { ...span.attributes } });
    activeState?.records.push({ name: span.name, attributes: { ...span.attributes } });
    if (span.name === "bones.batch" && span.attributes["bones.batch.chunk_index"] === 0 && span.attributes["bones.batch.outcome"] === "ok" && activeState?.swapAfterDelivery) {
      activeState.session = activeState.swapAfterDelivery;
      activeState.swapAfterDelivery = undefined;
    }
  },
  async forceFlush() {},
  async shutdown() {},
}] });
trace.setGlobalTracerProvider(spanProvider);

type Widget = { id: string; name: string; kind: string };
export type Result = BatchResults<Widget>[number];
type Session = { subject: string; org: string; serial: string };
type CredentialFault = "missing bearer" | "missing relay" | "malformed bearer" | "malformed relay";
type Seen = { rpc: string; ids: string[]; trace: string; authorization: string; cursor: string; pageSize?: number; bytes: number; timeout: string | null; cache: string | null; revision: string | null; lane: string };
export type BatchClient = { getWidget(request: { id: string; kind?: string }, options?: CallOptions): Promise<Widget>; batchGetWidgets(ids: readonly string[], template?: object, options?: CallOptions): Promise<BatchResults<Widget>>; batchListWidgetParts(ids: readonly string[], template?: object, options?: CallOptions): Promise<readonly { parentId: string; call?: ListCall<string>; error?: ConnectError }[]> };
export interface BatchState extends WidgetDatabase {
  fixtures: { org: string; id: string; parts: string[]; owner?: string }[]; generated: string[];
  session: Session; permissions: string[]; expires: number; keyPair: { privateKey: KeyObject; publicKey: KeyObject }; requests: Seen[];
  groups: { parentId: string; items: string[]; error?: ConnectError }[];
  groupCalls?: (ListCall<string> | undefined)[];
  events: EventEmitter; delay: number; active: number; peak: number; pageSize: number;
  reverse: boolean; failures: { carrying?: string; code: Code; remaining: number }[];
  authorizationFault?: { id: string; message: string };
  completions: Record<string, unknown>[]; firstTokenCalls: number; records: { name: string; attributes: Record<string, unknown> }[];
  server?: Server; workdir?: string; service?: DescService; policy?: GeneratedMethodPolicyDocument;
  client?: BatchClient;
  raw?: { getWidget(request: object): Promise<Widget>; batchGetWidgets(request: object): Promise<unknown> };
  swapOnRequest?: Session; swapAfterDelivery?: Session;
  release?: () => void;
  identityFailure?: boolean; failIdentityOnRequest?: boolean; metadata?: string; transport?: Transport; fetch?: typeof globalThis.fetch;
  malformed?: "order" | "count"; malformedErrors?: ConnectError[][]; continuationError?: ConnectError;
  continuation?: ListCall<string>;
  includeRelay?: boolean; sourceFault?: CredentialFault; sourceFaultOnRequest?: CredentialFault; batchHeaders?: HeadersInit;
}
export interface BatchWorld extends World {
  batch?: BatchState;
  batchResults: Result[];
  batchGroups?: Awaited<ReturnType<BatchClient["batchListWidgetParts"]>>;
  unbatchedError?: ConnectError;
  wholeBatchError?: ConnectError;
  providerWorkCalls?: number;
  batchConstructionErrors?: unknown[];
  batchMalformedErrors?: ConnectError[][];
  batchPending?: Promise<readonly Result[]>[];
}
export const split = (value: string) => value.split(",").filter(Boolean);
const codes: Record<string, Code> = { invalid_argument: Code.InvalidArgument, not_found: Code.NotFound, permission_denied: Code.PermissionDenied, unavailable: Code.Unavailable, internal: Code.Internal, unauthenticated: Code.Unauthenticated, resource_exhausted: Code.ResourceExhausted };
export function state(world: BatchWorld): BatchState {
  if (!world.batch) world.batch = { fixtures: [], generated: [], session: { subject: "ada", org: "acme", serial: "t1" }, permissions: ["widgets:read"], expires: Math.floor(Date.now() / 1000) + 3600, keyPair: generateKeyPairSync("ed25519"), requests: [], groups: [], events: new EventEmitter(), delay: 0, active: 0, peak: 0, pageSize: 1000, reverse: false, failures: [], firstTokenCalls: 0, records: [], completions: [] };
  activeState = world.batch;
  return world.batch;
}
function session(s: BatchState): Session {
  if (s.identityFailure) throw new Error("session source unavailable");
  return s.session;
}
function token(s: BatchState, current: Session = session(s)): string {
  const body = Buffer.from(JSON.stringify({ sub: current.subject, org: current.org, jti: current.serial, aud: "contract.v1.Widgets", exp: s.expires, permissions: s.permissions })).toString("base64url");
  const header = Buffer.from(JSON.stringify({ alg: "EdDSA", typ: "JWT" })).toString("base64url");
  const payload = `${header}.${body}`;
  return `${payload}.${sign(null, Buffer.from(payload), s.keyPair.privateKey).toString("base64url")}`;
}
export function identity(s: BatchState): string {
  const current = session(s);
  return `${current.subject}/${current.org}`;
}
function credentialSnapshot(s: BatchState) {
  const current = session(s), signed = token(s, current);
  let bearer: string | null = signed;
  let relay = s.includeRelay ? signed : undefined;
  switch (s.sourceFault) {
    case "missing bearer": bearer = null; break;
    case "missing relay": relay = undefined; break;
    case "malformed bearer": bearer += "\r\n"; break;
    case "malformed relay": relay = `${signed}\r\n`; break;
  }
  return { identityKey: `${current.subject}/${current.org}`, token: bearer, relayedCredential: relay };
}
export async function prepareBatch(world: BatchWorld): Promise<BatchState> {
  const s = state(world);
  if (s.transport) return s;
  const compiled = compileContract("bones-batch", readFileSync(new URL("../../rust/proto/contract/v1/widgets.proto", import.meta.url), "utf8"));
  s.workdir = compiled.workdir; s.service = compiled.registry.getService("contract.v1.Widgets")!;
  s.policy = JSON.parse(readFileSync(new URL("../connect-method-policy.json", import.meta.url), "utf8"));
  await startWidgetsProvider(s);
  const transport = configureConnectTransport({ baseUrl: `http://127.0.0.1:${(s.server.address() as AddressInfo).port}`, profile: "service", policy: s.policy, fetch: s.fetch, session: { snapshot: () => credentialSnapshot(s) }, interceptors: [next => req => { if (s.metadata !== undefined) req.header.set("x-trace", s.metadata); return next(req); }], retry: { initialDelayMs: 0, maxDelayMs: 0 } });
  s.transport = transport;
  return s;
}
export const settle = async (promise: Promise<Widget>): Promise<Result> => { try { return { ok: true, value: await promise }; } catch (error) { return { ok: false, error: ConnectError.from(error) }; } };
export const failedGet = (error: unknown): Result => ({ ok: false, error: ConnectError.from(error) });
function preparedClient(s: BatchState): BatchClient {
  return s.client ??= batchedClient(createPlatformClient(s.service!, s.transport!), s.policy!) as unknown as BatchClient;
}
export async function finishGets(pending: readonly Promise<Result>[], last: Result): Promise<Result[]> { return [...await Promise.all(pending), last]; }
Given("the organization {string} has widgets {string}", function (this: BatchWorld, org: string, ids: string) { const s = state(this); s.fixtures.push(...split(ids).map(id => ({ org, id, parts: [] }))); });
Given("the organization {string} has {int} widgets", function (this: BatchWorld, org: string, count: number) { const s = state(this); s.generated = Array.from({ length: count }, (_, i) => `w-${i + 1}`); s.fixtures.push(...s.generated.map(id => ({ org, id, parts: [] }))); });
Given("the organization {string} has {int} widgets with ids of {int} bytes", function (this: BatchWorld, org: string, count: number, bytes: number) { const s = state(this); s.generated = Array.from({ length: count }, (_, i) => `${i}`.padEnd(bytes, "x")); s.fixtures.push(...s.generated.map(id => ({ org, id, parts: [] }))); });
Given("the organization {string} has the widget {string} with parts {string}", function (this: BatchWorld, org: string, id: string, parts: string) { state(this).fixtures.push({ org, id, parts: split(parts) }); });
Given("the caller is {string} of organization {string}", function (this: BatchWorld, subject: string, org: string) { state(this).session = { subject, org, serial: "t1" }; });
Given("the bound session supplies a relayed credential", function (this: BatchWorld) { state(this).includeRelay = true; });
Given("the caller supplies revision cache headers with ordinary metadata", function (this: BatchWorld) { state(this).batchHeaders = { "bones-list-cache": "1", "if-none-match": '"captured-revision"', "x-trace": "captured" }; });
Given("the widget {string} of organization {string} is owned by {string}", function (this: BatchWorld, id: string, org: string, owner: string) { const row = state(this).fixtures.find(row => row.id === id && row.org === org); assert.ok(row); row.owner = owner; });
Given("the server takes {int} ms to answer", function (this: BatchWorld, delay: number) { state(this).delay = delay; });
Given("the part lists page {int} at a time", function (this: BatchWorld, size: number) { state(this).pageSize = size; });
Given("the server fails the batch request carrying {string} with {string}", function (this: BatchWorld, carrying: string, code: string) { state(this).failures.push({ carrying, code: codes[code], remaining: Infinity }); });
Given("the server fails the first batch request with {string}", function (this: BatchWorld, code: string) { state(this).failures.push({ code: codes[code], remaining: 1 }); });
Given("the server answers each batch in reverse order", function (this: BatchWorld) { state(this).reverse = true; });
Given("authorizing widget {string} fails internally with {string}", function (this: BatchWorld, id: string, message: string) { state(this).authorizationFault = { id, message }; });
When("the provider is asked for batch results with {int} ids", async function (this: BatchWorld, count: number) {
  const ids = Array.from({ length: count }, (_, index) => `w-${index + 1}`);
  let calls = 0;
  this.wholeBatchError = undefined;
  try { await serveBatchRequest(ids, () => { calls++; return ids; }); }
  catch (error) { this.wholeBatchError = ConnectError.from(error); }
  this.providerWorkCalls = calls;
});
Then("the provider performs no per-item work", function (this: BatchWorld) { assert.equal(this.providerWorkCalls, 0); });
When("the caller requests a batch for {string} while authorization fails internally", async function (this: BatchWorld, ids: string) {
  const s = await prepareBatch(this);
  s.generated = split(ids);
  const client = batchedClient(createPlatformClient(s.service!, s.transport!), s.policy!) as unknown as BatchClient;
  this.batchResults = [...await client.batchGetWidgets(s.generated)];
});
Then("the whole batch call fails with {string}", function (this: BatchWorld, code: string) { assert.ok(this.wholeBatchError, "whole RPC must fail"); assert.equal(this.wholeBatchError.code, codes[code]); });
Then("the whole batch call fails with {string} and ErrorInfo {string}", function (this: BatchWorld, code: string, info: string) { assert.ok(this.wholeBatchError, "whole RPC must fail"); assert.equal(this.wholeBatchError.code, codes[code]); assert.equal(errorInfoOf(this.wholeBatchError)?.code, info); });
Then("every item reports the same redacted internal error", function (this: BatchWorld) {
  assert.equal(this.batchResults.length, state(this).generated.length);
  const first = failure(this.batchResults, 1);
  for (let index = 0; index < this.batchResults.length; index++) {
    const error = failure(this.batchResults, index + 1);
    assert.equal(error.code, Code.Internal);
    assert.equal(error.rawMessage, "internal server error");
    assert.deepEqual(error, first);
  }
});
Then("no item error contains {string} or the requested ids", function (this: BatchWorld, privateMessage: string) {
  for (let index = 0; index < this.batchResults.length; index++) {
    const error = failure(this.batchResults, index + 1);
    const exposed = `${JSON.stringify(error)} ${error.message}`;
    assert.ok(!exposed.includes(privateMessage));
    for (const id of state(this).generated) assert.ok(!exposed.includes(id), "item id leaked in error");
  }
});
When("the caller batch-gets {string}", async function (this: BatchWorld, ids: string) { const s = await prepareBatch(this); const client = batchedClient(createPlatformClient(s.service!, s.transport!), s.policy!) as unknown as BatchClient; this.batchResults = [...await client.batchGetWidgets(split(ids))]; });
When("the caller batch-gets {string} with a per-call {string} credential override", async function (this: BatchWorld, ids: string, header: string) {
  const s = await prepareBatch(this); assert.ok(header === "authorization" || header === "brefwiz-relayed-credential");
  const client = batchedClient(createPlatformClient(s.service!, s.transport!), s.policy!) as unknown as BatchClient;
  this.batchResults = [...await client.batchGetWidgets(split(ids), {}, { headers: { [header]: header === "authorization" ? `Bearer ${token(s)}` : token(s) } })];
});
When("the caller batch-gets the generated widgets", async function (this: BatchWorld) { const s = await prepareBatch(this); const client = batchedClient(createPlatformClient(s.service!, s.transport!), s.policy!) as unknown as BatchClient; this.batchResults = [...await client.batchGetWidgets(s.generated)]; });
When("the caller batch-gets {string} and one id of {int} bytes", async function (this: BatchWorld, ids: string, bytes: number) { const s = await prepareBatch(this); const client = batchedClient(createPlatformClient(s.service!, s.transport!), s.policy!) as unknown as BatchClient; this.batchResults = [...await client.batchGetWidgets([...split(ids), "x".repeat(bytes)])]; });
When(/^the callers get (?:"([^"]*)"|the generated widgets) at the same time$/, async function (this: BatchWorld, ids?: string | null) {
  const s = await prepareBatch(this); const client = batchedClient(createPlatformClient(s.service!, s.transport!), s.policy!) as unknown as BatchClient;
  const requested = ids == null ? s.generated : split(ids);
  const pending = requested.slice(0, -1).map(id => settle(client.getWidget({ id })));
  let last: Result;
  try { last = { ok: true, value: await client.getWidget({ id: requested.at(-1)! }) }; }
  catch (error) { last = failedGet(error); }
  this.batchResults = await finishGets(pending, last);
});
When("the caller gets {string} with the unbatched client", async function (this: BatchWorld, id: string) { const s = await prepareBatch(this); const raw = createPlatformClient(s.service!, s.transport!) as unknown as NonNullable<BatchState["raw"]>; try { await raw.getWidget({ id }); assert.fail("Get succeeded"); } catch (error) { this.unbatchedError = ConnectError.from(error); } });
Given("the batch response has mismatched {string}", function (this: BatchWorld, malformed: string) {
  assert.ok(malformed === "order" || malformed === "count");
  state(this).malformed = malformed;
});
When("the caller receives a malformed batch {string} response", async function (this: BatchWorld, operation: string) {
  const s = await prepareBatch(this); const client = batchedClient(createPlatformClient(s.service!, s.transport!), s.policy!) as unknown as BatchClient;
  if (operation === "get") this.batchMalformedErrors = [(await client.batchGetWidgets(["w1", "w2", "w3"])).map(result => { if (result.ok) assert.fail("malformed item response must fail"); return result.error; })];
  else { assert.equal(operation, "list"); this.batchMalformedErrors = [(await client.batchListWidgetParts(["w1", "w2", "w3"])).map(group => { assert.ok(group.error); assert.equal(group.call, undefined); return group.error; })]; }
});
When("the SDK constructs a batched client with credentials outside its session binding", async function (this: BatchWorld) {
  const s = await prepareBatch(this);
  const unboundTransport = configureConnectTransport({ baseUrl: `http://127.0.0.1:${(s.server!.address() as AddressInfo).port}`, profile: "service", policy: s.policy!, getToken: () => token(s) });
  this.batchConstructionErrors = [];
  try { batchedClient(createPlatformClient(s.service!, unboundTransport), s.policy!); this.batchConstructionErrors.push(undefined); }
  catch (error) { this.batchConstructionErrors.push(error); }
});
When("the callers get {string} under different tokens", async function (this: BatchWorld, ids: string) {
  const s = await prepareBatch(this); const client = batchedClient(createPlatformClient(s.service!, s.transport!), s.policy!) as unknown as BatchClient;
  const requested = split(ids);
  const pending = requested.slice(0, -1).map((id, i) => { s.session.serial = `t${i + 1}`; return settle(client.getWidget({ id })); });
  s.session.serial = `t${requested.length}`;
  let last: Result;
  try { last = { ok: true, value: await client.getWidget({ id: requested.at(-1)! }) }; }
  catch (error) { last = failedGet(error); }
  this.batchResults = await finishGets(pending, last);
});
When(/^the callers get "([^"]*)" with the (x-trace values|deadlines|kinds) "([^"]*)"(?: ms)?$/, async function (this: BatchWorld, ids: string, variant: string, values: string) {
  const s = await prepareBatch(this); const client = batchedClient(createPlatformClient(s.service!, s.transport!), s.policy!) as unknown as BatchClient;
  const variants = split(values);
  const inputs = split(ids).map((id, i) => ({
    request: variant === "kinds" ? { id, kind: variants[i] } : { id },
    options: variant === "deadlines" ? { timeoutMs: Number(variants[i]) } : variant === "x-trace values" ? { headers: { "x-trace": variants[i] } } : {},
  }));
  const pending = inputs.slice(0, -1).map(input => settle(client.getWidget(input.request, input.options)));
  const input = inputs.at(-1)!;
  let last: Result;
  try { last = { ok: true, value: await client.getWidget(input.request, input.options) }; }
  catch (error) { last = failedGet(error); }
  this.batchResults = await finishGets(pending, last);
});
function rememberGroups(s: BatchState, groups: NonNullable<BatchWorld["batchGroups"]>) {
  s.groups = groups.map(group => ({ parentId: group.parentId, items: [], error: group.error }));
  s.groupCalls = groups.map(group => group.call);
}
async function batchListParts(world: BatchWorld, parents: string) {
  const s = await prepareBatch(world); const client = batchedClient(createPlatformClient(s.service!, s.transport!), s.policy!) as unknown as BatchClient;
  const groups = await client.batchListWidgetParts(split(parents), {}, { headers: s.batchHeaders });
  rememberGroups(s, groups);
  world.batchGroups = groups;
}
When("the caller batch-lists the parts of {string}", async function (this: BatchWorld, parents: string) {
  await batchListParts(this, parents);
});
Given("the caller has batch-listed the parts of {string}", async function (this: BatchWorld, parents: string) {
  await batchListParts(this, parents);
});
async function drainParts(stream: AsyncIterable<string>): Promise<{ items: string[]; error?: ConnectError }> {
  const items: string[] = [];
  try { for await (const item of stream) items.push(item); return { items }; }
  catch (error) { return { items, error: error instanceof PageError ? error.cause : ConnectError.from(error) }; }
}
When("the consumer continues parts group {int}", async function (this: BatchWorld, position: number) {
  const s = this.batch!; const call = s.groupCalls![position - 1]!;
  s.groupCalls![position - 1] = undefined;
  Object.assign(s.groups[position - 1], await drainParts(call.items()));
});
function holdPreparedCalls(s: BatchState) {
  s.release = () => { s.release = undefined; s.events.emit("release"); };
}
async function preparedWireLoad(s: BatchState, count: number) {
  if (s.active >= count) return;
  await new Promise<void>(resolve => {
    const arrived = () => { if (s.active >= count) { s.events.off("request", arrived); resolve(); } };
    s.events.on("request", arrived);
  });
}
Given("{int} explicit batches and {int} coalesced windows of two gets are waiting", async function (this: BatchWorld, batches: number, windows: number) {
  const s = await prepareBatch(this); const client = preparedClient(s);
  holdPreparedCalls(s);
  this.batchPending = Array.from({ length: batches }, () => client.batchGetWidgets(["w1", "w2"]));
  for (let i = 0; i < windows; i++) {
    const options = { headers: { window: String(i) } };
    this.batchPending.push(Promise.all([settle(client.getWidget({ id: "w1" }, options)), settle(client.getWidget({ id: "w2" }, options))]));
    await new Promise<void>(resolve => queueMicrotask(resolve));
  }
  await preparedWireLoad(s, Math.min(batches + windows, 4));
});
When("the final coalesced window {int} gets {string}", async function (this: BatchWorld, window: number, ids: string) {
  const s = this.batch!; const client = preparedClient(s); const requested = split(ids);
  const options = { headers: { window: String(window) } };
  const first = settle(client.getWidget({ id: requested[0] }, options));
  queueMicrotask(() => s.release!());
  let last: Result;
  try { last = { ok: true, value: await client.getWidget({ id: requested[1] }, options) }; }
  catch (error) { last = failedGet(error); }
  this.batchResults = await finishGets([first], last);
});
When("the caller aborts the get of {string} before the flush while {string} waits", async function (this: BatchWorld, aborted: string, kept: string) { const s = await prepareBatch(this); const client = batchedClient(createPlatformClient(s.service!, s.transport!), s.policy!) as unknown as BatchClient; const controller = new AbortController(); s.release = () => { s.release = undefined; s.events.emit("release"); }; const keep = settle(client.getWidget({ id: kept })); const leave = settle(client.getWidget({ id: aborted }, { signal: controller.signal })); controller.abort(); s.release(); const failure = await leave; assert.equal(failure.ok, false); if (!failure.ok) assert.equal(failure.error.code, Code.Canceled); this.batchResults = [await keep]; });
When("the caller aborts the get of {string} after the flush while {string} waits", async function (this: BatchWorld, aborted: string, kept: string) { const s = await prepareBatch(this); const client = batchedClient(createPlatformClient(s.service!, s.transport!), s.policy!) as unknown as BatchClient; const controller = new AbortController(); s.release = () => { s.release = undefined; s.events.emit("release"); }; const keep = settle(client.getWidget({ id: kept })); const leave = settle(client.getWidget({ id: aborted }, { signal: controller.signal })); await once(s.events, "request", { signal: AbortSignal.timeout(5000) }); controller.abort(); s.release(); const failure = await leave; assert.equal(failure.ok, false); if (!failure.ok) assert.equal(failure.error.code, Code.Canceled); this.batchResults = [await keep]; });
When("the caller aborts the batch-get of the generated widgets after its first chunk was sent", async function (this: BatchWorld) { const s = await prepareBatch(this); const client = batchedClient(createPlatformClient(s.service!, s.transport!), s.policy!) as unknown as BatchClient; const controller = new AbortController(); s.release = () => { s.release = undefined; s.events.emit("release"); }; const pending = client.batchGetWidgets(s.generated, {}, { signal: controller.signal }); await once(s.events, "request", { signal: AbortSignal.timeout(5000) }); controller.abort(); s.release(); await assert.rejects(pending, error => error instanceof ConnectError && error.code === Code.Canceled); this.batchResults = []; });
Given("{int} calls of the first token and {int} calls of the second token are batch-getting {string}", async function (this: BatchWorld, first: number, second: number, id: string) {
  const s = await prepareBatch(this); const client = preparedClient(s);
  s.firstTokenCalls = first;
  if (!s.delay) s.delay = 25;
  this.batchPending = [];
  let overflow = 0;
  let overflowReady: (() => void) | undefined;
  const overflowSeen = new Promise<void>(resolve => { overflowReady = resolve; });
  holdPreparedCalls(s);
  s.session.serial = "t1";
  for (let i = 0; i < first; i++) this.batchPending.push(client.batchGetWidgets([id], {}, { headers: { "x-token-lane": "t1" } }).then(results => {
    if (results.some(result => !result.ok && result.error.code === Code.ResourceExhausted) && ++overflow === first - 68) overflowReady!();
    return results;
  }));
  if (first > 68) await overflowSeen;
  else if (!s.requests.length) await once(s.events, "request", { signal: AbortSignal.timeout(5000) });
  s.session.serial = "t2";
  for (let i = 0; i < second; i++) this.batchPending.push(client.batchGetWidgets([id], {}, { headers: { "x-token-lane": "t2" } }));
});
When("the second token makes its final batch-get of {string}", async function (this: BatchWorld, id: string) {
  const s = this.batch!; const client = preparedClient(s);
  queueMicrotask(() => s.release!());
  this.batchResults = [...await client.batchGetWidgets([id], {}, { headers: { "x-token-lane": "t2" } })];
});
Then("all prepared batch calls have finished", async function (this: BatchWorld) {
  const finalResults = this.batchResults;
  const preparedResults = (await Promise.all(this.batchPending!)).flat();
  this.batchResults = [...preparedResults, ...finalResults];
});
const result = (results: readonly Result[], position: number) => { const value = results[position - 1]; assert.ok(value, `missing position ${position}`); return value; };
const widget = (results: readonly Result[], position: number) => { const value = result(results, position); assert.ok(value.ok, value.ok ? "" : value.error.message); return value.value; };
const failure = (results: readonly Result[], position: number) => { const value = result(results, position); assert.equal(value.ok, false); if (value.ok) throw new Error("expected failure"); return value.error; };
const shape = (error: ConnectError) => ({ code: error.code, message: error.rawMessage, info: errorInfoOf(error) });
Then("the coalesced denial of {string} has the single get's code, message and ErrorInfo", async function (this: BatchWorld, id: string) {
  const s = await prepareBatch(this); const raw = createPlatformClient(s.service!, s.transport!) as unknown as NonNullable<BatchState["raw"]>;
  try { await raw.getWidget({ id }); assert.fail("object-level rule must deny the single get"); }
  catch (error) { this.unbatchedError = ConnectError.from(error); }
  assert.deepEqual(shape(failure(this.batchResults, 1)), shape(this.unbatchedError!));
});
Then("the stored owner of widget {string} is {string}", async function (this: BatchWorld, id: string, owner: string) { const s = state(this); assert.equal(await storedWidgetOwner(s, s.session.org, id), owner); });
Then("batch client construction is refused", function (this: BatchWorld) { const errors = this.batchConstructionErrors; assert.equal(errors?.length, 1); for (const error of errors!) assert.ok(error instanceof Error); });
Then("every malformed batch response fails its chunk with {string}", function (this: BatchWorld, expected: string) { const responses = this.batchMalformedErrors; assert.equal(responses?.length, 1); for (const errors of responses!) { assert.equal(errors.length, 3); for (const error of errors) { assert.equal(error.code, Code.Internal); assert.equal(errorInfoOf(error)?.code, expected); } } });
const seen = (world: BatchWorld, rpc?: string) => state(world).requests.filter(request => !rpc || request.rpc === rpc);
Then("the results are the widgets {string}", function (this: BatchWorld, ids: string) { assert.deepEqual(this.batchResults.map((_, i) => widget(this.batchResults, i + 1).id), split(ids)); });
Then("the results are the generated widgets", function (this: BatchWorld) { assert.deepEqual(this.batchResults.map((_, i) => widget(this.batchResults, i + 1).id), state(this).generated); });
Then("there are no results", function (this: BatchWorld) { assert.deepEqual(this.batchResults, []); });
Then("position {int} is the widget {string}", function (this: BatchWorld, position: number, id: string) { assert.equal(widget(this.batchResults, position).id, id); });
Then("position {int} fails with {string}", function (this: BatchWorld, position: number, code: string) { assert.equal(failure(this.batchResults, position).code, codes[code]); });
Then("position {int} carries the ErrorInfo {string}", function (this: BatchWorld, position: number, code: string) { assert.equal(errorInfoOf(failure(this.batchResults, position))?.code, code); });
Then("positions {int} to {int} are the generated widgets", function (this: BatchWorld, from: number, to: number) { for (let i = from; i <= to; i++) assert.equal(widget(this.batchResults, i).id, state(this).generated[i - 1]); });
Then("positions {int} to {int} fail with {string}", function (this: BatchWorld, from: number, to: number, code: string) { for (let i = from; i <= to; i++) assert.equal(failure(this.batchResults, i).code, codes[code]); });
Then("positions {int} to {int} carry the ErrorInfo {string}", function (this: BatchWorld, from: number, to: number, code: string) { for (let i = from; i <= to; i++) assert.equal(errorInfoOf(failure(this.batchResults, i))?.code, code); });
Then("every position fails with {string}", function (this: BatchWorld, code: string) { assert.ok(this.batchResults.length); for (let i = 0; i < this.batchResults.length; i++) assert.equal(failure(this.batchResults, i + 1).code, codes[code]); });
Then("every position carries the ErrorInfo {string}", function (this: BatchWorld, code: string) { assert.ok(this.batchResults.length); for (let i = 0; i < this.batchResults.length; i++) assert.equal(errorInfoOf(failure(this.batchResults, i + 1))?.code, code); });
Then("positions {int} and {int} fail identically", function (this: BatchWorld, a: number, b: number) { assert.deepEqual(shape(failure(this.batchResults, a)), shape(failure(this.batchResults, b))); });
Then("position {int} carries the code, message and ErrorInfo of the unbatched get", function (this: BatchWorld, position: number) { assert.ok(this.unbatchedError); assert.deepEqual(shape(failure(this.batchResults, position)), shape(this.unbatchedError!)); });
Then("the widget at position {int} echoes the kind {string}", function (this: BatchWorld, position: number, kind: string) { assert.equal(widget(this.batchResults, position).kind, kind); });
Then("every result is a widget", function (this: BatchWorld) { assert.ok(this.batchResults.length); this.batchResults.forEach((_, i) => widget(this.batchResults, i + 1)); });
Then("exactly {int} calls of the first token fail with {string}", function (this: BatchWorld, count: number, code: string) { assert.equal(this.batchResults.slice(0, state(this).firstTokenCalls).filter(value => !value.ok && value.error.code === codes[code]).length, count); });
Then("every other call of the first token is the widget {string}", function (this: BatchWorld, id: string) { for (const value of this.batchResults.slice(0, state(this).firstTokenCalls)) if (value.ok) assert.equal(value.value.id, id); });
Then("every call of the second token is the widget {string}", function (this: BatchWorld, id: string) { const s = state(this); assert.ok(this.batchResults.length > s.firstTokenCalls); for (let i = s.firstTokenCalls; i < this.batchResults.length; i++) assert.equal(widget(this.batchResults, i + 1).id, id); });
Then("every call is the widget {string}", function (this: BatchWorld, id: string) { assert.ok(this.batchResults.length); this.batchResults.forEach((_, i) => assert.equal(widget(this.batchResults, i + 1).id, id)); });
Then("group {int} is for {string} and lists the parts {string}", async function (this: BatchWorld, position: number, parentId: string, parts: string) {
  assert.equal(this.batchGroups![position - 1].parentId, parentId);
  const s = state(this); const call = s.groupCalls![position - 1];
  if (call) { s.groupCalls![position - 1] = undefined; Object.assign(s.groups[position - 1], await drainParts(call.items())); }
  assert.deepEqual(s.groups[position - 1], { parentId, items: split(parts), error: undefined });
});
Then("group {int} fails with {string}", function (this: BatchWorld, position: number, code: string) { assert.equal(this.batchGroups![position - 1].error?.code, codes[code]); const group = state(this).groups[position - 1]; assert.equal(group.error?.code, codes[code]); assert.deepEqual(group.items, []); });
Then("groups {int} and {int} fail identically", function (this: BatchWorld, a: number, b: number) { assert.deepEqual(shape(this.batchGroups![a - 1].error!), shape(this.batchGroups![b - 1].error!)); assert.deepEqual(shape(state(this).groups[a - 1].error!), shape(state(this).groups[b - 1].error!)); assert.deepEqual(state(this).groups[a - 1].items, []); assert.deepEqual(state(this).groups[b - 1].items, []); });
Then(/^the server received (\d+) "([^"]+)" requests?$/, function (this: BatchWorld, count: string, rpc: string) { assert.equal(seen(this, rpc).length, Number(count)); });
Then("the server received no request", function (this: BatchWorld) { assert.deepEqual(seen(this), []); });
Then("the batch requests carried the id counts {string}", function (this: BatchWorld, counts: string) { assert.deepEqual(seen(this, "BatchGetWidgets").map(request => request.ids.length), split(counts).map(Number)); });
Then("the batch requests carried the id counts {string} in any order", function (this: BatchWorld, counts: string) { assert.deepEqual(seen(this, "BatchGetWidgets").map(request => request.ids.length).sort((a,b) => a-b), split(counts).map(Number).sort((a,b) => a-b)); });
Then("the requests carried the ids {string}", function (this: BatchWorld, ids: string) { assert.deepEqual(seen(this).flatMap(request => request.ids).sort(), split(ids).sort()); });
Then("the first batch request carried fewer than {int} ids", function (this: BatchWorld, limit: number) { const count = seen(this, "BatchGetWidgets")[0].ids.length; assert.ok(count > 0 && count < limit); });
Then("no request was larger than 1 MiB", function (this: BatchWorld) { assert.ok(seen(this).length); for (const request of seen(this)) assert.ok(request.bytes <= 1048576); });
Then("the server never had more than {int} batch requests in flight", function (this: BatchWorld, limit: number) { assert.ok(state(this).peak > 1); assert.ok(state(this).peak <= limit); });
Then("the requests carried the x-trace values {string}", function (this: BatchWorld, traces: string) { assert.deepEqual(seen(this, "GetWidget").map(request => request.trace).sort(), split(traces).sort()); });
Then("the server received one batch request for each call that was served", function (this: BatchWorld) { assert.equal(seen(this, "BatchGetWidgets").length, this.batchResults.filter(result => result.ok).length); });
Then("the single list requests carried the cursors {string}", function (this: BatchWorld, cursors: string) { assert.deepEqual(seen(this, "ListWidgetParts").map(request => request.cursor), split(cursors)); const captured = new Headers(state(this).batchHeaders); for (const request of seen(this, "BatchListWidgetParts")) { assert.equal(request.cache, captured.get("bones-list-cache")); assert.equal(request.revision, captured.get("if-none-match")); } });
Then("group continuation requests omit revision cache headers and preserve ordinary metadata", function (this: BatchWorld) { const requests = seen(this, "ListWidgetParts"); assert.ok(requests.length); for (const request of requests) { assert.equal(request.cache, null); assert.equal(request.revision, null); assert.equal(request.trace, "captured"); } });
Then("the second token's requests were admitted before the first token's backlog drained", function (this: BatchWorld) { const requests = seen(this, "BatchGetWidgets"); const indices = requests.map((request, i) => ({ i, lane: request.lane })).filter(value => value.lane === "t2"); assert.equal(indices.length, 2); assert.ok(indices[1].i < requests.length - 2); });
Then("one bones.batch record names {string} with {int} items and {int} calls", function (this: BatchWorld, method: string, items: number, calls: number) { const s = state(this); const spans = s.records.filter(record => record.name === "bones.batch");
  assert.equal(this.batchResults.length, items);
  assert.equal(spans.length, 1); assert.equal(s.completions.length, 1);
  assert.equal(spans[0].attributes["rpc.method"], method);
  assert.equal(spans[0].attributes["bones.batch.items"], items);
  assert.equal(spans[0].attributes["bones.batch.calls"], calls);
  assert.equal(spans[0].attributes["bones.batch.requested"], items);
  assert.equal(spans[0].attributes["bones.batch.chunk_index"], 0);
  assert.equal(spans[0].attributes["bones.batch.chunks"], 1);
  assertTelemetry(s, "ok"); });
After(async function (this: BatchWorld) { const s = this.batch; if (!s) return; if (s.server) { const closed = new Promise<void>(resolve => s.server!.close(() => resolve())); s.server.closeAllConnections(); await closed; } await stopWidgetsProvider(s); if (s.workdir) rmSync(s.workdir, { recursive: true, force: true }); });
Given("the metadata producer attaches the x-trace value {string}", function (this: BatchWorld, value: string) { state(this).metadata = value; });
When("the callers get {string} while the metadata producer changes to {string} after enqueue", async function (this: BatchWorld, ids: string, next: string) { const s = await prepareBatch(this); const client = batchedClient(createPlatformClient(s.service!, s.transport!), s.policy!) as unknown as BatchClient; const pending = split(ids).map(id => settle(client.getWidget({ id }))); s.metadata = next; this.batchResults = await Promise.all(pending); });
Then("every request carried the captured x-trace value {string}", function (this: BatchWorld, value: string) { assert.ok(seen(this).length); for (const request of seen(this)) assert.equal(request.trace, value); });
Then("the single requests carried distinct timeout headers", function (this: BatchWorld) { const requests = seen(this, "GetWidget"); assert.equal(requests.length, 2); assert.ok(requests.every(request => Number(request.timeout) > 0)); assert.equal(new Set(requests.map(request => request.timeout)).size, 2); });

function assertTelemetry(s: BatchState, outcomes: string) {
  const spans = s.records.filter(record => record.name === "bones.batch");
  assert.equal(spans.length, s.completions.length, "one unsampled completion per span");
  assert.equal(spans.length, s.requests.filter(request => request.rpc.startsWith("Batch")).length, "one span per wire batch");
  assert.deepEqual(spans.map(span => span.attributes["bones.batch.outcome"]).sort(), split(outcomes).sort());
  const fields = ["rpc.method", "bones.batch.single_method", "bones.batch.requested", "bones.batch.items", "bones.batch.calls", "bones.batch.chunk_index", "bones.batch.chunks", "bones.batch.bytes", "bones.batch.wait_ms", "bones.batch.item_errors", "bones.batch.outcome"];
  assert.deepEqual(spans.map(span => span.attributes["bones.batch.items"]).sort((a, b) => Number(a) - Number(b)), s.requests.filter(request => request.rpc.startsWith("Batch")).map(request => request.ids.length).sort((a, b) => a - b));
  assert.deepEqual(spans.map(span => span.attributes["bones.batch.bytes"]).sort((a, b) => Number(a) - Number(b)), s.requests.filter(request => request.rpc.startsWith("Batch")).map(request => request.bytes).sort((a, b) => a - b));
  for (let i = 0; i < spans.length; i++) {
    const attributes = spans[i].attributes;
    for (const field of fields) { assert.notEqual(attributes[field], undefined, field); assert.equal(s.completions[i][field], attributes[field], field); }
    for (const field of fields.slice(2, 10)) assert.ok(typeof attributes[field] === "number" && Number.isFinite(attributes[field]) && Number(attributes[field]) >= 0, field);
    assert.ok(Number(attributes["bones.batch.bytes"]) > 0);
    assert.equal(attributes["bones.batch.item_errors"], 0);
    assert.ok(Number(attributes["bones.batch.requested"]) >= Number(attributes["bones.batch.items"]));
    assert.ok(Number(attributes["bones.batch.calls"]) >= 1);
    assert.ok(Number(attributes["bones.batch.chunk_index"]) < Number(attributes["bones.batch.chunks"]));
    const request = s.requests.filter(request => request.rpc.startsWith("Batch"))[i];
    assert.equal(attributes["rpc.method"], `contract.v1.Widgets/${request.rpc}`);
    assert.equal(attributes["bones.batch.single_method"], `contract.v1.Widgets/${request.rpc === "BatchListWidgetParts" ? "ListWidgetParts" : "GetWidget"}`);
  }
}
Then("batch telemetry has exactly the wire outcomes {string}", function (this: BatchWorld, outcomes: string) { assertTelemetry(state(this), outcomes); });
Then("no batch span or completion is emitted", function (this: BatchWorld) { const s = state(this); assert.equal(s.records.filter(record => record.name === "bones.batch").length, 0); assert.equal(s.completions.length, 0); });
