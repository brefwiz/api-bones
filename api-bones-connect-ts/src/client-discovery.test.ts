// SPDX-License-Identifier: MIT
import { create, createFileRegistry } from "@bufbuild/protobuf";
import { FileDescriptorSetSchema } from "@bufbuild/protobuf/wkt";
import { Code, ConnectError } from "@connectrpc/connect";
import { connectNodeAdapter } from "@connectrpc/connect-node";
import { once } from "node:events";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { describe, expect, it, vi } from "vitest";
import { batchedClient } from "./batch.js";
import { configureNodeConnectTransport } from "./node.js";
import { pagedClient } from "./page.js";
import { anonymousSession, createPlatformClient, prepareHeaders } from "./platform-client.js";
import { indexGeneratedPolicy, type GeneratedMethodPolicy, type GeneratedMethodPolicyDocument } from "./policy.js";

const registry = createFileRegistry(create(FileDescriptorSetSchema, { file: [
  { name: "errors.proto", package: "bones.v1", syntax: "proto3", messageType: [{ name: "ItemError" }] },
  {
    name: "discovery.proto", package: "discovery.v1", syntax: "proto3", dependency: ["errors.proto"],
    messageType: [
      { name: "Widget", field: [{ name: "id", number: 1, type: 9 }] },
      { name: "GetRequest", field: [{ name: "id", number: 1, type: 9 }] },
      { name: "BatchRequest", field: [{ name: "ids", number: 1, type: 9, label: 3 }] },
      {
        name: "Result", oneofDecl: [{ name: "outcome" }], field: [
          { name: "id", number: 1, type: 9 },
          { name: "item", number: 2, type: 11, typeName: ".discovery.v1.Widget", oneofIndex: 0 },
          { name: "error", number: 3, type: 11, typeName: ".bones.v1.ItemError", oneofIndex: 0 },
        ],
      },
      { name: "BatchResponse", field: [{ name: "results", number: 1, type: 11, typeName: ".discovery.v1.Result", label: 3 }] },
    ],
    service: [{ name: "Widgets", method: [
      { name: "GetWidget", inputType: ".discovery.v1.GetRequest", outputType: ".discovery.v1.Widget" },
      { name: "BatchGetWidgets", inputType: ".discovery.v1.BatchRequest", outputType: ".discovery.v1.BatchResponse" },
      { name: "List", inputType: ".discovery.v1.GetRequest", outputType: ".discovery.v1.BatchResponse" },
    ] }],
  },
] }));
const service = registry.getService("discovery.v1.Widgets")!;
const method = (name: string, extra: Partial<GeneratedMethodPolicy> = {}): GeneratedMethodPolicy => ({
  rpc: `/discovery.v1.Widgets/${name}`, procedure: "unary", idempotency: "NO_SIDE_EFFECTS",
  browserCache: { scope: "NO_STORE", maxAgeSeconds: 0 }, sensitivity: "NON_SENSITIVE", maxEncodedUrlBytes: 0, ...extra,
});
const paging = { kind: "unary" as const, itemField: "results", pageField: "page", requestPageField: "page" };

