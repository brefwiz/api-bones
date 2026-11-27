// SPDX-License-Identifier: MIT
/**
 * The TypeScript half of `tests/features/connect_rate_limit_retry.feature`,
 * answered through the real `configureConnectTransport` entry point with a
 * server that refuses calls as the platform limiter does: HTTP 429 carrying
 * `Retry-After`. The Rust lane answers the same file over an in-process
 * connectrpc server.
 */

import assert from "node:assert/strict";

import { Given, Then, When, World } from "@cucumber/cucumber";

import { Code, ConnectError, RateLimitedError } from "@brefwiz/api-bones-connect";
import { configureConnectTransport } from "@brefwiz/api-bones-connect/web";

import { unaryMethodFixture, unaryPolicyDoc } from "./support.ts";

const RPC_TYPE_NAME = "pkg.v1.RateLimitRetryService";
const RPC_METHOD = "Method";

/** How long a step waits for the first refusal to arrive before it gives up. */
const FIRST_ATTEMPT_TIMEOUT_MS = 5000;
/** The pause after the first refusal that puts the call inside its wait. */
const INTO_THE_WAIT_MS = 100;

interface RateLimitWorld extends World {
  idempotency: string;
  baseUrl?: string;
  deadlineMs?: number;
  refusals: number;
  retryAfter?: string;
  retryAfterInTrailers?: boolean;
  arrivals: number[];
  served: boolean;
  elapsedMs: number;
  error?: unknown;
}

Given("a throttled method declared {string}", function (this: RateLimitWorld, idempotency: string) {
  this.idempotency = idempotency;
});

Given("the transport base URL is {string}", function (this: RateLimitWorld, baseUrl: string) {
  this.baseUrl = baseUrl;
});

Given("the throttled call has a deadline of {int} ms", function (this: RateLimitWorld, deadlineMs: number) {
  this.deadlineMs = deadlineMs;
});

Given(
  "the server refuses it {int} times with Retry-After {string}",
  function (this: RateLimitWorld, refusals: number, retryAfter: string) {
    this.refusals = refusals;
    this.retryAfter = retryAfter;
  },
);

Given(
  "the server refuses it {int} times with a Retry-After date {int} seconds ahead",
  function (this: RateLimitWorld, refusals: number, ahead: number) {
    this.refusals = refusals;
    this.retryAfter = new Date(Date.now() + ahead * 1000).toUTCString();
  },
);

Given(
  "the server refuses it {int} times with Retry-After {string} in the trailers",
  function (this: RateLimitWorld, refusals: number, retryAfter: string) {
    this.refusals = refusals;
    this.retryAfter = retryAfter;
    this.retryAfterInTrailers = true;
  },
);

Given(
  "the server refuses it {int} times without Retry-After",
  function (this: RateLimitWorld, refusals: number) {
    this.refusals = refusals;
    this.retryAfter = undefined;
  },
);

/** Sends the one throttled call and records how it ended and how long it took. */
async function sendThrottledCall(world: RateLimitWorld, signal?: AbortSignal): Promise<void> {
  world.arrivals = [];
  const fetchImpl: typeof fetch = async () => {
    world.arrivals.push(Date.now());
    if (world.arrivals.length <= world.refusals) {
      return new Response(JSON.stringify({ code: "resource_exhausted", message: "slow down" }), {
        status: 429,
        headers: {
          "content-type": "application/json",
          ...(world.retryAfter === undefined
            ? {}
            : { [world.retryAfterInTrailers ? "trailer-retry-after" : "retry-after"]: world.retryAfter }),
        },
      });
    }
    return new Response(new Uint8Array(), {
      status: 200,
      headers: { "content-type": "application/proto" },
    });
  };
  const transport = configureConnectTransport({
    baseUrl: world.baseUrl ?? "https://svc",
    profile: "service",
    policy: unaryPolicyDoc(`/${RPC_TYPE_NAME}/${RPC_METHOD}`, world.idempotency),
    fetch: fetchImpl,
  });
  const started = Date.now();
  try {
    await transport.unary(
      unaryMethodFixture(RPC_TYPE_NAME, RPC_METHOD),
      signal,
      world.deadlineMs,
      undefined,
      { value: "x" },
    );
    world.served = true;
  } catch (err) {
    world.served = false;
    world.error = err;
  } finally {
    world.elapsedMs = Date.now() - started;
  }
}

