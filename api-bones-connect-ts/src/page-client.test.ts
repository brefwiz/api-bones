// SPDX-License-Identifier: MIT
import { create, createFileRegistry, setExtension } from "@bufbuild/protobuf";
import { FileDescriptorSetSchema, MethodOptionsSchema } from "@bufbuild/protobuf/wkt";
import { createClient } from "@connectrpc/connect";
import { describe, expect, it, vi } from "vitest";
import { batchedClient } from "./batch.js";
import { list_stream_of } from "./generated/bones/v1/annotations_pb.js";
import { pagedClient, PagingPolicyError, type ListCall, type Page } from "./page.js";
import { anonymousSession, createPlatformClient, preparedTransport } from "./platform-client.js";
import type { GeneratedMethodPolicy, GeneratedMethodPolicyDocument } from "./policy.js";
import { RevisionCache } from "./revision-cache.js";

const streamOptions = create(MethodOptionsSchema);
setExtension(streamOptions, list_stream_of, "ListWidgets");
const registry = createFileRegistry(create(FileDescriptorSetSchema, { file: [
  { name: "pages.proto", package: "bones.v1", syntax: "proto3", messageType: [
    { name: "PageRequest", field: [{ name: "cursor", number: 1, type: 9 }, { name: "page_size", number: 2, type: 5 }] },
    { name: "PageResponse", field: [{ name: "next_cursor", number: 1, type: 9 }, { name: "revision", number: 2, type: 9 }] },
  ] },
  { name: "widgets.proto", package: "paging.v1", syntax: "proto3", dependency: ["pages.proto"], messageType: [
    { name: "Widget", field: [{ name: "id", number: 1, type: 9 }] },
    { name: "WriteRequest", field: [{ name: "id", number: 1, type: 9 }] },
    { name: "Request", field: [{ name: "page", number: 1, type: 11, typeName: ".bones.v1.PageRequest" }] },
    { name: "Response", field: [
      { name: "widgets", number: 1, type: 11, typeName: ".paging.v1.Widget", label: 3 },
      { name: "page", number: 2, type: 11, typeName: ".bones.v1.PageResponse" },
    ] },
    { name: "Scalars", field: [
      { name: "items", number: 1, type: 9, label: 3 },
      { name: "page", number: 2, type: 11, typeName: ".bones.v1.PageResponse" },
    ] },
    { name: "Ambiguous", field: [
      { name: "widgets", number: 1, type: 11, typeName: ".paging.v1.Widget", label: 3 },
      { name: "others", number: 2, type: 11, typeName: ".paging.v1.Widget", label: 3 },
      { name: "page", number: 3, type: 11, typeName: ".bones.v1.PageResponse" },
    ] },
  ], service: [{ name: "Widgets", method: [
    { name: "ListWidgets", inputType: ".paging.v1.Request", outputType: ".paging.v1.Response", options: { idempotencyLevel: 1 } },
    { name: "ListOtherWidgets", inputType: ".paging.v1.Request", outputType: ".paging.v1.Response", options: { idempotencyLevel: 1 } },
    { name: "StreamWidgets", inputType: ".paging.v1.Request", outputType: ".paging.v1.Response", serverStreaming: true, options: streamOptions },
    { name: "Write", inputType: ".paging.v1.WriteRequest", outputType: ".paging.v1.Response" },
    { name: "Scalars", inputType: ".paging.v1.Request", outputType: ".paging.v1.Scalars" },
    { name: "Ambiguous", inputType: ".paging.v1.Request", outputType: ".paging.v1.Ambiguous", options: { idempotencyLevel: 1 } },
    { name: "PlainStream", inputType: ".paging.v1.Request", outputType: ".paging.v1.Response", serverStreaming: true },
    { name: "NonPageStream", inputType: ".paging.v1.WriteRequest", outputType: ".paging.v1.Response", serverStreaming: true, options: streamOptions },
    { name: "ClientStream", inputType: ".paging.v1.Request", outputType: ".paging.v1.Response", clientStreaming: true },
  ] }] },
] }));
const service = registry.getService("paging.v1.Widgets")!;
const paging = { kind: "unary" as const, itemField: "widgets", pageField: "page", requestPageField: "page" };
const entry = (name: string, extra: Partial<GeneratedMethodPolicy> = {}): GeneratedMethodPolicy => ({
  rpc: `/paging.v1.Widgets/${name}`, procedure: "unary", idempotency: "NO_SIDE_EFFECTS",
  browserCache: { scope: "NO_STORE", maxAgeSeconds: 0 }, sensitivity: "NON_SENSITIVE", maxEncodedUrlBytes: 0, ...extra,
});
const policy: GeneratedMethodPolicyDocument = { schemaVersion: 3, methods: [
  entry("ListWidgets", { paging }), entry("ListOtherWidgets", { paging }),
  entry("Scalars", { idempotency: "UNSPECIFIED", paging: { ...paging, itemField: "items" } }),
  entry("StreamWidgets", { procedure: "streaming", paging: { ...paging, kind: "stream", streamOf: "ListWidgets" } }),
] };
const call = <T = { id: string }>(value: unknown) => value as (request: Record<string, unknown>) => ListCall<T>;
function transport() {
  const unary = vi.fn(async (method, _signal, _timeout, headers) => {
    const requestHeaders = new Headers(headers);
    const responseHeaders = new Headers();
    if (requestHeaders.has("if-none-match")) responseHeaders.set("bones-not-modified", "1");
    return { stream: false, service, method, header: responseHeaders, trailer: new Headers(),
      message: create(method.output, method.name === "Scalars" ? { items: ["w1"], page: { revision: "r1" } } : { widgets: [{ id: "w1" }], page: { revision: "r1" } }) };
  });
  return preparedTransport({ unary, stream: vi.fn() }, anonymousSession);
}
async function collect(source: AsyncIterable<Page<{ id: string }>>) {
  const result: Page<{ id: string }>[] = [];
  for await (const page of source) result.push(page);
  return result;
}

