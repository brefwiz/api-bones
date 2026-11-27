// SPDX-License-Identifier: MIT
import { fromBinary } from "@bufbuild/protobuf";
import { timestampDate } from "@bufbuild/protobuf/wkt";
import type { ErrorArg as ErrorArgMessage } from "./generated/bones/v1/errors_pb.js";
import { CallErrorSchema, PageFailure_Kind, type CallError } from "./generated/bones/sdk/v1/boundary_pb.js";
import { BonesError, PageError, type ErrorArg, type PageErrorKind } from "./errors.js";

/** The argument a `bones.v1.ErrorArg` carries; an argument that names no member of its oneof gives `undefined`. */
function reviveArg(arg: ErrorArgMessage): ErrorArg | undefined {
  switch (arg.value.case) {
    case "string":
    case "int64":
    case "double":
      return arg.value.value;
    case "timestamp":
      return timestampDate(arg.value.value);
    default:
      return undefined;
  }
}

const PAGE_KINDS = new Map<PageFailure_Kind, PageErrorKind>([
  [PageFailure_Kind.CURSOR_INVALID, "cursor_invalid"],
  [PageFailure_Kind.CURSOR_EXPIRED, "cursor_expired"],
  [PageFailure_Kind.CURSOR_QUERY_MISMATCH, "cursor_query_mismatch"],
  [PageFailure_Kind.RPC, "rpc"],
]);

/**
 * The error a decoded `bones.sdk.v1.CallError` stands for: a {@link PageError}
 * when it names a page failure, else a {@link BonesError}. An argument that
 * names no member of its oneof is dropped and the error kept; a page failure
 * of an unknown kind is read as a failed call (`rpc`).
 */
export function errorOf(message: CallError): BonesError | PageError {
  const args: Record<string, ErrorArg> = {};
  for (const [name, arg] of Object.entries(message.args)) {
    const value = reviveArg(arg);
    if (value !== undefined) Object.defineProperty(args, name, { value, enumerable: true, writable: true, configurable: true });
  }
  const init = {
    code: message.code,
    emitter: message.emitter,
    platform: message.platform,
    connectCode: message.connectCode,
    message: message.message,
    args,
    ...(message.retryAfter === undefined ? {} : { retryAfterMs: Number(message.retryAfter.seconds) * 1000 + Math.trunc(message.retryAfter.nanos / 1_000_000) }),
  };
  if (message.page === undefined) return new BonesError(init);
  return new PageError(init, {
    kind: PAGE_KINDS.get(message.page.kind) ?? "rpc",
    deliveredPages: message.page.deliveredPages,
    deliveredItems: Number(message.page.deliveredItems),
  });
}

/**
 * The error a caller sees for what a Rust export rejected with. The core
 * rejects with `bones.sdk.v1.CallError` bytes only: those are revived into
 * their error class, and anything else (a host failure, an abort) is given
 * back unchanged.
 */
export function mapCallError(rejection: unknown): unknown {
  return rejection instanceof Uint8Array ? errorOf(fromBinary(CallErrorSchema, rejection)) : rejection;
}
