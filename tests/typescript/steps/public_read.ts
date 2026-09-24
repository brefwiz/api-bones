// SPDX-License-Identifier: MIT
/**
 * `tests/features/connect_public_read.feature`, answered through the browser
 * entry of `@brefwiz/api-bones-connect`, where the webapp profile lives.
 */

import assert from "node:assert/strict";

import type { DescMethodUnary } from "@bufbuild/protobuf";
import { EmptySchema, StringValueSchema } from "@bufbuild/protobuf/wkt";
import type { ConnectTransportOptions } from "@brefwiz/api-bones-connect/web";
import { Given, Then, When, World } from "@cucumber/cucumber";

import { publicLaneUrl } from "@brefwiz/api-bones-connect";
import { configureConnectTransport } from "@brefwiz/api-bones-connect/web";

interface LaneWorld extends World {
  base: string;
  options: Omit<ConnectTransportOptions, "fetch">;
  /** What the server saw of the call, as it echoed it back. */
  seen: Headers;
}

const tenantField = { name: "org_handle", jsonName: "orgHandle", number: 1 };

function policy(publicRead: Record<string, unknown>): unknown {
  return {
    schemaVersion: 1,
    methods: [
      {
        rpc: "/pkg.v1.PublicService/GetWeek",
        procedure: "unary",
        idempotency: "NO_SIDE_EFFECTS",
        browserCache: { scope: "NO_STORE", maxAgeSeconds: 0 },
        sensitivity: "NON_SENSITIVE",
        maxEncodedUrlBytes: 4096,
        publicRead,
      },
      {
        rpc: "/pkg.v1.PublicService/Book",
        procedure: "unary",
        idempotency: "UNSPECIFIED",
        browserCache: { scope: "NO_STORE", maxAgeSeconds: 0 },
        sensitivity: "UNSPECIFIED",
        maxEncodedUrlBytes: 4096,
      },
    ],
  };
}

function unary(name: string, sideEffectFree: boolean): DescMethodUnary<typeof StringValueSchema, typeof EmptySchema> {
  return {
    kind: "rpc",
    name,
    localName: name.charAt(0).toLowerCase() + name.slice(1),
    parent: { typeName: "pkg.v1.PublicService" },
    methodKind: "unary",
    input: StringValueSchema,
    output: EmptySchema,
    // What protoc-gen-es emits for `idempotency_level = NO_SIDE_EFFECTS`.
    idempotency: sideEffectFree ? 1 : 0,
    deprecated: false,
  } as unknown as DescMethodUnary<typeof StringValueSchema, typeof EmptySchema>;
}

function optionsFor(base: string, publicRead: Record<string, unknown>): LaneWorld["options"] {
  return {
    baseUrl: base,
    profile: "webapp",
    policy: policy(publicRead),
    getToken: () => "session-bearer",
    interceptors: [
      (next) => (req) => {
        req.header.set("x-product-trace", "1");
        return next(req);
      },
    ],
  };
}

/**
 * A server that answers an empty message and echoes back what it received --
 * where, how, with which credentials mode and which headers -- so the call's
 * own result carries the evidence.
 */
const echoingServer: typeof fetch = async (input, init) => {
  const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
  const received = new Headers(init?.headers);
  return new Response(new Uint8Array(), {
    status: 200,
    headers: {
      "content-type": "application/proto",
      "x-seen-url": url,
      "x-seen-method": init?.method ?? "GET",
      "x-seen-credentials": init?.credentials ?? "",
      "x-seen-headers": [...received.keys()].join(","),
    },
  });
};

Given("a product served at {string}", function (this: LaneWorld, base: string) {
  this.base = base;
});

Then("its public lane is {string}", function (this: LaneWorld, lane: string) {
  assert.equal(publicLaneUrl(this.base), lane);
});

Given(
  "a webapp transport for {string} holding a session and a bearer",
  function (this: LaneWorld, base: string) {
    this.options = optionsFor(base, { maxAgeSeconds: 60, origins: "OWNER_CONFIRMED", tenantField });
  },
);

Given(
  "a webapp transport for {string} whose public read declares no tenant",
  function (this: LaneWorld, base: string) {
    this.options = optionsFor(base, { maxAgeSeconds: 60, origins: "OWNER_CONFIRMED" });
  },
);

When("it calls the public read {string}", async function (this: LaneWorld, name: string) {
  const result = await configureConnectTransport({ ...this.options, fetch: echoingServer }).unary(
    unary(name, true),
    undefined,
    undefined,
    undefined,
    { value: "mori-ora" },
  );
  this.seen = result.header;
});

When("it calls the ordinary method {string}", async function (this: LaneWorld, name: string) {
  const result = await configureConnectTransport({ ...this.options, fetch: echoingServer }).unary(
    unary(name, false),
    undefined,
    undefined,
    undefined,
    { value: "x" },
  );
  this.seen = result.header;
});

Then("the request is a GET to {string}", function (this: LaneWorld, url: string) {
  assert.equal(this.seen.get("x-seen-method"), "GET");
  assert.ok(this.seen.get("x-seen-url")?.startsWith(`${url}?`), this.seen.get("x-seen-url") ?? "");
});

Then("the request is a POST to {string}", function (this: LaneWorld, url: string) {
  assert.equal(this.seen.get("x-seen-method"), "POST");
  assert.equal(this.seen.get("x-seen-url"), url);
});

Then("the request sends no credentials", function (this: LaneWorld) {
  assert.equal(this.seen.get("x-seen-credentials"), "omit");
});

Then("the request sends the session", function (this: LaneWorld) {
  assert.equal(this.seen.get("x-seen-credentials"), "include");
});

Then(
  "the request carries no {string}, {string} or product header",
  function (this: LaneWorld, first: string, second: string) {
    const received = (this.seen.get("x-seen-headers") ?? "").split(",");
    for (const name of [first, second, "x-product-trace"]) {
      assert.ok(!received.includes(name), `${name} was sent`);
    }
  },
);
