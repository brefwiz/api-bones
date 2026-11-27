// SPDX-License-Identifier: MIT

import { Code, ConnectError } from "@connectrpc/connect";

/** A refusal from a platform limiter: resource exhausted, naming in `retry-after` the whole seconds until the server is ready. */
export function throttled(retryAfterSeconds: number): ConnectError {
  if (!Number.isInteger(retryAfterSeconds) || retryAfterSeconds < 0) {
    throw new RangeError(`retryAfterSeconds must be a whole number of seconds, got ${retryAfterSeconds}`);
  }
  return new ConnectError("the server is throttling", Code.ResourceExhausted, { "retry-after": String(retryAfterSeconds) });
}

/** An internal failure, which the SDK does not retry; `count` is how many requests the server answers with it. */
export function failing(count: number): ConnectError {
  if (!Number.isInteger(count) || count < 0) {
    throw new RangeError(`failing must be a whole number of requests, got ${count}`);
  }
  return new ConnectError("the server failed", Code.Internal);
}

/** Refuses an expiry script whose page or count is not a whole number. */
export function expiring(page: number, count: number): void {
  for (const [name, value] of [["page", page], ["count", count]] as const) {
    if (!Number.isInteger(value) || value < 0) throw new RangeError(`expireAtPage ${name} must be a whole number, got ${value}`);
  }
}
