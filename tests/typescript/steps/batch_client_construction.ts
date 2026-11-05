// SPDX-License-Identifier: MIT
import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import type { AddressInfo } from "node:net";
import { ScalarType } from "@bufbuild/protobuf";
import { Code, createClient } from "@connectrpc/connect";
import { createConnectTransport } from "@connectrpc/connect-web";
import { Given, Then, When } from "@cucumber/cucumber";
import { anonymousSession, batchedClient, createPlatformClient } from "@brefwiz/api-bones-connect";
import { configureConnectTransport, createPlatformClient as createWebClient } from "@brefwiz/api-bones-connect/web";
import { createPlatformClient as createNodeClient } from "@brefwiz/api-bones-connect/node";
import { prepareBatch, split, type BatchWorld, type BatchClient } from "./batch.ts";

type ConstructionWorld = BatchWorld & { constructionError?: unknown; entrypointClient?: BatchClient; anonymousRequests?: { credentials?: RequestCredentials; headers: Headers }[] };
When("an explicitly anonymous platform client batch-gets {string} carrying an ambient cookie", async function (this: ConstructionWorld, ids: string) {
  const s = await prepareBatch(this);
  this.anonymousRequests = [];
  const transport = configureConnectTransport({ baseUrl: `http://127.0.0.1:${(s.server!.address() as AddressInfo).port}`, profile: "service", policy: s.policy!, session: anonymousSession, fetch: (input, init) => { this.anonymousRequests!.push({ credentials: init?.credentials, headers: new Headers(init?.headers) }); return globalThis.fetch(input, init); } });
  const client = batchedClient(createPlatformClient(s.service!, transport), s.policy!) as unknown as BatchClient;
  this.batchResults = [...await client.batchGetWidgets(split(ids), {}, { headers: { cookie: `session=${randomUUID()}` } })];
});
Then("anonymous batch requests omit credentials", function (this: ConstructionWorld) {
  assert.ok(this.batchResults.length);
  assert.equal(this.batchResults.every(result => !result.ok && result.error.code === Code.Unauthenticated), true);
  assert.equal(this.anonymousRequests?.length, 1);
  for (const request of this.anonymousRequests!) { assert.equal(request.credentials, "omit"); for (const name of ["cookie", "authorization", "brefwiz-relayed-credential"]) assert.equal(request.headers.has(name), false); }
});
Given("the platform entry point is {string}", async function (this: ConstructionWorld, entrypoint: string) {
  const s = await prepareBatch(this);
  switch (entrypoint) {
    case "default": this.entrypointClient = batchedClient(createPlatformClient(s.service!, s.transport!), s.policy!) as unknown as BatchClient; break;
    case "web": this.entrypointClient = batchedClient(createWebClient(s.service!, s.transport!), s.policy!) as unknown as BatchClient; break;
    case "node": this.entrypointClient = batchedClient(createNodeClient(s.service!, s.transport!), s.policy!) as unknown as BatchClient; break;
    default: assert.fail(`unknown platform entry point ${entrypoint}`);
  }
});
When("the constructed platform client batch-gets {string}", async function (this: ConstructionWorld, ids: string) {
  assert.ok(this.entrypointClient);
  this.batchResults = [...await this.entrypointClient.batchGetWidgets(split(ids))];
});
Then("the constructed client returns the widgets {string}", function (this: ConstructionWorld, ids: string) {
  assert.deepEqual(this.batchResults.map(result => { assert.ok(result.ok); return result.value.id; }), split(ids));
});
When("batching wraps an ordinary generated client", async function (this: ConstructionWorld) {
  const s = await prepareBatch(this);
  try { batchedClient(createClient(s.service!, s.transport!), s.policy!); }
  catch (error) { this.constructionError = error; }
});
When("a platform client is constructed with an ordinary Connect transport", async function (this: ConstructionWorld) {
  const s = await prepareBatch(this);
  try { createPlatformClient(s.service!, createConnectTransport({ baseUrl: "http://127.0.0.1:1" })); }
  catch (error) { this.constructionError = error; }
});
Then("construction fails with {string}", function (this: ConstructionWorld, message: string) {
  assert.ok(this.constructionError instanceof Error);
  assert.ok(this.constructionError.message.includes(message));
});

