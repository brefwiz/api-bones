// SPDX-License-Identifier: MIT
import assert from "node:assert/strict";
import { once } from "node:events";
import { setFlagsFromString } from "node:v8";
import { runInNewContext } from "node:vm";
import { setImmediate as nextTurn } from "node:timers/promises";
import { Code, ConnectError } from "@connectrpc/connect";
import { create, fromBinary, fromJson, toBinary, toJson } from "@bufbuild/protobuf";
import { TotalSchema, type TotalMessage as Total } from "@brefwiz/api-bones-connect";
import { After, Given, Then, When } from "@cucumber/cucumber";
import { createPlatformClient, encodeErrorInfo, isCursorRejected, listCall, pagedClient, PageError, RevisionCache, type Page, type ListCall, type ListConfig, type PageFetch } from "@brefwiz/api-bones-connect";
import { storedWidgetOwner, storedWidgetParts, lockWidgetReads, unlockWidgetReads, waitForBlockedWidgetRead, startWidgetsListener, stopWidgetsListener } from "../provider/widgets.ts";
import { prepareBatch, state, split, type BatchState, type BatchWorld } from "./batch.ts";
import { CONNECT_CODE_NAMES } from "./support.ts";
import { listSpans } from "./list-observability.ts";

const PARENT = "paginated-widget";
Given("the server reports a total of kind {int}", function (this: PagingWorld, number: number) {
  this.reportedTotal = create(TotalSchema, { value: 42n, kind: number });
});
When("the caller fetches the first page with a list call", async function (this: PagingWorld) {
  assert.ok(this.reportedTotal, "reported total");
  const wire = toBinary(TotalSchema, this.reportedTotal);
  const decoded = fromBinary(TotalSchema, wire);
  const call = listCall<string>(async () => ({ items: ["1"], page: { nextCursor: "", revision: "total-revision", total: decoded } }), {}, {
    method: "/contract.v1.Numbers/ListNoSideEffects",
    paging: { kind: "unary", itemField: "items", pageField: "page", requestPageField: "page" },
  });
  this.pagingFirst = await call.firstPage();
});
Then("the page total kind is {string}", function (this: PagingWorld, kind: string) {
  assert.ok(this.pagingFirst, "SDK page with total");
  assert.deepEqual(this.pagingFirst.total, { value: 42n, kind });
});
Then("the total kind travels as {int} on the wire and {string} in JSON", function (this: PagingWorld, number: number, name: string) {
  assert.ok(this.reportedTotal, "reported total");
  const total = this.reportedTotal;
  const json = toJson(TotalSchema, total);
  assert.deepEqual(json, { value: "42", kind: name });
  assert.deepEqual(fromJson(TotalSchema, json), total);
  const wire = toBinary(TotalSchema, total);
  assert.deepEqual(wire, Uint8Array.of(8, 42, 16, number));
  assert.deepEqual(fromBinary(TotalSchema, wire), total);
});
const pagingPolicy = { kind: "unary" as const, itemField: "items", pageField: "page", requestPageField: "page" };
type PartList = (request: object, options?: object) => ListCall<string>;
type PartsResponse = { items: string[]; page?: { nextCursor: string; revision: string } };
interface PagingWorld extends BatchWorld {
  reportedTotal?: Total;
  pagingItems: string[];
  pagingPages: Page<string>[];
  pagingError?: PageError;
  pagingRefusal?: ConnectError;
  pagingFirst?: Page<string>;
  paging: {
    cache: RevisionCache; held: Page<string>[]; revisions: Map<string, string>;
    seed?: { first: Page<string>; nextCursor: string };
    replay?: AsyncIterator<Page<string>>;
    continuation?: AsyncIterator<Page<string>>;
    listStart: number; revalidationStart: number; aborted: boolean; blocked: boolean; lockAfterFirst?: boolean;
    method: "listWidgetParts" | "listWidgetPartsIdempotent" | "listWidgetPartsUnspecified";
    stopAfterFirst?: boolean; outageActive: boolean; restartAfter: number; failedAttempts: number;
    completions: Record<string, unknown>[]; spanStart: number;
  };
}
let activePaging: PagingWorld["paging"] | undefined;
const previousInfo = console.info;
console.info = (...args: unknown[]) => {
  if (typeof args[0] === "string") {
    try {
      const completion = JSON.parse(args[0]);
      if (completion.target === "bones.list") activePaging?.completions.push(completion);
    } catch { /* Non-JSON application output. */ }
  }
  previousInfo(...args);
};
const identity = (s: BatchState) => `${s.session.subject}/${s.session.org}`;
const requests = (world: PagingWorld) => state(world).requests.filter(request => request.rpc.startsWith("ListWidgetParts")).slice(world.paging.listStart);
const methodName = (world: PagingWorld) => world.paging.method;
const methodRpc = (world: PagingWorld) => `/contract.v1.Widgets/${world.paging.method[0].toUpperCase()}${world.paging.method.slice(1)}`;
function remembered(world: PagingWorld, label: string): string {
  const revision = world.paging.revisions.get(label);
  assert.ok(revision, `stored revision ${label} was captured`);
  return revision;
}
function changeIdentity(world: PagingWorld, value: string) {
  const [subject, org] = value.split(/[:/]/);
  assert.ok(subject && org);
  state(world).session = { subject, org, serial: "changed" };
  state(world).permissions = ["widgets:read"];
}
Given(/^a list of pages (.+)$/, function (this: PagingWorld, text: string) {
  const pages = [...text.matchAll(/"([^"]*)"/g)].map(match => split(match[1]));
  assert.ok(pages.length && pages[0].length);
  const s = state(this);
  s.permissions = ["widgets:read", "widgets:write"];
  s.pageSize = pages[0].length;
  for (const org of [s.session.org, "org-b"]) s.fixtures.push({ org, id: PARENT, parts: pages.flat() });
  this.pagingItems = []; this.pagingPages = [];
  this.paging = { cache: new RevisionCache(() => identity(s)), held: [], revisions: new Map(), listStart: 0, revalidationStart: 0, aborted: false, blocked: false, method: "listWidgetParts", outageActive: false, restartAfter: 0, failedAttempts: 0, completions: [], spanStart: listSpans.length };
  s.fetch = async (...args) => {
    try { return await globalThis.fetch(...args); }
    catch (error) {
      if (this.paging.outageActive) {
        this.paging.failedAttempts++;
        if (this.paging.failedAttempts === this.paging.restartAfter) { await startWidgetsListener(s); this.paging.outageActive = false; }
      }
      throw error;
    }
  };
  s.events.on("aborted", () => { this.paging.aborted = true; });
  activePaging = this.paging;
});
async function pagingArguments(world: PagingWorld): Promise<Parameters<typeof pagedClient>> {
  const s = await prepareBatch(world);
  return [createPlatformClient(s.service!, s.transport!), s.policy!, world.paging.cache];
}
async function preparePageFetch(world: PagingWorld): Promise<{ fetch: PageFetch; config: ListConfig<string> }> {
  const s = await prepareBatch(world);
  const raw = createPlatformClient(s.service!, s.transport!);
  const method = raw[methodName(world)] as unknown as PageFetch;
  const fetch: PageFetch = async (request, options) => {
    const response = await method(request, options);
    if (world.paging.lockAfterFirst && !(request.page as { cursor?: string } | undefined)?.cursor) {
      world.paging.lockAfterFirst = false;
      await lockWidgetReads(s); world.paging.blocked = true;
    }
    if (world.paging.stopAfterFirst && !(request.page as { cursor?: string } | undefined)?.cursor) {
      world.paging.stopAfterFirst = false;
      await stopWidgetsListener(s); world.paging.outageActive = true;
    }
    return response;
  };
  return { fetch, config: { method: methodRpc(world), paging: pagingPolicy, cache: world.paging.cache } };
}
async function collectIteration<T>(iteration: AsyncIterable<T>) {
  const values: T[] = [];
  let failure: PageError | undefined;
  try { for await (const value of iteration) values.push(value); }
  catch (error) { assert.ok(error instanceof PageError); failure = error; }
  return { values, error: failure };
}
async function rememberWrite(world: PagingWorld, response: PartsResponse, label: string) {
  const s = state(world); const stored = await storedWidgetParts(s, s.session.org, PARENT);
  assert.deepEqual(response.items, stored.items); assert.equal(response.page?.revision, stored.revision);
  world.paging.revisions.set(label, stored.revision);
  return stored;
}
Given("the current stored list revision is remembered as {string}", async function (this: PagingWorld, label: string) {
  const s = await prepareBatch(this);
  this.paging.revisions.set(label, (await storedWidgetParts(s, s.session.org, PARENT)).revision);
});
Given("the owner replaces stored widget parts with {string} and remembers revision as {string}", async function (this: PagingWorld, items: string, label: string) {
  const s = await prepareBatch(this); const raw = createPlatformClient(s.service!, s.transport!);
  const response = await raw.updateWidgetParts({ widgetId: PARENT, items: split(items) }) as unknown as PartsResponse;
  await rememberWrite(this, response, label);
});
Given("the generated list client uses a revision cache", async function (this: PagingWorld) { await prepareBatch(this); });
Given("the RPC is declared {string}", function (this: PagingWorld, contract: string) {
  assert.ok(["NO_SIDE_EFFECTS", "IDEMPOTENT", "NON_IDEMPOTENT", "UNSPECIFIED"].includes(contract));
  this.paging.method = contract === "NO_SIDE_EFFECTS" ? "listWidgetParts" : contract === "IDEMPOTENT" ? "listWidgetPartsIdempotent" : "listWidgetPartsUnspecified";
});
Given("the widgets provider stops before page {int} and restarts after {int} failed attempts", async function (this: PagingWorld, page: number, attempts: number) {
  assert.ok(page === 1 || page === 2); assert.ok(attempts > 0);
  const s = await prepareBatch(this); this.paging.restartAfter = attempts;
  if (page === 1) { await stopWidgetsListener(s); this.paging.outageActive = true; }
  else this.paging.stopAfterFirst = true;
});
Given("the consumer retains every generated list page", async function (this: PagingWorld) {
  const client = pagedClient(...await pagingArguments(this)); const call = client[methodName(this)] as unknown as PartList;
  const result = await collectIteration(call({ widgetId: PARENT }, { prefetch: 0 }).pages());
  assert.equal(result.error, undefined); this.paging.held = result.values;
});
Given("the consumer has delivered {int} stored list pages", async function (this: PagingWorld, count: number) {
  const client = pagedClient(...await pagingArguments(this)); const call = client[methodName(this)] as unknown as PartList;
  this.paging.continuation = call({ widgetId: PARENT }, { prefetch: 0 }).pages()[Symbol.asyncIterator]();
  this.pagingPages = []; this.pagingItems = [];
  for (let page = 0; page < count; page++) {
    const next = await this.paging.continuation.next(); assert.equal(next.done, false);
    this.pagingPages.push(next.value!); this.pagingItems.push(...next.value!.items);
  }
});
Given("a Connect refusal carries cursor code {string}", async function (this: PagingWorld, code: string) {
  this.pagingRefusal = new ConnectError("cursor refused", Code.InvalidArgument, undefined, [{ type: "bones.v1.ErrorInfo", value: encodeErrorInfo({ code, emitter: "widgets" }) }]);
  await this.paging.continuation?.return?.();
  this.paging.continuation = undefined;
});
Given("the owner transfers the stored widget to {string}", async function (this: PagingWorld, owner: string) {
  const s = await prepareBatch(this); const stored = await storedWidgetParts(s, s.session.org, PARENT);
  const raw = createPlatformClient(s.service!, s.transport!);
  const response = await raw.updateWidgetParts({ widgetId: PARENT, items: stored.items, owner }) as unknown as PartsResponse;
  await rememberWrite(this, response, "owner-change");
  assert.equal(await storedWidgetOwner(s, s.session.org, PARENT), owner);
});
Given("the consumer retains every generated list page across a write changing revision to {string}", async function (this: PagingWorld, label: string) {
  const s = await prepareBatch(this); const client = pagedClient(...await pagingArguments(this));
  const call = client[methodName(this)] as unknown as PartList;
  const pages = call({ widgetId: PARENT }, { prefetch: 0 }).pages()[Symbol.asyncIterator]();
  const first = await pages.next(); assert.equal(first.done, false); this.paging.held = [first.value!];
  const before = await storedWidgetParts(s, s.session.org, PARENT);
  const raw = createPlatformClient(s.service!, s.transport!);
  const response = await raw.updateWidgetParts({ widgetId: PARENT, items: [...before.items, String(Number(before.items.at(-1)) + 1)] }) as unknown as PartsResponse;
  const stored = await rememberWrite(this, response, label); assert.notEqual(first.value!.revision, stored.revision);
  for (;;) { const next = await pages.next(); if (next.done) break; assert.equal(next.value.revision, stored.revision); this.paging.held.push(next.value); }
  assert.ok(this.paging.held.length > 1);
});
function holdSeed(world: PagingWorld, response: PartsResponse) {
  assert.ok(response.page);
  world.paging.seed = { first: { items: response.items, revision: response.page.revision, fromCache: false }, nextCursor: response.page.nextCursor };
}
Given("the caller holds the first stored list page", async function (this: PagingWorld) {
  const s = await prepareBatch(this); const raw = createPlatformClient(s.service!, s.transport!);
  holdSeed(this, await raw.listWidgetParts({ widgetId: PARENT }) as unknown as PartsResponse);
  this.paging.listStart = s.requests.filter(request => request.rpc === "ListWidgetParts").length;
});
Given("the caller holds the first grouped widget list page", async function (this: PagingWorld) {
  const s = await prepareBatch(this); const raw = createPlatformClient(s.service!, s.transport!);
  const response = await raw.batchListWidgetParts({ widgetIds: [PARENT] }) as unknown as { groups: PartsResponse[] };
  assert.equal(response.groups.length, 1); holdSeed(this, response.groups[0]);
});
Given("the consumer releases retained list pages", async function (this: PagingWorld) {
  this.paging.held = []; this.pagingPages = []; this.pagingFirst = undefined;
  setFlagsFromString("--expose-gc"); const collect = runInNewContext("gc") as () => void;
  await nextTurn(); collect(); await nextTurn();
});
Given("the caller identity changes to {string}", function (this: PagingWorld, value: string) { changeIdentity(this, value); });
Given("the revision cache is cleared", function (this: PagingWorld) { this.paging.cache.clear(); });
Given("the producer credential is no longer valid", function (this: PagingWorld) { state(this).expires = 0; });
Given("the stored widget list is locked by another transaction", async function (this: PagingWorld) { await lockWidgetReads(await prepareBatch(this)); this.paging.blocked = true; });
Given("the stored list read for page {int} waits on another transaction", async function (this: PagingWorld, page: number) {
  assert.ok(page === 1 || page === 2);
  if (page === 1) { await lockWidgetReads(await prepareBatch(this)); this.paging.blocked = true; }
  else this.paging.lockAfterFirst = true;
});
Given(/^the consumer has (listed bypassing the revision cache|continued a batch-seeded generated list)$/, async function (this: PagingWorld, mode: string) {
  const prepared = await preparePageFetch(this), seed = mode.startsWith("continued") ? this.paging.seed : undefined;
  if (mode.startsWith("continued")) assert.ok(seed);
  const start = requests(this).length;
  const config = { ...prepared.config, ...(seed ? { initialPage: seed.first, nextCursor: seed.nextCursor } : {}) };
  const result = await collectIteration(listCall<string>(prepared.fetch, { widgetId: PARENT }, config, { prefetch: 0, cache: "bypass" }).pages());
  assert.equal(result.error, undefined); assert.ok(result.values.length); assert.ok(result.values.every(page => !page.fromCache));
  const sent = requests(this).slice(start); assert.ok(sent.length); assert.ok(sent.every(request => request.revision === null));
  if (seed) assert.equal(sent[0].cursor, seed.nextCursor);
  this.pagingPages = result.values; this.pagingItems = []; this.pagingError = undefined;
});

