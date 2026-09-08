// SPDX-License-Identifier: MIT
/**
 * The TypeScript half of `tests/features/connect_retry_eligibility.feature`.
 *
 * The Rust half answers the same file. Neither language keeps its own copy of
 * the rows: the signature list is transcribed into each by hand, and a corpus
 * each side wrote for itself would let the two drift apart exactly as the
 * implementations could.
 */

import assert from "node:assert/strict";

import { Given, Then, World } from "@cucumber/cucumber";
import { Code, ConnectError } from "@connectrpc/connect";

import {
  connectionFailureAsUnavailable,
  isConnectionWriteFailure,
  isReplayableTransportFailure,
  isUnpromptedRetryable,
} from "../../src/retry-eligibility.js";

const CODES: Readonly<Record<string, Code>> = {
  internal: Code.Internal,
  unavailable: Code.Unavailable,
  unauthenticated: Code.Unauthenticated,
  aborted: Code.Aborted,
  resource_exhausted: Code.ResourceExhausted,
  permission_denied: Code.PermissionDenied,
};

const NAMES: ReadonlyMap<Code, string> = new Map(
  Object.entries(CODES).map(([name, code]) => [code, name] as const),
);

function codeOf(name: string): Code {
  const code = CODES[name];
  if (code === undefined) {
    throw new Error(`the contract names a code this step cannot build: ${name}`);
  }
  return code;
}

interface RetryWorld extends World {
  failure?: ConnectError;
}

function failureOf(world: RetryWorld): ConnectError {
  if (!world.failure) throw new Error("no failure given");
  return world.failure;
}

Given(
  "a Connect failure with code {string} and message {string}",
  function (this: RetryWorld, code: string, message: string) {
    this.failure = new ConnectError(message, codeOf(code));
  },
);

Then("it is a connection write failure: {word}", function (this: RetryWorld, expected: string) {
  assert.equal(String(isConnectionWriteFailure(failureOf(this))), expected);
});

Then(
  "it is retryable without server instruction: {word}",
  function (this: RetryWorld, expected: string) {
    assert.equal(String(isUnpromptedRetryable(failureOf(this).code)), expected);
  },
);

Then("its shape permits a replay: {word}", function (this: RetryWorld, expected: string) {
  assert.equal(String(isReplayableTransportFailure(failureOf(this))), expected);
});

Then("it is reported to the caller as {string}", function (this: RetryWorld, expected: string) {
  assert.equal(NAMES.get(connectionFailureAsUnavailable(failureOf(this)).code), expected);
});
