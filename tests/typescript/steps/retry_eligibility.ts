// SPDX-License-Identifier: MIT
/**
 * The TypeScript half of `tests/features/connect_retry_eligibility.feature`.
 *
 * The Rust half answers the same file; neither keeps its own copy of the rows.
 */

import assert from "node:assert/strict";

import { Given, Then, World } from "@cucumber/cucumber";

import {
  ConnectError,
  connectionFailureAsUnavailable,
  isConnectionWriteFailure,
  isReplayableTransportFailure,
  isUnpromptedRetryable,
} from "@brefwiz/api-bones-connect";

import { CONNECT_CODE_NAMES, connectCodeOf } from "./support.js";

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
    this.failure = new ConnectError(message, connectCodeOf(code));
  },
);

Then("it is a connection write failure: {word}", function (this: RetryWorld, expected: string) {
  const actual = isConnectionWriteFailure(failureOf(this));
  assert.equal(String(actual), expected);
});

Then(
  "it is retryable without server instruction: {word}",
  function (this: RetryWorld, expected: string) {
    const actual = isUnpromptedRetryable(failureOf(this).code);
    assert.equal(String(actual), expected);
  },
);

Then("its shape permits a replay: {word}", function (this: RetryWorld, expected: string) {
  const actual = isReplayableTransportFailure(failureOf(this));
  assert.equal(String(actual), expected);
});

Then("it is reported to the caller as {string}", function (this: RetryWorld, expected: string) {
  const recoded = connectionFailureAsUnavailable(failureOf(this));
  assert.equal(CONNECT_CODE_NAMES.get(recoded.code), expected);
});
