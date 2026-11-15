// SPDX-License-Identifier: MIT
/**
 * The TypeScript half of `tests/features/connect_rate_limit_retry.feature`,
 * answered through the real `configureConnectTransport` entry point with a
 * server that refuses calls as the platform limiter does: HTTP 429 carrying
 * `Retry-After`. There is no Rust half: the retry interceptor is an
 * interceptor-pipeline concept, and Rust has no client interceptor pipeline.
 */

import assert from "node:assert/strict";

import { Given, Then, When, World } from "@cucumber/cucumber";

import { Code, ConnectError, RateLimitedError } from "@brefwiz/api-bones-connect";
import { configureConnectTransport } from "@brefwiz/api-bones-connect/web";

import { unaryMethodFixture, unaryPolicyDoc } from "./support.ts";

const RPC_TYPE_NAME = "pkg.v1.RateLimitRetryService";
const RPC_METHOD = "Method";

interface RateLimitWorld extends World {
  idempotency: string;
  baseUrl?: string;
  refusals: number;
  retryAfter?: string;
  attempts: number;
  served: boolean;
  error?: unknown;
}

Given("a throttled method declared {string}", function (this: RateLimitWorld, idempotency: string) {
  this.idempotency = idempotency;
});

Given("the transport base URL is {string}", function (this: RateLimitWorld, baseUrl: string) {
  this.baseUrl = baseUrl;
});

Given(
  "the server refuses it {int} times with Retry-After {string}",
  function (this: RateLimitWorld, refusals: number, retryAfter: string) {
    this.refusals = refusals;
    this.retryAfter = retryAfter;
  },
);

Given(
  "the server refuses it {int} times without Retry-After",
  function (this: RateLimitWorld, refusals: number) {
    this.refusals = refusals;
    this.retryAfter = undefined;
  },
);

When("the throttled call is sent through the retry interceptor", async function (this: RateLimitWorld) {
  this.attempts = 0;
  const fetchImpl: typeof fetch = async () => {
    this.attempts += 1;
    if (this.attempts <= this.refusals) {
      return new Response(JSON.stringify({ code: "resource_exhausted", message: "slow down" }), {
        status: 429,
        headers: {
          "content-type": "application/json",
          ...(this.retryAfter === undefined ? {} : { "retry-after": this.retryAfter }),
        },
      });
    }
    return new Response(new Uint8Array(), {
      status: 200,
      headers: { "content-type": "application/proto" },
    });
  };
  const transport = configureConnectTransport({
    baseUrl: this.baseUrl ?? "https://svc",
    profile: "service",
    policy: unaryPolicyDoc(`/${RPC_TYPE_NAME}/${RPC_METHOD}`, this.idempotency),
    fetch: fetchImpl,
  });
  try {
    await transport.unary(unaryMethodFixture(RPC_TYPE_NAME, RPC_METHOD), undefined, undefined, undefined, {
      value: "x",
    });
    this.served = true;
  } catch (err) {
    this.served = false;
    this.error = err;
  }
});

Then("the throttled call succeeds on attempt {int}", function (this: RateLimitWorld, attempt: number) {
  assert.equal(this.served, true);
  assert.equal(this.attempts, attempt);
});

Then(
  "the throttled call is rejected as a rate limit after {int} ms on attempt {int}",
  function (this: RateLimitWorld, retryAfterMs: number, attempt: number) {
    assert.equal(this.served, false);
    assert.ok(this.error instanceof RateLimitedError);
    assert.equal(this.error.retryAfterMs, retryAfterMs);
    assert.equal(ConnectError.from(this.error).code, Code.ResourceExhausted);
    assert.equal(this.attempts, attempt);
  },
);

Then(
  "the throttled call is rejected as a plain refusal on attempt {int}",
  function (this: RateLimitWorld, attempt: number) {
    assert.equal(this.served, false);
    assert.ok(!(this.error instanceof RateLimitedError));
    assert.equal(ConnectError.from(this.error).code, Code.ResourceExhausted);
    assert.equal(this.attempts, attempt);
  },
);
