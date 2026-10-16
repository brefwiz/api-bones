// SPDX-License-Identifier: MIT
/**
 * The TypeScript half of `tests/features/connect_precondition.feature`,
 * answered through the real `configureConnectTransport` entry point — the
 * same one a product's generated client is built against — so the proof
 * covers the transport a caller actually gets, not the interceptor in
 * isolation.
 */

import assert from "node:assert/strict";

import { Given, Then, When, World } from "@cucumber/cucumber";

import { configureConnectTransport } from "@brefwiz/api-bones-connect/web";

import { unaryMethodFixture, unaryPolicyDoc } from "./support.ts";

const RPC_TYPE_NAME = "pkg.v1.PreconditionService";
const RPC_METHOD = "Method";

interface PreconditionWorld extends World {
  policy: unknown;
  callerHeader?: string;
  seenIfMatch: string | null;
}

function method() {
  return unaryMethodFixture(RPC_TYPE_NAME, RPC_METHOD);
}

function policyDoc(idempotency: string): unknown {
  return unaryPolicyDoc(`/${RPC_TYPE_NAME}/${RPC_METHOD}`, idempotency);
}

/** Echoes back whatever if-match header the call actually carried. */
const echoingServer: typeof fetch = async (_input, init) => {
  const received = new Headers(init?.headers);
  return new Response(new Uint8Array(), {
    status: 200,
    headers: { "content-type": "application/proto", "x-seen-if-match": received.get("if-match") ?? "" },
  });
};

Given("a unary method declared {string}", function (this: PreconditionWorld, idempotency: string) {
  this.policy = policyDoc(idempotency);
});

Given(
  "a unary method declared {string} in policy schema {string}",
  function (this: PreconditionWorld, idempotency: string, version: string) {
    assert.match(version, /^\d+$/);
    const schemaVersion = Number(version);
    assert.ok(Number.isSafeInteger(schemaVersion));
    this.policy = { ...(policyDoc(idempotency) as object), schemaVersion };
  },
);

Given("a method with no policy entry at all", function (this: PreconditionWorld) {
  this.policy = { schemaVersion: 1, methods: [] };
});

Given(
  "a call to that method carrying {string} as its own if-match header",
  function (this: PreconditionWorld, value: string) {
    this.callerHeader = value;
  },
);

When("the call is sent", async function (this: PreconditionWorld) {
  const headers = new Headers();
  if (this.callerHeader && this.callerHeader !== "none") {
    headers.set("if-match", this.callerHeader);
  }
  const transport = configureConnectTransport({
    baseUrl: "https://svc",
    profile: "service",
    policy: this.policy,
    fetch: echoingServer,
  });
  const result = await transport.unary(method(), undefined, undefined, headers, { value: "x" });
  this.seenIfMatch = result.header.get("x-seen-if-match");
});

Then(
  "the call carries {string} as its if-match header",
  function (this: PreconditionWorld, expected: string) {
    const actual = this.seenIfMatch && this.seenIfMatch !== "" ? this.seenIfMatch : "none";
    assert.equal(actual, expected);
  },
);
