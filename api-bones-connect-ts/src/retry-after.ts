// SPDX-License-Identifier: MIT
/**
 * The wait a refusal asks of its caller.
 *
 * A service that refuses a call because the caller must slow down says how long
 * to wait, and a client reads it back. This is the TypeScript half of
 * `src/connect/retry_after.rs`; both answer
 * `tests/features/connect_retry_after.feature`.
 *
 * - {@link retryAfterSeconds} is the one rounding rule: whole seconds, rounded
 *   up, never less than one.
 * - {@link withRetryAfter} stamps a refusal with the wait three ways: the
 *   `Retry-After` header, a `google.rpc.RetryInfo` detail and
 *   `Cache-Control: no-store`.
 * - {@link retryAfter} reads the wait: the detail first, the header when
 *   there is no detail.
 *
 * `google.rpc.RetryInfo` is hand-encoded, as `bones.v1.ErrorInfo` is. Its
 * `retry_delay` is a `google.protobuf.Duration`; the platform writes whole
 * `seconds` and reads `seconds` and `nanos`.
 */

import { Code, ConnectError } from "@connectrpc/connect";

import { putLen, putVarint, wireFields } from "./error-info.js";

/** The fully-qualified protobuf name of the detail message. */
export const RETRY_INFO_TYPE = "google.rpc.RetryInfo";

const TYPE_URL_PREFIX = "type.googleapis.com/";

/** The most `RetryInfo` details of one error that are examined. */
const MAX_RETRY_INFOS = 4;

/** The largest whole-second count a `Retry-After` header carries (unsigned 64-bit). */
const MAX_SECONDS = 18_446_744_073_709_551_615n;

/**
 * The whole seconds a wait of `waitMs` milliseconds is sent as: rounded up, and
 * never less than one. A wait that is not a number is one second.
 */
export function retryAfterSeconds(waitMs: number): number {
  if (Number.isNaN(waitMs)) return 1;
  return Math.max(1, Math.ceil(Math.min(waitMs, Number.MAX_SAFE_INTEGER) / 1000));
}

/**
 * The protobuf wire bytes of a `google.rpc.RetryInfo` asking for `waitMs`:
 * `retry_delay` (1) holding a `Duration` whose `seconds` (1) is
 * {@link retryAfterSeconds} of the wait.
 */
export function encodeRetryInfo(waitMs: number): Uint8Array {
  const duration: number[] = [1 << 3];
  putVarint(duration, BigInt(retryAfterSeconds(waitMs)));
  const out: number[] = [];
  putLen(out, 1, duration);
  return Uint8Array.from(out);
}

function decodeDuration(bytes: Uint8Array): number | undefined {
  const fields = wireFields(bytes);
  if (!fields) return undefined;
  let seconds = 0n;
  let nanos = 0n;
  for (const [field, wire] of fields) {
    if (wire.type !== "varint") continue;
    if (field === 1) {
      seconds = BigInt.asIntN(64, wire.value);
      if (seconds < 0n) return undefined;
    } else if (field === 2) {
      nanos = BigInt.asIntN(64, wire.value);
      if (nanos < 0n || nanos >= 1_000_000_000n) return undefined;
    }
  }
  return Number(seconds) * 1000 + Number(nanos) / 1_000_000;
}

/**
 * The wait, in milliseconds, a `google.rpc.RetryInfo` names. Unknown fields are
 * skipped. Malformed input, a negative wait, and a message that names no
 * `retry_delay` yield `undefined`.
 */
export function decodeRetryInfo(bytes: Uint8Array): number | undefined {
  const fields = wireFields(bytes);
  if (!fields) return undefined;
  let waitMs: number | undefined;
  for (const [field, wire] of fields) {
    if (field !== 1 || wire.type !== "len") continue;
    waitMs = decodeDuration(wire.value);
    if (waitMs === undefined) return undefined;
  }
  return waitMs;
}

function isRetryInfo(detail: unknown): boolean {
  const { type } = detail as { type?: unknown };
  if (typeof type !== "string") return false;
  return (type.startsWith(TYPE_URL_PREFIX) ? type.slice(TYPE_URL_PREFIX.length) : type) === RETRY_INFO_TYPE;
}

/**
 * A copy of `err` stamped with the wait it asks of its caller.
 *
 * Sets `Retry-After` to {@link retryAfterSeconds} of `waitMs`, `Cache-Control`
 * to `no-store`, and attaches a `google.rpc.RetryInfo` detail, replacing any
 * already there. The code, the message and every other header and detail of
 * `err` are kept.
 */
export function withRetryAfter(err: ConnectError, waitMs: number): ConnectError {
  const stamped = new ConnectError(err.rawMessage, err.code, new Headers(err.metadata), undefined, err.cause);
  stamped.metadata.set("retry-after", String(retryAfterSeconds(waitMs)));
  stamped.metadata.set("cache-control", "no-store");
  stamped.details = [
    ...err.details.filter((detail) => !isRetryInfo(detail)),
    { type: RETRY_INFO_TYPE, value: encodeRetryInfo(waitMs) },
  ];
  return stamped;
}

/**
 * A copy of `err` stamped with the wait it asks of its caller, under the
 * `resource_exhausted` code with free-text `message`. See {@link withRetryAfter}.
 */
export function resourceExhaustedRetryAfter(message: string, waitMs: number): ConnectError {
  return withRetryAfter(new ConnectError(message, Code.ResourceExhausted), waitMs);
}

function retryInfoWaitMs(err: ConnectError): number | undefined {
  let examined = 0;
  for (const detail of err.details) {
    if (!isRetryInfo(detail)) continue;
    if (examined++ >= MAX_RETRY_INFOS) break;
    const { value } = detail as { value?: unknown };
    if (!(value instanceof Uint8Array)) continue;
    const waitMs = decodeRetryInfo(value);
    if (waitMs !== undefined) return waitMs;
  }
  return undefined;
}

function headerWaitMs(err: ConnectError): number | undefined {
  const value = err.metadata.get("retry-after")?.trim();
  if (value === undefined || !/^[0-9]+$/.test(value) || BigInt(value) > MAX_SECONDS) return undefined;
  return Number(value) * 1000;
}

/**
 * The wait, in milliseconds, a refusal asks of its caller, or `undefined` when
 * it names none.
 *
 * The `google.rpc.RetryInfo` detail wins; with none, the `Retry-After` header.
 * The header counts only as whole non-negative seconds: an HTTP-date, a
 * negative or fractional number, and an empty value read as no wait.
 *
 * The value is the server's claim and is uncapped: callers must cap it before
 * sleeping.
 */
export function retryAfter(err: ConnectError): number | undefined {
  return retryInfoWaitMs(err) ?? headerWaitMs(err);
}
