// SPDX-License-Identifier: MIT
import assert from "node:assert/strict";
import { rmSync } from "node:fs";
import { After, Given, Then, When } from "@cucumber/cucumber";
import { createPlatformClient, pagedClient, PagingPolicyError, type Page, type ListCall, type GeneratedMethodPolicyDocument } from "@brefwiz/api-bones-connect";
import { compileContract } from "./descriptor_contract.ts";
import { prepareBatch, state, split, type BatchWorld, type BatchClient } from "./batch.ts";

const RPC = "/contract.v1.Entries/";
const CONTRACT = `syntax = "proto3";
package contract.v1;
import "bones/v1/annotations.proto";
import "bones/v1/pagination.proto";
message Entry { string id = 1; }
message ListEntriesRequest { bones.v1.PageRequest page = 1; }
message ListEntriesResponse { repeated Entry items = 1; bones.v1.PageResponse page = 2; }
service Entries {
  rpc ListEntries(ListEntriesRequest) returns (ListEntriesResponse) { option idempotency_level = NO_SIDE_EFFECTS; }
  rpc StreamEntries(ListEntriesRequest) returns (stream ListEntriesResponse) {
    option (bones.v1.list_stream_of) = "ListEntries";
    option (bones.v1.call_shape) = CALL_SHAPE_FINITE_STREAM;
  }
}`;
type ComposedClient = BatchClient & { listWidgetParts(request: { widgetId: string }, options?: { prefetch: 0 }): ListCall<string> };
interface PaginationConstructionWorld extends BatchWorld {
  missingPagingMethod?: string;
  pagingConstructionError?: unknown;
  pagingContractDirectory?: string;
  composedClient?: ComposedClient;
  independentClient?: ComposedClient;
  composedHeldPages?: Page<string>[];
}
Given("the generated pagination method is {string}", function (this: PaginationConstructionWorld, method: string) {
  assert.ok(["ListEntries", "StreamEntries"].includes(method));
  this.missingPagingMethod = method;
});
When("the SDK constructs a paginated client without that method's paging entry", async function (this: PaginationConstructionWorld) {
  const s = await prepareBatch(this);
  const compiled = compileContract("bones-pagination-construction", CONTRACT);
  this.pagingContractDirectory = compiled.workdir;
  const service = compiled.registry.getService("contract.v1.Entries");
  assert.ok(service);
  const policy: GeneratedMethodPolicyDocument = { ...s.policy!, methods: this.missingPagingMethod === "ListEntries" ? [] : [{
    ...s.policy!.methods.find(method => method.paging)!,
    rpc: `${RPC}ListEntries`,
    paging: { kind: "unary", itemField: "items", pageField: "page", requestPageField: "page" },
  }] };
  try { pagedClient(createPlatformClient(service, s.transport!), policy); }
  catch (error) { this.pagingConstructionError = error; }
});
Then("pagination construction fails with a typed error naming {string}", function (this: PaginationConstructionWorld, method: string) {
  assert.ok(this.pagingConstructionError instanceof PagingPolicyError);
  assert.equal(this.pagingConstructionError.method, `${RPC}${method}`);
  assert.ok(this.pagingConstructionError.message.includes(`${RPC}${method}`));
});
When("the caller constructs two composed platform clients from discovery", async function (this: PaginationConstructionWorld) {
  state(this).pageSize = 2;
  const s = await prepareBatch(this);
  this.composedClient = createPlatformClient(s.service!, s.transport!, s.policy!) as unknown as ComposedClient;
  this.independentClient = createPlatformClient(s.service!, s.transport!, s.policy!) as unknown as ComposedClient;
});
Then("the composed client batch-gets widgets {string}", async function (this: PaginationConstructionWorld, ids: string) {
  assert.ok(this.composedClient);
  const results = await this.composedClient.batchGetWidgets(split(ids));
  assert.deepEqual(results.map(result => { assert.ok(result.ok); return result.value.id; }), split(ids));
  assert.equal(state(this).requests.filter(request => request.rpc === "BatchGetWidgets").length, 1);
});
async function pages(client: ComposedClient): Promise<Page<string>[]> {
  const held: Page<string>[] = [];
  for await (const page of client.listWidgetParts({ widgetId: "w1" }, { prefetch: 0 }).pages()) held.push(page);
  assert.deepEqual(held.flatMap(page => [...page.items]), ["p1", "p2", "p3"]);
  return held;
}
Then("the composed client replays its retained list pages", async function (this: PaginationConstructionWorld) {
  assert.ok(this.composedClient);
  this.composedHeldPages = await pages(this.composedClient);
  assert.equal(this.composedHeldPages.length, 2);
  const replay = await pages(this.composedClient);
  assert.ok(replay.every(page => page.fromCache));
  const requests = state(this).requests.filter(request => request.rpc === "ListWidgetParts");
  assert.equal(requests.length, 3);
  assert.equal(requests[2].revision, `"${this.composedHeldPages[0].revision}"`);
});
Then("the second composed client fetches its own list pages", async function (this: PaginationConstructionWorld) {
  assert.ok(this.independentClient && this.composedHeldPages);
  const own = await pages(this.independentClient);
  assert.ok(own.every(page => !page.fromCache));
  const requests = state(this).requests.filter(request => request.rpc === "ListWidgetParts");
  assert.equal(requests.length, 5);
  assert.equal(requests[3].revision, null);
});
After(function (this: PaginationConstructionWorld) {
  if (this.pagingContractDirectory) rmSync(this.pagingContractDirectory, { recursive: true, force: true });
});
