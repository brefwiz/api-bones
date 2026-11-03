// SPDX-License-Identifier: MIT
//
// The server-stream keepalive.
//
// A proxy cuts an upstream read that stays idle, so a server stream with
// nothing to send is dropped mid-flight. After `STREAM_KEEPALIVE_INTERVAL` of
// silence a server sends a keepalive: an ordinary data envelope whose payload
// is a protobuf message holding only field 19999 as the varint 1. Protobuf
// reserves field numbers 19000 through 19999, so no real message declares the
// field. Decoded as any response type, a keepalive is an empty message whose
// only content is that unknown field.
//
// A server sends one only when the request carried `STREAM_KEEPALIVE_HEADER`
// set to `STREAM_KEEPALIVE_HEADER_VALUE`, and only on a protobuf codec. Both
// transports send the header on every server-stream call and drop keepalives
// before the caller, a rotating stream included, sees them.

import { toBinary, type DescMessage } from "@bufbuild/protobuf";
import type { Interceptor } from "@connectrpc/connect";

/** The request header with which a client declares it drops keepalives. */
export const STREAM_KEEPALIVE_HEADER = "bones-stream-keepalive";

/** The value of `STREAM_KEEPALIVE_HEADER` that declares the capability. */
export const STREAM_KEEPALIVE_HEADER_VALUE = "1";

/** The reserved field number the keepalive payload sets. */
export const STREAM_KEEPALIVE_FIELD_NUMBER = 19_999;

/** How long, in milliseconds, a stream stays silent before the server sends a keepalive. */
export const STREAM_KEEPALIVE_INTERVAL = 15_000;

/** The tag of field 19999 with wire type varint, then the value 1. */
export const STREAM_KEEPALIVE_PAYLOAD: Uint8Array = new Uint8Array([0xf8, 0xe1, 0x09, 0x01]);

/** Whether `bytes` is exactly a keepalive payload. */
export function isKeepalivePayload(bytes: Uint8Array): boolean {
  return (
    bytes.length === STREAM_KEEPALIVE_PAYLOAD.length &&
    STREAM_KEEPALIVE_PAYLOAD.every((byte, at) => bytes[at] === byte)
  );
}

/**
 * Whether a decoded message of type `schema` is a keepalive: field 19999 is
 * among its unknown fields and re-encoding it yields exactly the keepalive
 * payload, so a message with any other content, field 19999 included, is not.
 */
export function isKeepaliveMessage(schema: DescMessage, message: unknown): boolean {
  const unknown = (message as { $unknown?: readonly { no: number }[] } | undefined)?.$unknown;
  if (unknown === undefined || unknown.length !== 1) return false;
  if (unknown[0]?.no !== STREAM_KEEPALIVE_FIELD_NUMBER) return false;
  return isKeepalivePayload(toBinary(schema, message as never));
}

async function* withoutKeepalives<T>(
  schema: DescMessage,
  messages: AsyncIterable<T>,
): AsyncGenerator<T, void, undefined> {
  for await (const message of messages) {
    if (!isKeepaliveMessage(schema, message)) yield message;
  }
}

/**
 * Opt every server-stream call in to keepalives and drop the ones read.
 *
 * Unary calls are untouched. Errors and every other message pass through in
 * order.
 */
export function makeStreamKeepaliveInterceptor(): Interceptor {
  return (next) => async (req) => {
    if (!req.stream) return next(req);
    req.header.set(STREAM_KEEPALIVE_HEADER, STREAM_KEEPALIVE_HEADER_VALUE);
    const res = await next(req);
    if (!res.stream) return res;
    return { ...res, message: withoutKeepalives(req.method.output, res.message) };
  };
}
