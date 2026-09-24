// SPDX-License-Identifier: MIT
import type { DescMethodUnary } from "@bufbuild/protobuf";
import { EmptySchema, StringValueSchema } from "@bufbuild/protobuf/wkt";
import { describe, expect, it } from "vitest";

import {
  eligiblePublicReadPolicy,
  indexGeneratedPolicy,
  parseMethodPolicy,
  publicLaneUrl,
} from "./policy.js";
import { configureConnectTransport } from "./web.js";

const tenantField = { name: "org_handle", jsonName: "orgHandle", number: 1 };

const publicRead = (over: Record<string, unknown> = {}) => ({
  rpc: "/pkg.v1.PublicService/GetWeek",
  procedure: "unary",
  idempotency: "NO_SIDE_EFFECTS",
  browserCache: { scope: "NO_STORE", maxAgeSeconds: 0 },
  sensitivity: "NON_SENSITIVE",
  maxEncodedUrlBytes: 4096,
  publicRead: { maxAgeSeconds: 60, origins: "OWNER_CONFIRMED", tenantField },
  ...over,
});

const ordinary = {
  rpc: "/pkg.v1.PublicService/Book",
  procedure: "unary",
  idempotency: "UNSPECIFIED",
  browserCache: { scope: "NO_STORE", maxAgeSeconds: 0 },
  sensitivity: "UNSPECIFIED",
  maxEncodedUrlBytes: 4096,
};

describe("public read policy", () => {
  it("reads a public read with its bound, origins and tenant", () => {
    expect(parseMethodPolicy(publicRead())?.publicRead).toEqual({
      maxAgeSeconds: 60,
      origins: "OWNER_CONFIRMED",
      tenantField,
    });
    expect(eligiblePublicReadPolicy(publicRead())).not.toBeNull();
    expect(eligiblePublicReadPolicy(ordinary)).toBeNull();
  });

  it("fails closed on a public read it cannot trust", () => {
    for (const broken of [
      { maxAgeSeconds: 0, origins: "ANY", tenantField },
      { maxAgeSeconds: 301, origins: "ANY", tenantField },
      { maxAgeSeconds: 60, origins: "UNSPECIFIED", tenantField },
      { maxAgeSeconds: 60, origins: "ANY" },
      { maxAgeSeconds: 60, origins: "ANY", tenantField: { ...tenantField, number: 0 } },
    ]) {
      expect(parseMethodPolicy(publicRead({ publicRead: broken }))).toBeNull();
      expect(
        indexGeneratedPolicy({ schemaVersion: 1, methods: [publicRead({ publicRead: broken })] }).size,
      ).toBe(0);
    }
  });

  it("is never also a credentialed or side-effecting read", () => {
    expect(eligiblePublicReadPolicy(publicRead({ sensitivity: "UNSPECIFIED" }))).toBeNull();
    expect(eligiblePublicReadPolicy(publicRead({ idempotency: "IDEMPOTENT" }))).toBeNull();
    expect(
      eligiblePublicReadPolicy(
        publicRead({ browserCache: { scope: "PRIVATE", maxAgeSeconds: 60 } }),
      ),
    ).toBeNull();
    expect(eligiblePublicReadPolicy(publicRead({ procedure: "streaming" }))).toBeNull();
  });

  it("puts the lane beside the session mount", () => {
    expect(publicLaneUrl("https://app.example.com/itinerwiz")).toBe(
      "https://app.example.com/public/itinerwiz",
    );
    expect(publicLaneUrl("https://app.example.com/itinerwiz/")).toBe(
      "https://app.example.com/public/itinerwiz",
    );
    expect(publicLaneUrl("https://app.example.com")).toBe("https://app.example.com/public");
  });

  it("derives the lane from a same-origin base path too", () => {
    expect(publicLaneUrl("/itinerwiz")).toBe("/public/itinerwiz");
    expect(publicLaneUrl("/itinerwiz/?x=1")).toBe("/public/itinerwiz");
    expect(publicLaneUrl("api")).toBe("/public/api");
    expect(publicLaneUrl("")).toBe("/public");
  });
});

