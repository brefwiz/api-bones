// SPDX-License-Identifier: MIT

import { Code, ConnectError } from "@connectrpc/connect";

/** A refusal from a platform limiter: resource exhausted, naming in `retry-after` the whole seconds until the server is ready. */
export function throttled(retryAfterSeconds: number): ConnectError {
  if (!Number.isInteger(retryAfterSeconds) || retryAfterSeconds < 0) {
    throw new RangeError(`retryAfterSeconds must be a whole number of seconds, got ${retryAfterSeconds}`);
  }
  return new ConnectError("the server is throttling", Code.ResourceExhausted, { "retry-after": String(retryAfterSeconds) });
}
