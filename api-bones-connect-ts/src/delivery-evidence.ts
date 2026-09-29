// SPDX-License-Identifier: MIT
/**
 * Structured evidence that a failed request never left the client.
 *
 * A failure's message says what broke; it does not say whether any byte of the
 * request reached the peer, and that is the only fact that makes a replay safe
 * for a method whose idempotency nobody declared. The transport that owns
 * the connection knows it — the handshake never finished, or no request byte
 * left — and records it here by marking the error it raises.
 * The retry interceptor reads the mark; it never re-derives it from a message.
 *
 * Marks live in a WeakSet rather than on the error, so they survive neither
 * serialization nor a copy: a failure re-wrapped further up the stack has to be
 * re-proven, never inherited.
 */

import type { ConnectError } from "@connectrpc/connect";

const NOT_DELIVERED = new WeakSet<ConnectError>();

/** Record that the request behind `err` provably never reached the server. */
export function markNotDelivered<E extends ConnectError>(err: E): E {
  NOT_DELIVERED.add(err);
  return err;
}

/**
 * Whether the request behind `err` provably never reached the server.
 *
 * Such a failure is replay-safe whatever the method's declared idempotency,
 * because no server ever saw the call.
 */
export function isNotDelivered(err: ConnectError): boolean {
  return NOT_DELIVERED.has(err);
}
