// SPDX-License-Identifier: MIT
import assert from "node:assert/strict";
import { once } from "node:events";
import { Code, ConnectError } from "@connectrpc/connect";
import { PageError, errorInfoOf, batchedClient, createPlatformClient } from "@brefwiz/api-bones-connect";
import { Given, Then, When } from "@cucumber/cucumber";
import { prepareBatch, settle, split, state, failedGet, finishGets, type Result, type BatchWorld, type BatchState, type BatchClient } from "./batch.ts";

function credentialFault(value: string): NonNullable<BatchState["sourceFault"]> {
  switch (value) { case "missing bearer": case "missing relay": case "malformed bearer": case "malformed relay": return value; default: assert.fail(`unknown source fault ${value}`); }
}

Given("the token refresh moves to subject {string} of organization {string} once the first chunk is delivered", function (this: BatchWorld, subject: string, org: string) {
  state(this).swapAfterDelivery = { subject, org, serial: "refreshed" };
});
Given(/^the token refresh changes the (subject|organization) once the first chunk is delivered$/, function (this: BatchWorld, what: string) {
  const s = state(this);
  s.swapAfterDelivery = { ...s.session, [what === "subject" ? "subject" : "org"]: what === "subject" ? "bob" : "globex", serial: "refreshed" };
});
Given("the server switches the caller to subject {string} of organization {string} while it handles a batch request", function (this: BatchWorld, subject: string, org: string) {
  state(this).swapOnRequest = { subject, org, serial: "refreshed" };
});
When("the callers get {string} at the same time and the token refresh changes the subject before the flush", async function (this: BatchWorld, ids: string) {
  const s = await prepareBatch(this); const client = batchedClient(createPlatformClient(s.service!, s.transport!), s.policy!) as unknown as BatchClient;
  const requested = split(ids);
  const pending = requested.slice(0, -1).map(id => settle(client.getWidget({ id })));
  const lastPending = client.getWidget({ id: requested.at(-1)! });
  s.session = { ...s.session, subject: "bob", serial: "refreshed" };
  let last: Result;
  try { last = { ok: true, value: await lastPending }; }
  catch (error) { last = failedGet(error); }
  this.batchResults = await finishGets(pending, last);
});
When("the callers get {string} and the session source fails before flush", async function (this: BatchWorld, ids: string) {
  const s = await prepareBatch(this); const client = batchedClient(createPlatformClient(s.service!, s.transport!), s.policy!) as unknown as BatchClient;
  const requested = split(ids);
  const pending = requested.slice(0, -1).map(id => settle(client.getWidget({ id })));
  const lastPending = client.getWidget({ id: requested.at(-1)! });
  s.identityFailure = true;
  let last: Result;
  try { last = { ok: true, value: await lastPending }; }
  catch (error) { last = failedGet(error); }
  this.batchResults = await finishGets(pending, last);
});
Given("the session source fails while the server handles a batch request", function (this: BatchWorld) { state(this).failIdentityOnRequest = true; });
Given("the source returns {string} while the server handles a batch request", function (this: BatchWorld, fault: string) { state(this).sourceFaultOnRequest = credentialFault(fault); });
When("the callers get {string} and the source returns {string} before flush", async function (this: BatchWorld, ids: string, fault: string) {
  const s = await prepareBatch(this); const client = batchedClient(createPlatformClient(s.service!, s.transport!), s.policy!) as unknown as BatchClient;
  const requested = split(ids);
  const pending = requested.slice(0, -1).map(id => settle(client.getWidget({ id })));
  const lastPending = client.getWidget({ id: requested.at(-1)! });
  s.sourceFault = credentialFault(fault);
  let last: Result;
  try { last = { ok: true, value: await lastPending }; }
  catch (error) { last = failedGet(error); }
  this.batchResults = await finishGets(pending, last);
});
Given("the caller has started the parts of {string}", async function (this: BatchWorld, parent: string) {
  const s = await prepareBatch(this); const client = batchedClient(createPlatformClient(s.service!, s.transport!), s.policy!) as unknown as BatchClient;
  const groups = await client.batchListWidgetParts([parent]);
  assert.equal(groups.length, 1); assert.ok(groups[0].call);
  s.continuation = groups[0].call;
});
When("the caller continues the seeded parts group", async function (this: BatchWorld) {
  const s = state(this); const iterator = s.continuation!.items()[Symbol.asyncIterator]();
  assert.deepEqual(await iterator.next(), { value: "a", done: false });
  try { await iterator.next(); assert.fail("continuation identity must be checked before sending"); }
  catch (error) { assert.ok(error instanceof PageError); s.continuationError = error.cause; }
  assert.equal((await iterator.next()).done, true);
});
Then("the group continuation fails with {string} and ErrorInfo {string}", function (this: BatchWorld, code: string, info: string) {
  assert.equal(code, "unauthenticated");
  assert.equal(state(this).continuationError?.code, Code.Unauthenticated);
  assert.equal(errorInfoOf(state(this).continuationError!)?.code, info);
});

Given("the caller has started the grouped parts continuation", async function (this: BatchWorld) {
  const s = await prepareBatch(this); const client = batchedClient(createPlatformClient(s.service!, s.transport!), s.policy!) as unknown as BatchClient;
  const groups = await client.batchListWidgetParts(["complete", "continuing"]);
  assert.deepEqual(groups.map(group => group.parentId), ["complete", "continuing"]);
  const independent: string[] = [];
  for await (const item of groups[0].call!.items()) independent.push(item);
  s.groups = [{ parentId: "complete", items: independent }];
  s.continuation = groups[1].call;
});

When("a grouped single-List continuation encounters a {string} in flight", async function (this: BatchWorld, failure: string) {
  const s = state(this); const iterator = s.continuation!.items()[Symbol.asyncIterator]();
  s.release = () => { s.release = undefined; s.events.emit("release"); };
  const arrived = once(s.events, "request", { signal: AbortSignal.timeout(5000) });
  const prefix = await iterator.next();
  assert.deepEqual(prefix, { value: "prefix", done: false });
  const pending = iterator.next();
  await arrived;
  assert.equal(s.requests.at(-1)?.rpc, "ListWidgetParts");
  assert.equal(s.requests.at(-1)?.cursor, "c1");
  if (failure === "source failure") s.identityFailure = true;
  else { assert.equal(failure, "identity change"); s.session = { ...s.session, subject: "bob", serial: "refreshed" }; }
  s.release();
  let cause: ConnectError | undefined;
  try { await pending; assert.fail("in-flight response must be discarded"); }
  catch (error) {
    assert.ok(error instanceof PageError);
    assert.equal(error.deliveredItems, 1);
    assert.equal(error.deliveredPages, 1);
    cause = error.cause;
  }
  s.groups.push({ parentId: "continuing", items: [prefix.value!], error: cause });
  assert.equal((await iterator.next()).done, true);
  assert.equal((await iterator.next()).done, true);
});

Then("the grouped continuation retains its delivered prefix and independent group", function (this: BatchWorld) {
  const s = state(this);
  assert.deepEqual(s.groups[0], { parentId: "complete", items: ["kept"] });
  assert.deepEqual(s.groups[1].items, ["prefix"]);
  assert.equal(s.groups[1].error?.code, Code.Unauthenticated);
  assert.equal(errorInfoOf(s.groups[1].error!)?.code, "BATCH_IDENTITY_CHANGED");
  assert.equal(s.requests.length, 2);
});
