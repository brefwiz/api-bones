// SPDX-License-Identifier: MIT

import assert from "node:assert/strict";
import { test } from "node:test";

import { Code } from "@connectrpc/connect";

import { throttled } from "./throttle.js";

test("a throttle is resource exhausted and names its retry-after in seconds", () => {
  const refusal = throttled(3);
  assert.equal(refusal.code, Code.ResourceExhausted);
  assert.equal(refusal.metadata.get("retry-after"), "3");
});

test("a retry-after of zero is a throttle that is ready at once", () => {
  assert.equal(throttled(0).metadata.get("retry-after"), "0");
});

test("a retry-after that is not a whole number of seconds is refused", () => {
  for (const seconds of [-1, 1.5, Number.NaN, Number.POSITIVE_INFINITY]) {
    assert.throws(() => throttled(seconds), RangeError, String(seconds));
  }
});
