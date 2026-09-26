// SPDX-License-Identifier: MIT
/**
 * The TypeScript half of `tests/features/connect_precondition.feature`.
 *
 * The Rust half answers the same file; neither keeps its own copy of the rows.
 */

import assert from "node:assert/strict";

import { Given, Then, When, World } from "@cucumber/cucumber";
import type { GeneratedMethodPolicy } from "@brefwiz/api-bones-connect";
import { makePreconditionInterceptor } from "@brefwiz/api-bones-connect";

const RPC = "/pkg.v1.Svc/Method";

const fakeReq = (headers: Headers) => ({
  stream: false as const,
  header: headers,
  method: { name: "Method", parent: { typeName: "pkg.v1.Svc" } },
});

interface PreconditionWorld extends World {
  policyByRpc?: ReadonlyMap<string, GeneratedMethodPolicy>;
  callerHeader?: string;
  sentHeader?: string | null;
}

function unaryPolicy(idempotency: GeneratedMethodPolicy["idempotency"]): GeneratedMethodPolicy {
  return {
    rpc: RPC,
    procedure: "unary",
    idempotency,
    browserCache: { scope: "NO_STORE", maxAgeSeconds: 0 },
    sensitivity: "UNSPECIFIED",
    maxEncodedUrlBytes: 4096,
  };
}

Given("a unary method declared {string}", function (this: PreconditionWorld, idempotency: string) {
  this.policyByRpc = new Map([
    [RPC, unaryPolicy(idempotency as GeneratedMethodPolicy["idempotency"])],
  ]);
});

Given("a method with no policy entry at all", function (this: PreconditionWorld) {
  this.policyByRpc = new Map();
});

Given(
  "a call to that method carrying {string} as its own if-match header",
  function (this: PreconditionWorld, value: string) {
    this.callerHeader = value;
  },
);

When("the call is sent", async function (this: PreconditionWorld) {
  const interceptor = makePreconditionInterceptor({ policyByRpc: this.policyByRpc ?? new Map() });
  const headers = new Headers();
  if (this.callerHeader && this.callerHeader !== "none") {
    headers.set("if-match", this.callerHeader);
  }
  const req = fakeReq(headers);
  const next = async (r: typeof req) => {
    this.sentHeader = r.header.get("if-match");
    return "ok" as never;
  };
  await interceptor(next as never)(req as never);
});

Then("the call carries {string} as its if-match header", function (this: PreconditionWorld, expected: string) {
  const actual = this.sentHeader ?? "none";
  assert.equal(actual, expected);
});