async function constructWithPolicyDrift(this: ConstructionWorld, drift: string) {
  const s = await prepareBatch(this);
  let service = s.service!;
  const policy = JSON.parse(JSON.stringify(s.policy!));
  const entry = policy.methods.find((method: { rpc: string }) => method.rpc.endsWith("/GetWidget"));
  assert.ok(entry?.batch, "generated fixture declares GetWidget batch relationship");
  switch (drift) {
    case "unsupported schema": policy.schemaVersion = 999; break;
    case "empty discovery": policy.methods = []; break;
    case "missing single": entry.rpc = "/contract.v1.Widgets/Missing"; break;
    case "missing batch": entry.batch.method = "/contract.v1.Widgets/Missing"; break;
    case "missing key": entry.batch.keyField = "missing"; break;
    case "missing keys": entry.batch.keysField = "missing"; break;
    case "missing results": entry.batch.resultsField = "missing"; break;
    case "non-string key": entry.rpc = "/contract.v1.Widgets/ListWidgetParts"; entry.batch.keyField = "page"; break;
    case "scalar keys": entry.batch.keysField = "kind"; break;
    case "scalar results": {
      const output = service.methods.find(method => method.name === "GetWidget")!.output;
      service = { ...service, methods: service.methods.map(method => method.name === "BatchGetWidgets" ? { ...method, output } : method) } as typeof service;
      entry.batch.resultsField = "id"; break;
    }
    case "wrong item response": entry.rpc = "/contract.v1.Widgets/ListWidgetParts"; entry.batch.keyField = "widget_id"; break;
    case "wrong group item type": {
      const batch = service.methods.find(method => method.name === "BatchListWidgetParts")!;
      const groups = batch.output.fields.find(field => field.name === "groups")!;
      assert.ok(groups.fieldKind === "list" && groups.listKind === "message");
      const output = { ...batch.output, fields: batch.output.fields.map(field => field === groups ? { ...groups, message: { ...groups.message, fields: groups.message.fields.map(item => item.name === "items" ? { ...item, scalar: ScalarType.INT32 } : item) } } : field) };
      service = { ...service, methods: service.methods.map(method => method === batch ? { ...batch, output } : method) } as typeof service;
      break;
    }
    case "non-string result key":
    case "wrong error type":
    case "wrong item type":
    case "missing outcome":
    case "scalar group items": {
      const batch = service.methods.find(method => method.name === "BatchGetWidgets")!;
      const results = batch.output.fields.find(field => field.name === "results")!;
      assert.ok(results.fieldKind === "list" && results.listKind === "message");
      const row = results.message;
      let fields = row.fields.map(field => {
        if (drift === "non-string result key" && field.name === "id") return { ...field, scalar: ScalarType.INT32 };
        if (drift === "wrong error type" && field.name === "error") return { ...field, message: batch.input };
        if (drift === "wrong item type" && field.name === "item") return { ...field, message: batch.input };
        if (drift === "missing outcome" && field.name === "item") return { ...field, oneof: undefined };
        return field;
      });
      if (drift === "scalar group items") { entry.batch.kind = "list"; fields = [...fields, { ...row.fields.find(field => field.name === "id")!, name: "items", localName: "items", jsonName: "items" }]; }
      const output = { ...batch.output, fields: batch.output.fields.map(field => field === results ? { ...results, message: { ...row, fields } } : field) };
      service = { ...service, methods: service.methods.map(method => method === batch ? { ...batch, output } : method) } as typeof service;
      break;
    }
    case "mutating batch": policy.methods.find((method: { rpc: string }) => method.rpc.endsWith("/BatchGetWidgets")).idempotency = "IDEMPOTENT"; break;
    default: assert.fail(`unknown drift ${drift}`);
  }
  if (drift === "non-string key" || drift === "wrong item response") policy.methods = policy.methods.filter((method: { rpc: string }) => method === entry || !method.rpc.endsWith("/ListWidgetParts"));
  try { batchedClient(createPlatformClient(service, s.transport!), policy); }
  catch (error) { this.constructionError = error; }
}

When("batching constructs a generated client with {string} policy drift", constructWithPolicyDrift);
When("the SDK constructs a batched client with {string} policy drift", constructWithPolicyDrift);
