// SPDX-License-Identifier: MIT
/**
 * Which transport failures may be replayed, and how they are coded.
 *
 * The TypeScript half of a vocabulary shipped in two languages; `src/connect/
 * retry_eligibility.rs` is the other. Both answer the same corpus, at
 * `test-fixtures/parity/connect-retry-eligibility.json` — the signature list
 * below is transcribed into each language by hand, and for a long time the two
 * agreed only because two authors typed the same seven strings. The corpus is
 * what turns that into a contract.
 *
 * This module holds only the vocabulary. The interceptor pipeline that
 * consumes it lives in `retry.ts` and has no Rust counterpart: a Rust caller
 * owns its own retry decision and calls these predicates directly.
 */

import { Code, ConnectError } from "@connectrpc/connect";

/**
 * Codes retried without any server instruction.
 *
 * Only Unavailable: it is the one code that reliably means "this connection did
 * not carry the call". Aborted is deliberately absent — it signals a concurrency
 * or transaction conflict, and the correct response is to re-run the enclosing
 * transaction, not to replay one RPC inside it. ResourceExhausted is absent too;
 * it is handled under explicit server pushback only, because retrying a quota
 * rejection on our own schedule is how a degraded service is converted into a
 * fully saturated one.
 */
const UNPROMPTED_RETRYABLE_CODES: ReadonlySet<Code> = new Set([Code.Unavailable]);

/**
 * Failures raised while writing the request onto the connection.
 *
 * A keep-alive pool hands back a socket the peer has already closed, and the
 * write fails before any byte of the request is delivered. Connect surfaces
 * that as Internal — the code it uses for "the transport itself broke" — so a
 * code check alone never sees it and the call fails outright, even though
 * nothing ran on the server.
 */
const CONNECTION_WRITE_FAILURE_SIGNATURES: readonly string[] = [
  "write epipe",
  "broken pipe",
  "econnreset",
  "connection reset",
  "socket hang up",
  "http2 stream closed",
  "received goaway",
];

/** Whether a code may be retried with no instruction from the server. */
export function isUnpromptedRetryable(code: Code): boolean {
  return UNPROMPTED_RETRYABLE_CODES.has(code);
}

/**
 * Whether a failure happened before the request reached the server.
 *
 * Matched on the message because neither adapter preserves the underlying
 * `code` property once the reason is wrapped into a ConnectError.
 */
export function isConnectionWriteFailure(err: ConnectError): boolean {
  if (err.code !== Code.Internal) return false;
  const message = err.rawMessage.toLowerCase();
  return CONNECTION_WRITE_FAILURE_SIGNATURES.some((signature) => message.includes(signature));
}

/**
 * Whether a failure's *shape* permits a replay.
 *
 * A socket that rejected the write demonstrably carried nothing, which is the
 * safest replay there is; Unavailable can reach the client after the server
 * accepted the call, which is why eligibility is still read from the method's
 * declared idempotency and never from this alone.
 */
export function isReplayableTransportFailure(err: ConnectError): boolean {
  return isUnpromptedRetryable(err.code) || isConnectionWriteFailure(err);
}

/**
 * Re-code a failure that never reached the server as `Unavailable`.
 *
 * Internal asserts the server has a bug; a socket that rejected the write
 * asserts the opposite, that nothing server-side formed an opinion. The
 * difference is what a caller acts on — stop and investigate, versus the hop
 * failed and may be retried — and it is what a caller asserting on a refusal
 * sees in place of the refusal.
 *
 * Applied at the boundary where the error leaves the transport, AFTER any
 * retry decision: `isConnectionWriteFailure` recognises these by their Internal
 * code, so re-coding earlier would make every one of them look unretryable.
 * Anything else is returned untouched.
 */
export function connectionFailureAsUnavailable(err: ConnectError): ConnectError {
  if (!isConnectionWriteFailure(err)) return err;
  return new ConnectError(
    err.rawMessage,
    Code.Unavailable,
    err.metadata,
    undefined,
    err.cause,
  );
}