When("the throttled call is sent", async function (this: RateLimitWorld) {
  await sendThrottledCall(this);
});

When("the throttled call is sent and abandoned during the wait", async function (this: RateLimitWorld) {
  const controller = new AbortController();
  const settled = sendThrottledCall(this, controller.signal);
  const giveUpAt = Date.now() + FIRST_ATTEMPT_TIMEOUT_MS;
  while ((this.arrivals?.length ?? 0) < 1) {
    assert.ok(Date.now() < giveUpAt, "the server never saw the first attempt");
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  await new Promise((resolve) => setTimeout(resolve, INTO_THE_WAIT_MS));
  controller.abort();
  await settled;
});

/** The wait the server asked for, in milliseconds. */
function retryAfterMs(world: RateLimitWorld): number {
  assert.notEqual(world.retryAfter, undefined, "the scenario names no Retry-After");
  return Number(world.retryAfter) * 1000;
}

Then("the throttled call succeeds on attempt {int}", function (this: RateLimitWorld, attempt: number) {
  assert.equal(this.served, true);
  assert.equal(this.arrivals.length, attempt);
});

Then(
  "the replay arrived at least {int} ms after the refusal",
  function (this: RateLimitWorld, minimumMs: number) {
    assert.ok(this.arrivals.length >= 2, "the call was never replayed");
    const gap = this.arrivals[1] - this.arrivals[0];
    assert.ok(gap >= minimumMs, `the replay came ${gap} ms after the refusal, expected at least ${minimumMs}`);
  },
);

Then(
  "the throttled call is rejected as a rate limit after {int} ms on attempt {int}",
  function (this: RateLimitWorld, waitMs: number, attempt: number) {
    assert.equal(this.served, false);
    assert.ok(this.error instanceof RateLimitedError);
    assert.equal(this.error.retryAfterMs, waitMs);
    assert.equal(ConnectError.from(this.error).code, Code.ResourceExhausted);
    assert.equal(this.arrivals.length, attempt);
  },
);

Then(
  "the throttled call is rejected as a rate limit naming a wait between {int} and {int} ms on attempt {int}",
  function (this: RateLimitWorld, leastMs: number, mostMs: number, attempt: number) {
    assert.equal(this.served, false);
    assert.ok(this.error instanceof RateLimitedError);
    assert.ok(
      this.error.retryAfterMs >= leastMs && this.error.retryAfterMs <= mostMs,
      `the wait of ${this.error.retryAfterMs} ms is outside ${leastMs}..${mostMs} ms`,
    );
    assert.equal(ConnectError.from(this.error).code, Code.ResourceExhausted);
    assert.equal(this.arrivals.length, attempt);
  },
);

Then(
  "the throttled call is rejected as a plain refusal on attempt {int}",
  function (this: RateLimitWorld, attempt: number) {
    assert.equal(this.served, false);
    assert.ok(!(this.error instanceof RateLimitedError));
    assert.equal(ConnectError.from(this.error).code, Code.ResourceExhausted);
    assert.equal(this.arrivals.length, attempt);
  },
);

Then(
  "the throttled call ends as deadline exceeded before its Retry-After on attempt {int}",
  function (this: RateLimitWorld, attempt: number) {
    assert.equal(this.served, false);
    assert.equal(ConnectError.from(this.error).code, Code.DeadlineExceeded);
    assert.equal(this.arrivals.length, attempt);
    assert.ok(
      this.elapsedMs < retryAfterMs(this),
      `the call ran ${this.elapsedMs} ms, which waited the Retry-After out`,
    );
  },
);

Then(
  "the throttled call ends as canceled before its Retry-After on attempt {int}",
  function (this: RateLimitWorld, attempt: number) {
    assert.equal(this.served, false);
    assert.equal(ConnectError.from(this.error).code, Code.Canceled);
    assert.equal(this.arrivals.length, attempt);
    assert.ok(
      this.elapsedMs < retryAfterMs(this),
      `the call ran ${this.elapsedMs} ms, which waited the Retry-After out`,
    );
  },
);