When(/^the consumer reads every item(?: with page size (\d+))?$/, async function (this: PagingWorld, pageSize?: string) {
  const prepared = await preparePageFetch(this);
  const result = await collectIteration(listCall<string>(prepared.fetch, { widgetId: PARENT }, prepared.config, pageSize === undefined ? {} : { pageSize: Number(pageSize) }).items());
  this.pagingItems = result.values; this.pagingPages = []; this.pagingError = result.error;
});
When("the consumer reads every page", async function (this: PagingWorld) {
  const prepared = await preparePageFetch(this);
  const result = await collectIteration(listCall<string>(prepared.fetch, { widgetId: PARENT }, prepared.config).pages());
  this.pagingPages = result.values; this.pagingItems = []; this.pagingError = result.error;
});
When("the consumer resumes the stored list", async function (this: PagingWorld) {
  const iterator = this.paging.continuation; assert.ok(iterator);
  try {
    const next = await iterator.next();
    if (!next.done) { this.pagingPages.push(next.value); this.pagingItems.push(...next.value.items); }
  } catch (error) { assert.ok(error instanceof PageError); this.pagingError = error; }
});
When("the consumer classifies the cursor refusal as a page error", function (this: PagingWorld) {
  assert.ok(this.pagingRefusal);
  this.pagingError = new PageError(this.pagingRefusal, this.pagingPages.length, this.pagingItems.length);
});
When(/^the consumer lists (again|bypassing the revision cache)$/, async function (this: PagingWorld, mode: string) {
  this.paging.revalidationStart = requests(this).length;
  const prepared = await preparePageFetch(this);
  const result = await collectIteration(listCall<string>(prepared.fetch, { widgetId: PARENT }, prepared.config, { prefetch: 0, cache: mode === "again" ? "use" : "bypass" }).pages());
  this.pagingPages = result.values; this.pagingItems = []; this.pagingError = result.error;
});
When(/^the consumer reads one page(?: with prefetch (off))? and stops$/, async function (this: PagingWorld, prefetch?: string) {
  const prepared = await preparePageFetch(this); const seed = this.paging.seed;
  const iterator = listCall<string>(prepared.fetch, { widgetId: PARENT }, { ...prepared.config, ...(seed ? { initialPage: seed.first, nextCursor: seed.nextCursor } : {}) }, prefetch === "off" ? { prefetch: 0 } : {}).pages()[Symbol.asyncIterator]();
  const first = await iterator.next(); assert.equal(first.done, false);
  this.pagingFirst = first.value; this.pagingPages = [first.value!]; this.pagingItems = []; this.pagingError = undefined;
  if (this.paging.blocked) await waitForBlockedWidgetRead(state(this));
  await iterator.return?.();
});
When("the consumer reads the first page only", async function (this: PagingWorld) {
  const prepared = await preparePageFetch(this); this.pagingItems = []; this.pagingPages = []; this.pagingError = undefined;
  const call = listCall<string>(prepared.fetch, { widgetId: PARENT }, prepared.config);
  try { const page = await call.firstPage(); this.pagingFirst = page; this.pagingPages = [page]; }
  catch (error) { assert.ok(error instanceof PageError); this.pagingError = error; }
});
When(/^the consumer continues (?:from the held first page|a batch-seeded generated list)$/, async function (this: PagingWorld) {
  const seed = this.paging.seed; assert.ok(seed); this.paging.revalidationStart = requests(this).length;
  const prepared = await preparePageFetch(this);
  const result = await collectIteration(listCall<string>(prepared.fetch, { widgetId: PARENT }, { ...prepared.config, initialPage: seed.first, nextCursor: seed.nextCursor }, { prefetch: 0 }).pages());
  this.pagingPages = result.values; this.pagingError = result.error;
});
When("the caller makes a list call with a matching revision without the list cache opt-in", async function (this: PagingWorld) {
  const s = await prepareBatch(this); const raw = createPlatformClient(s.service!, s.transport!);
  const response = await raw.listWidgetParts({ widgetId: PARENT }, { headers: { "if-none-match": `"${remembered(this, "r1")}"` } }) as unknown as PartsResponse;
  this.pagingItems = response.items; assert.equal(requests(this)[0]?.cache, null);
});
Given("the consumer has paused a cached replay", async function (this: PagingWorld) {
  const client = pagedClient(...await pagingArguments(this)); const call = client[methodName(this)] as unknown as PartList;
  const iterator = call({ widgetId: PARENT }, { prefetch: 0 }).pages()[Symbol.asyncIterator]();
  const first = await iterator.next(); assert.equal(first.value?.fromCache, true);
  this.pagingFirst = first.value; this.paging.replay = iterator;
});
When(/^the consumer (?:changes identity to "([^"]*)"|clears the revision cache) and resumes the cached replay$/, async function (this: PagingWorld, value?: string | null) {
  const iterator = this.paging.replay; assert.ok(iterator);
  if (value == null) this.paging.cache.clear(); else changeIdentity(this, value);
  try { await iterator.next(); assert.fail("identity guard must discard replay remainder"); }
  catch (error) { assert.ok(error instanceof PageError); this.pagingError = error; }
});
async function finishInFlight(world: PagingWorld, iterator: AsyncIterator<Page<string>>, page: number, mutate: () => void) {
  const delivered: Page<string>[] = [];
  if (page === 2) { const first = await iterator.next(); assert.equal(first.done, false); delivered.push(first.value!); }
  const pending = iterator.next().then(value => ({ value }), (error: unknown) => ({ error }));
  await waitForBlockedWidgetRead(state(world));
  mutate(); await unlockWidgetReads(state(world)); world.paging.blocked = false;
  const result = await pending;
  assert.ok("error" in result, "in-flight response must be discarded");
  assert.ok(result.error instanceof PageError);
  assert.equal((await iterator.next()).done, true);
  return { pages: delivered, error: result.error };
}
When(/^the consumer (?:changes identity to "([^"]*)"|clears the revision cache) while page (\d+) is in flight with prefetch (\d+)$/, async function (this: PagingWorld, value: string | null, pageText: string, prefetchText: string) {
  const page = Number(pageText), prefetch = Number(prefetchText);
  assert.ok(prefetch === 0 || prefetch === 1);
  const prepared = await preparePageFetch(this);
  const iterator = listCall<string>(prepared.fetch, { widgetId: PARENT }, prepared.config, { prefetch: prefetch as 0 | 1 }).pages()[Symbol.asyncIterator]();
  const result = await finishInFlight(this, iterator, page, () => value == null ? this.paging.cache.clear() : changeIdentity(this, value));
  this.pagingPages = result.pages; this.pagingItems = []; this.pagingError = result.error;
});

