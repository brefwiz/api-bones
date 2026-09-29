// SPDX-License-Identifier: MIT
/**
 * The TypeScript half of `tests/features/connect_undelivered_retry.feature`.
 * There is no Rust half: the retry interceptor is an interceptor-pipeline
 * concept, and Rust has no client interceptor pipeline to attach one to.
 *
 * The failures are raised as the Node transport raises them -- an Internal
 * write failure -- carrying the not-delivered mark the transport sets only when
 * the connection proved the request never left. The proof is answered through
 * the package's exported retry interceptor and marker, not a copy of them.
 */

import assert from "node:assert/strict";

import { Given, Then, When, World } from "@cucumber/cucumber";

import {
  Code,
  ConnectError,
  indexGeneratedPolicy,
  makeRetryInterceptor,
  markNotDelivered,
} from "@brefwiz/api-bones-connect";

import { unaryMethodFixture, unaryPolicyDoc } from "./support.ts";

const RPC_TYPE_NAME = "pkg.v1.UndeliveredRetryService";
const RPC_METHOD = "Method";

interface UndeliveredWorld extends World {
  idempotency: string;
  failures: number;
  undelivered: boolean;
  attempts: number;
  succeeded: boolean;
}

Given("a method declared {string}", function (this: UndeliveredWorld, idempotency: string) {
  this.idempotency = idempotency;
});

Given(
  "its connection fails {int} times before the TLS handshake completed",
  function (this: UndeliveredWorld, failures: number) {
    this.failures = failures;
    this.undelivered = true;
  },
);

Given(
  "its connection fails {int} times after the request was sent",
  function (this: UndeliveredWorld, failures: number) {
    this.failures = failures;
    this.undelivered = false;
  },
);

When("the call is sent through the retry interceptor", async function (this: UndeliveredWorld) {
  const policyByRpc = indexGeneratedPolicy(
    unaryPolicyDoc(`/${RPC_TYPE_NAME}/${RPC_METHOD}`, this.idempotency) as never,
  );
  const retry = makeRetryInterceptor({ policyByRpc, sleep: async () => {} });
  this.attempts = 0;
  const next = async () => {
    if (this.attempts++ < this.failures) {
      const failure = new ConnectError("write EPIPE", Code.Internal);
      throw this.undelivered ? markNotDelivered(failure) : failure;
    }
    return "ok" as never;
  };
  const req = { stream: false, method: unaryMethodFixture(RPC_TYPE_NAME, RPC_METHOD) };
  try {
    await retry(next as never)(req as never);
    this.succeeded = true;
  } catch {
    this.succeeded = false;
  }
});

// "the call succeeds" / "the call fails" are shared with the retry-observability
// steps, which read the same `succeeded` flag off the world.
Then("the request was attempted {int} times", function (this: UndeliveredWorld, attempts: number) {
  assert.equal(this.attempts, attempts);
});