describe("service-scoped generated client discovery", () => {
  it.each(["authorization", "brefwiz-relayed-credential"])("rejects duplicate %s headers as invalid argument before transport preparation", async name => {
    const unary = vi.fn();
    const transport = { unary, stream: vi.fn() };
    for (const reverse of [false, true]) {
      const headers: [string, string][] = [[name, "first"], [name.toUpperCase(), "second"]];
      if (reverse) headers.reverse();
      await expect(prepareHeaders(transport, service.method.getWidget, {}, headers)).rejects.toMatchObject({ code: Code.InvalidArgument });
    }
    expect(unary).not.toHaveBeenCalled();
  });

  it("rejects an explicit batch aborted during its first chunk with one cancelled completion", async () => {
    const policy: GeneratedMethodPolicyDocument = { schemaVersion: 3, methods: [
      method("GetWidget", { batch: { kind: "get", method: "BatchGetWidgets", keyField: "id", keysField: "ids", resultsField: "results" } }),
      method("BatchGetWidgets"),
    ] };
    const controller = new AbortController();
    let requests = 0;
    const server = createServer(connectNodeAdapter({ routes(router) {
      router.rpc(service.method.batchGetWidgets, () => {
        requests++;
        controller.abort();
        throw new ConnectError("batch cancelled", Code.Canceled);
      });
    } }));
    server.listen(0, "127.0.0.1");
    await once(server, "listening");
    const transport = await configureNodeConnectTransport({ baseUrl: `http://127.0.0.1:${(server.address() as AddressInfo).port}`, httpVersion: "1.1", profile: "service", policy, session: anonymousSession });
    const completion = vi.spyOn(console, "info").mockImplementation(() => {});
    try {
      const client = batchedClient(createPlatformClient(service, transport), policy);
      const ids = Array.from({ length: 450 }, (_, index) => `w-${index + 1}`);
      await expect(client.batchGetWidgets(ids, {}, { signal: controller.signal })).rejects.toMatchObject({ code: Code.Canceled });
      expect(requests).toBe(1);
      const records = completion.mock.calls.map(([value]) => JSON.parse(String(value))).filter(value => value.target === "bones.batch");
      expect(records).toHaveLength(1);
      expect(records[0]["bones.batch.outcome"]).toBe("cancelled");
    } finally {
      completion.mockRestore();
      transport.close();
      server.closeAllConnections();
      await new Promise<void>((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
    }
  });

  it("resolves batch safety from the real policy index's canonical RPC key", async () => {
    const policy: GeneratedMethodPolicyDocument = { schemaVersion: 3, methods: [
      method("GetWidget", { batch: { kind: "get", method: "BatchGetWidgets", keyField: "id", keysField: "ids", resultsField: "results" } }),
      method("BatchGetWidgets"),
    ] };
    expect(indexGeneratedPolicy(policy).get("/discovery.v1.Widgets/BatchGetWidgets")?.idempotency).toBe("NO_SIDE_EFFECTS");
    const transport = await configureNodeConnectTransport({ baseUrl: "http://localhost", profile: "service", policy, session: anonymousSession });
    try {
      const client = createPlatformClient(service, transport);
      expect(batchedClient(client, policy).batchGetWidgets).not.toBe(client.batchGetWidgets);
      const foreign = { ...policy.methods[0], rpc: "/discovery.v1.Other/GetOther", batch: { ...policy.methods[0].batch!, method: "BatchGetOthers" } };
      expect(() => batchedClient(client, { ...policy, methods: [...policy.methods, foreign] })).not.toThrow();
      expect(() => batchedClient(client, { ...policy, methods: [policy.methods[0], method("BatchGetWidgets", { idempotency: "IDEMPOTENT" })] })).toThrow("must declare NO_SIDE_EFFECTS");
    } finally { transport.close(); }
  });

  it("wraps its service's list while ignoring another service's paging methods", async () => {
    const policy: GeneratedMethodPolicyDocument = { schemaVersion: 3, methods: [
      method("List", { paging }), method("ListOther", { rpc: "/discovery.v1.Other/ListOther", paging }),
    ] };
    const transport = await configureNodeConnectTransport({ baseUrl: "http://localhost", profile: "service", policy, session: anonymousSession });
    try {
      const client = createPlatformClient(service, transport);
      expect(pagedClient(client, policy).list).not.toBe(client.list);
      expect(pagedClient(client, policy).getWidget).toBe(client.getWidget);
    } finally { transport.close(); }
  });

  it("refuses a missing list method declared for its own service", async () => {
    const policy: GeneratedMethodPolicyDocument = { schemaVersion: 3, methods: [method("MissingList", { paging })] };
    const transport = await configureNodeConnectTransport({ baseUrl: "http://localhost", profile: "service", policy, session: anonymousSession });
    try {
      expect(() => pagedClient(createPlatformClient(service, transport), policy)).toThrow("missing list method /discovery.v1.Widgets/MissingList");
    } finally { transport.close(); }
  });
});