Then("the consumer receives items {string}", function (this: PagingWorld, expected: string) { assert.deepEqual(this.pagingItems, split(expected)); });
Then("the consumer receives pages {string}", function (this: PagingWorld, expected: string) { assert.deepEqual(this.pagingPages.map(page => [...page.items]), expected.split("|").map(split)); });
Then(/^every (?:generated list )?page carries (?:the )?revision "([^"]*)"$/, function (this: PagingWorld, label: string) { assert.ok(this.pagingPages.length); for (const page of this.pagingPages) assert.equal(page.revision, remembered(this, label)); });
Then("every generated list page is marked from cache {word}", async function (this: PagingWorld, expected: string) {
  assert.ok(this.pagingPages.length); assert.ok(this.pagingPages.every(page => page.fromCache === (expected === "true")));
  const s = state(this); const stored = await storedWidgetParts(s, s.session.org, PARENT);
  assert.deepEqual(this.pagingPages.flatMap(page => [...page.items]), stored.items);
});
Then("the stored widget parts are {string}", async function (this: PagingWorld, expected: string) {
  const s = state(this); const stored = await storedWidgetParts(s, s.session.org, PARENT);
  assert.deepEqual(stored.items, split(expected)); assert.deepEqual(this.pagingPages.flatMap(page => [...page.items]), stored.items);
  assert.ok(this.pagingPages.every(page => page.revision === stored.revision));
});
Then("the stored widget owner is {string}", async function (this: PagingWorld, owner: string) { const s = state(this); assert.equal(await storedWidgetOwner(s, s.session.org, PARENT), owner); });
Then("the first revalidation carried if-none-match {string}", function (this: PagingWorld, expected: string) {
  const seen = requests(this).slice(this.paging.revalidationStart);
  const header = expected ? `"${remembered(this, expected.replace(/^"|"$/g, ""))}"` : "";
  assert.equal(seen[0]?.revision ?? "", header); assert.ok(seen.every(request => request.cache === "1"));
  assert.ok(seen.slice(1).every(request => request.revision === null));
});
Then("the batch continuation sent no conditional revision", function (this: PagingWorld) {
  const seen = requests(this).slice(this.paging.revalidationStart); assert.ok(seen.length); assert.ok(seen.every(request => request.revision === null));
  assert.equal(seen[0].cursor, this.paging.seed?.nextCursor);
});
Then("the revision cache is empty", function (this: PagingWorld) { assert.ok(this.paging.cache.isEmpty()); });
Then("the paused replay ends with an unauthenticated error", async function (this: PagingWorld) { assert.equal(this.pagingError?.cause.code, Code.Unauthenticated); if (this.paging.replay) assert.equal((await this.paging.replay.next()).done, true); });
Then("the stream ended without an error", function (this: PagingWorld) { assert.equal(this.pagingError, undefined); });
Then("the server saw the cursors {string}", function (this: PagingWorld, expected: string) { assert.equal(requests(this).map(request => request.cursor).join(","), expected); });
Then(/^the server saw (\d+) requests?$/, function (this: PagingWorld, count: string) { assert.equal(requests(this).length, Number(count)); });
Then("the server saw at most {int} requests", function (this: PagingWorld, count: number) { assert.ok(requests(this).length <= count); assert.ok(this.pagingPages.length); });
Then("the server saw the page size {int} on every request", async function (this: PagingWorld, size: number) {
  const items = this.pagingItems;
  const s = state(this), stored = await storedWidgetParts(s, s.session.org, PARENT);
  assert.deepEqual(items, stored.items);
  assert.ok(requests(this).length); for (const request of requests(this)) assert.equal(request.pageSize, size);
});
Then("the server saw no page size on every request", async function (this: PagingWorld) {
  const items = this.pagingItems;
  const s = state(this), stored = await storedWidgetParts(s, s.session.org, PARENT);
  assert.deepEqual(items, stored.items);
  assert.ok(requests(this).length); for (const request of requests(this)) assert.equal(request.pageSize, undefined);
});
Then("the server saw {int} requests for the cursor {string}", function (this: PagingWorld, count: number, cursor: string) { assert.equal(requests(this).filter(request => request.cursor === cursor).length, count); });
Then("the SDK made {int} failed page attempts", function (this: PagingWorld, count: number) { assert.equal(this.paging.failedAttempts, count); });
Then("the fetch for page {int} was aborted", async function (this: PagingWorld, _page: number) {
  assert.ok(this.pagingFirst); assert.equal(this.pagingPages.length, 1);
  if (!this.paging.aborted) await once(state(this).events, "aborted", { signal: AbortSignal.timeout(3000) });
  assert.equal(this.paging.aborted, true); await unlockWidgetReads(state(this)); this.paging.blocked = false;
  const s = state(this), stored = await storedWidgetParts(s, s.session.org, PARENT);
  assert.deepEqual(this.pagingFirst.items, stored.items.slice(0, s.pageSize)); assert.equal(this.pagingFirst.revision, stored.revision);
});
Then(/^the (?:stream ended with an error after|classified refusal preserves delivery context of) (\d+) pages and (\d+) items$/, async function (this: PagingWorld, pages: string, items: string) { assert.ok(this.pagingError instanceof PageError); assert.equal(this.pagingError.deliveredPages, Number(pages)); assert.equal(this.pagingError.deliveredItems, Number(items)); if (this.paging.continuation) assert.equal((await this.paging.continuation.next()).done, true); });
Then("the error is an RPC failure with code {string}", function (this: PagingWorld, code: string) { assert.equal(this.pagingError?.kind, "rpc"); assert.equal(CONNECT_CODE_NAMES.get(this.pagingError!.cause.code), code); });
Then("the error is not a rejected cursor", function (this: PagingWorld) { assert.equal(isCursorRejected(this.pagingError), false); });
Then("the cursor refusal is classified as kind {string}", function (this: PagingWorld, kind: string) { assert.equal(this.pagingError?.kind, kind); assert.equal(isCursorRejected(this.pagingError), true); });
function completionRecords(world: PagingWorld) {
  const completions = world.paging.completions; const spans = listSpans.slice(world.paging.spanStart);
  assert.equal(completions.length, spans.length); return { completions, spans };
}
Then("the list completion records {int} pages and {int} not-modified hits", function (this: PagingWorld, pages: number, hits: number) {
  const { completions, spans } = completionRecords(this); assert.equal(completions.length, 2);
  for (const attributes of [completions.at(-1)!, spans.at(-1)!.attributes]) {
    assert.equal(attributes["bones.list.pages"], pages); assert.equal(attributes["bones.list.not_modified_hits"], hits);
    assert.equal(attributes["bones.list.prefetch_hits"], 0); assert.equal(attributes["bones.list.cancelled"], false);
    assert.equal(typeof attributes["bones.list.time_to_first_item_ms"], "number"); assert.equal(typeof attributes["bones.list.time_to_full_list_ms"], "number");
    assert.equal(attributes["rpc.method"], methodRpc(this));
  }
  assert.equal(spans.at(-1)!.name, "bones.list");
  assert.equal(completions[0]["bones.list.not_modified_hits"], 0);
  assert.equal(spans[0].attributes["bones.list.not_modified_hits"], 0);
});
Then("the list completion records cancelled {word} without full-list time", function (this: PagingWorld, cancelled: string) {
  const { completions, spans } = completionRecords(this); assert.equal(completions.length, 1);
  for (const attributes of [completions[0], spans[0].attributes]) {
    assert.equal(attributes["bones.list.cancelled"], cancelled === "true"); assert.equal(typeof attributes["bones.list.time_to_first_item_ms"], "number"); assert.equal(attributes["bones.list.time_to_full_list_ms"], undefined);
  }
});
After(async function (this: PagingWorld) {
  if (!this.paging) return;
  activePaging = undefined;
  if (this.paging.blocked) await unlockWidgetReads(state(this));
  await this.paging.replay?.return?.();
  await this.paging.continuation?.return?.();
});
