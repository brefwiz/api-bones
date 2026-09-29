// SPDX-License-Identifier: MIT
/**
 * The TypeScript half of `tests/features/connect_undelivered_retry.feature`.
 * There is no Rust half: the retry interceptor is an interceptor-pipeline
 * concept, and Rust has no client interceptor pipeline to attach one to.
 *
 * The failures are Internal write failures, as the Node transport raises them,
 * carrying the not-delivered mark only when the connection proved the request
 * never left. They are answered through the real transport entry point and the
 * package's exported marker, not a copy of either.
 */

import assert from "node:assert/strict";

import { Given, Then, When, World } from "@cucumber/cucumber";

import { Code, ConnectError, markNotDelivered } from "@brefwiz/api-bones-connect";
import { configureConnectTransport } from "@brefwiz/api-bones-connect/web";

import { unaryMethodFixture, unaryPolicyDoc } from "./support.ts";

const RPC_TYPE_NAME = "pkg.v1.UndeliveredRetryService";
const RPC_METHOD = "Method";

interface UndeliveredWorld extends World {
  idempotency: string;
  failures: number;
  undelivered: boolean;
  failureMessage?: string;
  /** What the call settled on: the attempt that answered it, or the error it died with. */
  outcome: { answered: boolean; attempt: number };
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
  "its connection never opened {int} times with {string}",
  function (this: UndeliveredWorld, failures: number, message: string) {
    this.failures = failures;
    this.undelivered = true;
    this.failureMessage = message;
  },
);

Given(
  "the peer refuses its stream {int} times with REFUSED_STREAM",
  function (this: UndeliveredWorld, failures: number) {
    this.failures = failures;
    this.undelivered = true;
    this.failureMessage = "stream refused";
  },
);

Given(
  "the peer shuts its session down {int} times below its stream",
  function (this: UndeliveredWorld, failures: number) {
    this.failures = failures;
    this.undelivered = true;
    this.failureMessage = "session shut down below the stream";
  },
);

Given(
  "its stream fails {int} times after the peer processed it",
  function (this: UndeliveredWorld, failures: number) {
    this.failures = failures;
    this.undelivered = false;
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
  let attempts = 0;
  // A fetch that can prove its connection never carried the request marks the
  // failure it raises; each attempt numbers itself through the header or the
  // error it produces, so the outcome asserted on is what the transport returned.
  const fetchImpl: typeof fetch = async () => {
    attempts += 1;
    if (attempts <= this.failures) {
      const failure = new ConnectError(
        `${this.failureMessage ?? "write EPIPE"} on attempt ${attempts}`,
        this.failureMessage === undefined ? Code.Internal : Code.Unavailable,
      );
      throw this.undelivered ? markNotDelivered(failure) : failure;
    }
    return new Response(new Uint8Array(), {
      status: 200,
      headers: { "content-type": "application/proto", "x-attempt": String(attempts) },
    });
  };
  const transport = configureConnectTransport({
    baseUrl: "https://svc",
    profile: "service",
    policy: unaryPolicyDoc(`/${RPC_TYPE_NAME}/${RPC_METHOD}`, this.idempotency),
    retry: { initialDelayMs: 0, maxDelayMs: 0 },
    fetch: fetchImpl,
  });
  try {
    const result = await transport.unary(
      unaryMethodFixture(RPC_TYPE_NAME, RPC_METHOD),
      undefined,
      undefined,
      undefined,
      { value: "x" },
    );
    this.outcome = { answered: true, attempt: Number(result.header.get("x-attempt")) };
  } catch (err) {
    const message = ConnectError.from(err).rawMessage;
    this.outcome = { answered: false, attempt: Number(message.split(" ").pop()) };
  }
});

Then("the call succeeds on attempt {int}", function (this: UndeliveredWorld, attempt: number) {
  assert.deepEqual(this.outcome, { answered: true, attempt });
});

Then("the call fails on attempt {int}", function (this: UndeliveredWorld, attempt: number) {
  assert.deepEqual(this.outcome, { answered: false, attempt });
});
