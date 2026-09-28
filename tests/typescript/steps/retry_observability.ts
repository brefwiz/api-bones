// SPDX-License-Identifier: MIT
/**
 * The TypeScript half of `tests/features/connect_retry_observability.feature`,
 * answered through the real `configureConnectTransport` entry point -- the
 * same one a product's generated client is built against -- so the proof
 * covers the `onRetry` hook a caller actually gets, not the interceptor in
 * isolation. There is no Rust half: `onRetry` is an interceptor-pipeline
 * concept, and Rust has no client interceptor pipeline to attach one to.
 */

import assert from "node:assert/strict";

import { Given, Then, When, World } from "@cucumber/cucumber";

import type { RetryEvent } from "@brefwiz/api-bones-connect";
import { configureConnectTransport } from "@brefwiz/api-bones-connect/web";

import { CONNECT_CODE_NAMES, connectCodeOf, unaryMethodFixture, unaryPolicyDoc } from "./support.js";

const RPC_TYPE_NAME = "pkg.v1.RetryObservabilityService";
const RPC_METHOD = "Method";

interface RetryObservabilityWorld extends World {
  failuresBeforeSuccess: number;
  retryable: boolean;
  events: RetryEvent[];
  succeeded: boolean;
}

function method() {
  return unaryMethodFixture(RPC_TYPE_NAME, RPC_METHOD);
}

function policyDoc(retryable: boolean): unknown {
  return unaryPolicyDoc(
    `/${RPC_TYPE_NAME}/${RPC_METHOD}`,
    retryable ? "NO_SIDE_EFFECTS" : "UNSPECIFIED",
  );
}

/** Fails the caller's declared number of times, in network-failure shape, then succeeds. */
function flakyFetch(failuresBeforeSuccess: number): typeof fetch {
  let calls = 0;
  return async () => {
    if (calls++ < failuresBeforeSuccess) {
      throw new Error("simulated network failure");
    }
    return new Response(new Uint8Array(), {
      status: 200,
      headers: { "content-type": "application/proto" },
    });
  };
}

Given(
  "a retryable method that fails twice with code {string} before succeeding",
  function (this: RetryObservabilityWorld, code: string) {
    connectCodeOf(code); // fails fast on a code this surface does not build
    this.failuresBeforeSuccess = 2;
    this.retryable = true;
  },
);

Given(
  "a retryable method that fails once with code {string} before succeeding",
  function (this: RetryObservabilityWorld, code: string) {
    connectCodeOf(code);
    this.failuresBeforeSuccess = 1;
    this.retryable = true;
  },
);

Given("a retryable method that succeeds immediately", function (this: RetryObservabilityWorld) {
  this.failuresBeforeSuccess = 0;
  this.retryable = true;
});

Given(
  "a non-retryable method that fails with code {string}",
  function (this: RetryObservabilityWorld, code: string) {
    // Never succeeds: eligibility is decided before any attempt runs, so the
    // call must fail on the first (and only) try.
    connectCodeOf(code);
    this.failuresBeforeSuccess = Number.POSITIVE_INFINITY;
    this.retryable = false;
  },
);

async function sendCall(world: RetryObservabilityWorld, onRetry: (event: RetryEvent) => void) {
  const transport = configureConnectTransport({
    baseUrl: "https://svc",
    profile: "service",
    policy: policyDoc(world.retryable),
    retry: { initialDelayMs: 0, maxDelayMs: 0 },
    onRetry,
    fetch: flakyFetch(world.failuresBeforeSuccess),
  });
  try {
    await transport.unary(method(), undefined, undefined, undefined, { value: "x" });
    world.succeeded = true;
  } catch {
    world.succeeded = false;
  }
}

When("the call is sent with a retry observer attached", async function (this: RetryObservabilityWorld) {
  this.events = [];
  await sendCall(this, (event) => this.events.push(event));
});

When("the call is sent with an observer that throws", async function (this: RetryObservabilityWorld) {
  this.events = [];
  await sendCall(this, () => {
    throw new Error("observer boom");
  });
});

Then("the call succeeds", function (this: RetryObservabilityWorld) {
  assert.equal(this.succeeded, true);
});

Then("the call fails", function (this: RetryObservabilityWorld) {
  assert.equal(this.succeeded, false);
});

Then(
  "the observer recorded {int} retried attempts",
  function (this: RetryObservabilityWorld, count: number) {
    assert.equal(this.events.length, count);
  },
);

Then(
  "recorded attempt {int} carries code {string}",
  function (this: RetryObservabilityWorld, attempt: number, expectedCode: string) {
    const event = this.events[attempt - 1];
    assert.ok(event, `no event recorded for attempt ${attempt}`);
    assert.equal(event.attempt, attempt);
    assert.equal(CONNECT_CODE_NAMES.get(event.code), expectedCode);
  },
);