const service = { typeName: "pkg.v1.PublicService" };

function unary(name: string): DescMethodUnary<typeof StringValueSchema, typeof EmptySchema> {
  return {
    kind: "rpc",
    name,
    localName: name.charAt(0).toLowerCase() + name.slice(1),
    parent: service,
    methodKind: "unary",
    input: StringValueSchema,
    output: EmptySchema,
    // What protoc-gen-es emits for `idempotency_level = NO_SIDE_EFFECTS`.
    idempotency: name.startsWith("Get") ? 1 : 0,
    deprecated: false,
  } as unknown as DescMethodUnary<typeof StringValueSchema, typeof EmptySchema>;
}

interface Sent {
  url: string;
  method: string;
  credentials: RequestCredentials | undefined;
  headers: Headers;
}

function recordingFetch(sent: Sent[]): typeof fetch {
  return async (input, init) => {
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    sent.push({
      url,
      method: init?.method ?? "GET",
      credentials: init?.credentials,
      headers: new Headers(init?.headers),
    });
    return new Response(new Uint8Array(), {
      status: 200,
      headers: { "content-type": "application/proto" },
    });
  };
}

describe("webapp transport", () => {
  it("builds with a relative base URL and sends its public reads beside it", async () => {
    const sent: Sent[] = [];
    const transport = configureConnectTransport({
      baseUrl: "/api",
      profile: "webapp",
      policy: { schemaVersion: 1, methods: [publicRead(), ordinary] },
      fetch: recordingFetch(sent),
    });
    await transport.unary(unary("Book"), undefined, undefined, undefined, { value: "x" });
    await transport.unary(unary("GetWeek"), undefined, undefined, undefined, { value: "x" });
    expect(sent[0].url).toBe("/api/pkg.v1.PublicService/Book");
    expect(sent[1].url.startsWith("/public/api/pkg.v1.PublicService/GetWeek?")).toBe(true);
  });

  const policy = { schemaVersion: 1, methods: [publicRead(), ordinary] };

  it("sends a public read to the lane as an anonymous GET", async () => {
    const sent: Sent[] = [];
    const transport = configureConnectTransport({
      baseUrl: "https://app.example.com/itinerwiz",
      profile: "webapp",
      policy,
      getToken: () => "secret-bearer",
      interceptors: [
        (next) => (req) => {
          req.header.set("x-product-trace", "1");
          return next(req);
        },
      ],
      fetch: recordingFetch(sent),
    });

    await transport.unary(unary("GetWeek"), undefined, undefined, { authorization: "x" }, {
      value: "mori-ora",
    });

    expect(sent).toHaveLength(1);
    const [request] = sent;
    expect(request.method).toBe("GET");
    expect(request.url.startsWith("https://app.example.com/public/itinerwiz/pkg.v1.PublicService/GetWeek?")).toBe(true);
    expect(request.credentials).toBe("omit");
    for (const name of ["authorization", "x-csrf-token", "x-product-trace"]) {
      expect(request.headers.get(name), name).toBeNull();
    }
  });

  it("sends any other method to the session mount with the session", async () => {
    const sent: Sent[] = [];
    const transport = configureConnectTransport({
      baseUrl: "https://app.example.com/itinerwiz",
      profile: "webapp",
      policy,
      fetch: recordingFetch(sent),
    });

    await transport.unary(unary("Book"), undefined, undefined, undefined, { value: "x" });

    expect(sent[0].method).toBe("POST");
    expect(sent[0].url).toBe("https://app.example.com/itinerwiz/pkg.v1.PublicService/Book");
    expect(sent[0].credentials).toBe("include");
  });

  it("leaves the service profile alone", async () => {
    const sent: Sent[] = [];
    const transport = configureConnectTransport({
      baseUrl: "https://app.example.com/itinerwiz",
      profile: "service",
      policy,
      fetch: recordingFetch(sent),
    });

    await transport.unary(unary("GetWeek"), undefined, undefined, undefined, { value: "x" });

    expect(sent[0].url.startsWith("https://app.example.com/itinerwiz/")).toBe(true);
  });
});