describe("generated paging discovery", () => {
  it.each(["ListWidgets", "StreamWidgets", "Scalars"])("names %s when its paging entry is missing", name => {
    const raw = createPlatformClient(service, transport());
    const missing = { ...policy, methods: policy.methods.map(method => method.rpc.endsWith(`/${name}`) ? { ...method, paging: undefined } : method) };
    expect(() => pagedClient(raw, missing)).toThrow(PagingPolicyError);
    expect(() => pagedClient(raw, missing)).toThrow(`missing paging policy for /paging.v1.Widgets/${name}`);
    try { pagedClient(raw, missing); } catch (error) {
      expect(error).toMatchObject({ method: `/paging.v1.Widgets/${name}` });
    }
  });

  it("refuses a list absent from the policy document entirely", () => {
    const raw = createPlatformClient(service, transport());
    expect(() => pagedClient(raw, { ...policy, methods: policy.methods.slice(1) })).toThrow(PagingPolicyError);
  });

  it("wraps policy entries and leaves non-List shapes untouched", async () => {
    const raw = createPlatformClient(service, transport());
    const wrapped = pagedClient(raw, policy);
    for (const name of ["write", "ambiguous", "plainStream", "nonPageStream", "clientStream"]) expect(wrapped[name]).toBe(raw[name]);
    expect(wrapped.scalars).not.toBe(raw.scalars);
    expect(wrapped.listWidgets).not.toBe(raw.listWidgets);
    expect(wrapped.streamWidgets).not.toBe(raw.streamWidgets);
    await expect(call<string>(wrapped.scalars)({}).firstPage()).resolves.toMatchObject({ items: ["w1"], revision: "r1" });
    await expect(call(wrapped.listWidgets)({}).firstPage()).resolves.toMatchObject({ items: [{ id: "w1" }], revision: "r1" });
  });

  it("keeps raw generated clients outside batching construction", () => {
    expect(() => batchedClient(createClient(service, transport()), policy)).toThrow("batching requires createPlatformClient");
  });

  it("permits paging-only construction without a batching session", async () => {
    const wire = transport();
    preparedTransport(wire);
    const raw = createPlatformClient(service, wire);
    await expect(call(pagedClient(raw, policy).listWidgets)({}).firstPage()).resolves.toMatchObject({ revision: "r1" });
    expect(() => batchedClient(raw, policy)).toThrow("batching requires a producer-owned session binding");
  });
});

describe("platform paging composition", () => {
  it("composes batching beneath paging and retains the two-argument raw constructor", async () => {
    const wire = transport();
    const raw = createPlatformClient(service, wire);
    expect(raw.listWidgets({})).toBeInstanceOf(Promise);
    const batched = batchedClient(raw, policy);
    const wrapped = pagedClient(batched, policy);
    await expect(call(wrapped.listWidgets)({}).firstPage()).resolves.toMatchObject({ items: [{ id: "w1" }] });
    const composed = createPlatformClient(service, wire, policy);
    await expect(call(composed.listWidgets)({}).firstPage()).resolves.toMatchObject({ items: [{ id: "w1" }] });
  });

  it("uses one cache across each client's lists and separates client instances", async () => {
    const keys = vi.spyOn(RevisionCache.prototype, "key");
    try {
      const wire = transport();
      const first = createPlatformClient(service, wire, policy);
      const second = createPlatformClient(service, wire, policy);
      const held = await collect(call(first.listWidgets)({}).pages());
      const firstCache = keys.mock.contexts[0];
      keys.mockClear();
      await collect(call(first.listOtherWidgets)({}).pages());
      expect(new Set(keys.mock.contexts)).toEqual(new Set([firstCache]));
      keys.mockClear();
      const replay = await collect(call(first.listWidgets)({}).pages());
      expect(replay[0].fromCache).toBe(true);
      expect(replay[0].items).toBe(held[0].items);
      expect(new Set(keys.mock.contexts)).toEqual(new Set([firstCache]));
      keys.mockClear();
      const fresh = await collect(call(second.listWidgets)({}).pages());
      expect(fresh[0].fromCache).toBe(false);
      expect(new Set(keys.mock.contexts).size).toBe(1);
      expect(keys.mock.contexts[0]).not.toBe(firstCache);
    } finally { keys.mockRestore(); }
  });
});
