// SPDX-License-Identifier: MIT
import assert from "node:assert/strict";
import { rmSync } from "node:fs";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import type { DescMethodUnary, DescService } from "@bufbuild/protobuf";
import { After, Given, Then, When, World } from "@cucumber/cucumber";
import { connectNodeAdapter } from "@connectrpc/connect-node";
import { anonymousSession, createPlatformClient, type GeneratedMethodPolicyDocument } from "@brefwiz/api-bones-connect";
import { configureConnectTransport } from "@brefwiz/api-bones-connect/web";
import { compileContract } from "./descriptor_contract.ts";

const LIST_ONLY = `syntax = "proto3";
package contract.v1;
import "bones/v1/pagination.proto";
message ListNumbersRequest { bones.v1.PageRequest page = 1; }
message ListNumbersResponse { repeated uint32 items = 1; bones.v1.PageResponse page = 2; }
service Numbers {
  rpc List(ListNumbersRequest) returns (ListNumbersResponse) { option idempotency_level = NO_SIDE_EFFECTS; }
}`;
const NEITHER = `syntax = "proto3";
package contract.v1;
message Ping { string id = 1; }
service Plain {
  rpc Get(Ping) returns (Ping) { option idempotency_level = NO_SIDE_EFFECTS; }
}`;
const LIST_POLICY: GeneratedMethodPolicyDocument = {
  schemaVersion: 3,
  methods: [{
    rpc: "/contract.v1.Numbers/List",
    procedure: "unary",
    idempotency: "NO_SIDE_EFFECTS",
    browserCache: { scope: "NO_STORE", maxAgeSeconds: 0 },
    sensitivity: "UNSPECIFIED",
    maxEncodedUrlBytes: 4096,
    paging: { kind: "unary", itemField: "items", pageField: "page", requestPageField: "page" },
  }],
};

interface UnbatchedPagingWorld extends World {
  unbatched?: { pages: number[][]; requests: number; server?: Server; workdir?: string; service?: DescService; items: number[] };
  constructionError?: unknown;
}
const numbers = (list: string) => list.split(",").filter(Boolean).map(Number);
const unbatched = (world: UnbatchedPagingWorld) => world.unbatched ??= { pages: [], requests: 0, items: [] };

After(async function (this: UnbatchedPagingWorld) {
  const s = this.unbatched;
  if (s?.server?.listening) await new Promise<void>(resolve => { s.server!.close(() => resolve()); s.server!.closeAllConnections(); });
  if (s?.workdir) rmSync(s.workdir, { recursive: true, force: true });
});

Given("a list-only service whose pages are {string}", function (this: UnbatchedPagingWorld, pages: string) {
  unbatched(this).pages = pages.split("|").map(numbers);
});
When("the caller reads every item of the list-only service through its platform client", async function (this: UnbatchedPagingWorld) {
  const s = unbatched(this);
  const compiled = compileContract("bones-paged-without-batch", LIST_ONLY);
  s.workdir = compiled.workdir;
  s.service = compiled.registry.getService("contract.v1.Numbers")!;
  const handler = connectNodeAdapter({ routes(router) {
    router.rpc(s.service!.methods[0] as DescMethodUnary, async request => {
      s.requests++;
      const cursor = (request as unknown as { page?: { cursor: string } }).page?.cursor ?? "";
      const named = /^c(\d+)$/.exec(cursor);
      const index = named ? Number(named[1]) : 0;
      return { items: s.pages[index] ?? [], page: { nextCursor: index + 1 < s.pages.length ? `c${index + 1}` : "", revision: "r1" } } as never;
    });
  } });
  s.server = createServer(handler);
  await new Promise<void>(resolve => s.server!.listen(0, "127.0.0.1", resolve));
  const transport = configureConnectTransport({ baseUrl: `http://127.0.0.1:${(s.server.address() as AddressInfo).port}`, profile: "service", policy: LIST_POLICY, session: anonymousSession });
  const client = createPlatformClient(s.service, transport, LIST_POLICY) as unknown as { list(request: object): { items(): AsyncIterable<number> } };
  for await (const item of client.list({}).items()) s.items.push(item);
});
Then("the caller receives the numbers {string}", function (this: UnbatchedPagingWorld, expected: string) {
  assert.deepEqual(unbatched(this).items, numbers(expected));
});
Then("the list-only service answered {int} list requests", function (this: UnbatchedPagingWorld, count: number) {
  assert.equal(unbatched(this).requests, count);
});

Given("a service with neither batch nor paging methods", function (this: UnbatchedPagingWorld) {
  const s = unbatched(this);
  const compiled = compileContract("bones-neither", NEITHER);
  s.workdir = compiled.workdir;
  s.service = compiled.registry.getService("contract.v1.Plain")!;
});
When("the SDK constructs a platform client for that service", function (this: UnbatchedPagingWorld) {
  const s = unbatched(this);
  const policy: GeneratedMethodPolicyDocument = { schemaVersion: 3, methods: [] };
  const transport = configureConnectTransport({ baseUrl: "http://127.0.0.1:1", profile: "service", policy, session: anonymousSession });
  try { createPlatformClient(s.service!, transport, policy); }
  catch (error) { this.constructionError = error; }
});
Then("platform client construction fails with {string}", function (this: UnbatchedPagingWorld, message: string) {
  assert.ok(this.constructionError instanceof Error);
  assert.ok(this.constructionError.message.includes(message));
});
