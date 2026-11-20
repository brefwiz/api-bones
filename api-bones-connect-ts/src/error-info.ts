// SPDX-License-Identifier: MIT
/**
 * The emitted error code, carried across a Connect call.
 *
 * A refused call reaches the caller as a Connect code (`not_found`,
 * `permission_denied`, ...), which is too coarse to say why the emitter
 * refused. The emitter names the reason with an error code and attaches it to
 * the Connect error as a `bones.v1.ErrorInfo` detail, together with the
 * emitting deployable.
 *
 * This is the TypeScript half of `src/connect/error_info.rs`; both answer
 * `tests/features/connect_error_info.feature`.
 *
 * {@link BonesError.message} ends with the canonical token
 * `[bones-error code=<CODE> emitter=<name>]`, so the code survives
 * any reporter that keeps only text. The values arrive from the remote peer and
 * must not be able to forge or break the surrounding text, so a detail whose
 * code or emitter breaks the token grammar is treated as absent, and at most
 * {@link MAX_ERROR_INFOS} details of an error are examined.
 */

import type { ConnectError } from "@connectrpc/connect";

/** The fully-qualified protobuf name of the detail message. */
export const ERROR_INFO_TYPE = "bones.v1.ErrorInfo";

const TYPE_URL_PREFIX = "type.googleapis.com/";

const CODE_PATTERN = /^[A-Z][A-Z0-9_]{0,127}$/;
const EMITTER_PATTERN = /^[a-z0-9][a-z0-9._-]{0,127}$/;

/** The most `ErrorInfo` details of one error that are examined. */
export const MAX_ERROR_INFOS = 4;

/** The most arguments one error message carries. */
export const MAX_ERROR_ARGS = 32;
/** The longest string argument value, in bytes. */
export const MAX_ERROR_ARG_STRING_BYTES = 1024;

const ARG_KEY_PATTERN = /^[a-z][a-z0-9_]{0,63}$/;
const MIN_TIMESTAMP_SECONDS = -62_135_596_800n;
const MAX_TIMESTAMP_SECONDS = 253_402_300_799n;

/**
 * One typed argument of an error message, the TypeScript shape of
 * `bones.v1.ErrorArg`. The viewer's client formats it in the viewer's locale,
 * so a service carries the value and never an interpolated string.
 *
 * An `int64` is a `bigint`, so a value above 2^53 stays exact.
 */
export type ErrorArg =
  | { readonly kind: "string"; readonly value: string }
  | { readonly kind: "int64"; readonly value: bigint }
  | { readonly kind: "double"; readonly value: number }
  | { readonly kind: "timestamp"; readonly seconds: bigint; readonly nanos: number };

/** Why a set of error arguments is refused. */
export type ErrorArgReason =
  | "too-many"
  | "invalid-key"
  | "string-too-long"
  | "non-finite-double"
  | "invalid-timestamp";

/**
 * Error arguments out of bounds. Arguments are bounded on construction and on
 * decode, and never truncated to fit.
 */
export class ErrorArgError extends Error {
  override name = "ErrorArgError";

  constructor(
    /** The bound that failed. */
    readonly reason: ErrorArgReason,
    /** The argument name, absent for `too-many`. */
    readonly key?: string,
  ) {
    super(key === undefined ? `error arguments: ${reason}` : `error argument ${JSON.stringify(key)}: ${reason}`);
  }
}

/** The reason one argument is out of bounds, if it is. */
function argProblem(key: string, arg: ErrorArg): ErrorArgReason | undefined {
  if (!ARG_KEY_PATTERN.test(key)) return "invalid-key";
  switch (arg.kind) {
    case "string":
      return new TextEncoder().encode(arg.value).length > MAX_ERROR_ARG_STRING_BYTES
        ? "string-too-long"
        : undefined;
    case "double":
      return Number.isFinite(arg.value) ? undefined : "non-finite-double";
    case "timestamp":
      return arg.seconds >= MIN_TIMESTAMP_SECONDS &&
        arg.seconds <= MAX_TIMESTAMP_SECONDS &&
        Number.isInteger(arg.nanos) &&
        arg.nanos >= 0 &&
        arg.nanos < 1_000_000_000
        ? undefined
        : "invalid-timestamp";
    default:
      return undefined;
  }
}

/**
 * Check an argument set: at most {@link MAX_ERROR_ARGS} arguments, names
 * matching `^[a-z][a-z0-9_]{0,63}$`, strings of at most
 * {@link MAX_ERROR_ARG_STRING_BYTES} bytes, finite doubles and in-range
 * timestamps.
 *
 * @throws {@link ErrorArgError} on the first violation.
 */
export function validateErrorArgs(args: ReadonlyMap<string, ErrorArg>): void {
  if (args.size > MAX_ERROR_ARGS) throw new ErrorArgError("too-many");
  for (const [key, arg] of args) {
    const reason = argProblem(key, arg);
    if (reason) throw new ErrorArgError(reason, key);
  }
}

/** The wire shape of `bones.v1.ErrorInfo`. Empty strings mean "not stamped". */
export interface ErrorInfo {
  /** The emitted error code, e.g. `GRANT_MISSING`. */
  code: string;
  /** The emitting deployable. */
  emitter: string;
  /**
   * Typed arguments of the message for `code`, keyed by placeholder name.
   * They never enter the text token. A decoded value always has the map.
   */
  args?: ReadonlyMap<string, ErrorArg>;
}

/** What an SDK error type exposes so a caller never parses text for the code. */
export interface CarriesErrorInfo {
  /** The emitted error code. */
  readonly code: string | undefined;
  /** The emitting deployable. */
  readonly emitter: string | undefined;
  /** The typed arguments of the message for the code, empty when none. */
  readonly args: ReadonlyMap<string, ErrorArg>;
}

function putVarint(out: number[], value: bigint): void {
  let rest = BigInt.asUintN(64, value);
  while (rest > 0x7fn) {
    out.push(Number(rest & 0x7fn) | 0x80);
    rest >>= 7n;
  }
  out.push(Number(rest));
}

function putLen(out: number[], field: number, payload: ArrayLike<number>): void {
  out.push((field << 3) | 2);
  putVarint(out, BigInt(payload.length));
  for (let i = 0; i < payload.length; i++) out.push(payload[i]);
}

function putString(out: number[], field: number, value: string): void {
  if (value !== "") putLen(out, field, new TextEncoder().encode(value));
}

function encodeArg(arg: ErrorArg): number[] {
  const out: number[] = [];
  switch (arg.kind) {
    case "string":
      putLen(out, 1, new TextEncoder().encode(arg.value));
      break;
    case "int64":
      out.push(2 << 3);
      putVarint(out, arg.value);
      break;
    case "double": {
      out.push((3 << 3) | 1);
      const raw = new DataView(new ArrayBuffer(8));
      raw.setFloat64(0, arg.value, true);
      for (let i = 0; i < 8; i++) out.push(raw.getUint8(i));
      break;
    }
    case "timestamp": {
      const timestamp: number[] = [];
      if (arg.seconds !== 0n) {
        timestamp.push(1 << 3);
        putVarint(timestamp, arg.seconds);
      }
      if (arg.nanos !== 0) {
        timestamp.push(2 << 3);
        putVarint(timestamp, BigInt(arg.nanos));
      }
      putLen(out, 4, timestamp);
      break;
    }
  }
  return out;
}

/** Map entries in name order, as `bones.v1.ErrorInfo.args` and `ValidationFailure.args`. */
function putArgs(out: number[], field: number, args: ReadonlyMap<string, ErrorArg>): void {
  for (const key of [...args.keys()].sort()) {
    const entry: number[] = [];
    putLen(entry, 1, new TextEncoder().encode(key));
    putLen(entry, 2, encodeArg(args.get(key) as ErrorArg));
    putLen(out, field, entry);
  }
}

/**
 * Wire bytes of an `ErrorInfo` that carries no arguments. Kept apart from
 * {@link encodeErrorInfo} so a bundle that only stamps a code leaves the
 * argument encoder and its bounds out.
 */
export function encodeErrorCode(code: string, emitter: string): Uint8Array {
  const out: number[] = [];
  putString(out, 1, code);
  putString(out, 2, emitter);
  return Uint8Array.from(out);
}

/**
 * Protobuf wire bytes. Empty fields are omitted, as proto3 specifies.
 *
 * @throws {@link ErrorArgError} when the arguments break the bounds.
 */
export function encodeErrorInfo(info: ErrorInfo): Uint8Array {
  const args = info.args ?? new Map<string, ErrorArg>();
  validateErrorArgs(args);
  const out = Array.from(encodeErrorCode(info.code, info.emitter));
  putArgs(out, 4, args);
  return Uint8Array.from(out);
}

function takeVarint(bytes: Uint8Array, at: number): [number, number] | undefined {
  let value = 0;
  for (let i = 0; i < 5 && at + i < bytes.length; i++) {
    const byte = bytes[at + i];
    value += (byte & 0x7f) * 2 ** (7 * i);
    if ((byte & 0x80) === 0) return [value, at + i + 1];
  }
  return undefined;
}

/** A varint of up to 10 bytes, read as the unsigned 64-bit value. */
function takeVarint64(bytes: Uint8Array, at: number): [bigint, number] | undefined {
  let value = 0n;
  for (let i = 0; i < 10 && at + i < bytes.length; i++) {
    const byte = bytes[at + i];
    value |= BigInt(byte & 0x7f) << BigInt(7 * i);
    if ((byte & 0x80) === 0) return [BigInt.asUintN(64, value), at + i + 1];
  }
  return undefined;
}

type WireValue =
  | { readonly type: "varint"; readonly value: bigint }
  | { readonly type: "fixed64"; readonly value: Uint8Array }
  | { readonly type: "len"; readonly value: Uint8Array }
  | { readonly type: "skipped" };

/** Every field of a message in order, or `undefined` when the bytes are malformed. */
function wireFields(bytes: Uint8Array): Array<[number, WireValue]> | undefined {
  const fields: Array<[number, WireValue]> = [];
  let at = 0;
  while (at < bytes.length) {
    const tag = takeVarint(bytes, at);
    if (!tag) return undefined;
    at = tag[1];
    const field = Math.floor(tag[0] / 8);
    switch (tag[0] % 8) {
      case 0: {
        const varint = takeVarint64(bytes, at);
        if (!varint) return undefined;
        at = varint[1];
        fields.push([field, { type: "varint", value: varint[0] }]);
        break;
      }
      case 1:
        if (at + 8 > bytes.length) return undefined;
        fields.push([field, { type: "fixed64", value: bytes.subarray(at, at + 8) }]);
        at += 8;
        break;
      case 5:
        at += 4;
        if (at > bytes.length) return undefined;
        fields.push([field, { type: "skipped" }]);
        break;
      case 2: {
        const length = takeVarint(bytes, at);
        if (!length) return undefined;
        const start = length[1];
        const end = start + length[0];
        if (end > bytes.length) return undefined;
        fields.push([field, { type: "len", value: bytes.subarray(start, end) }]);
        at = end;
        break;
      }
      default:
        return undefined;
    }
  }
  return fields;
}

function textOf(payload: Uint8Array): string | undefined {
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(payload);
  } catch {
    return undefined;
  }
}

/** One `ErrorArg`: `undefined` is malformed, `null` is a message with no oneof member set. */
function decodeArg(bytes: Uint8Array): ErrorArg | null | undefined {
  const fields = wireFields(bytes);
  if (!fields) return undefined;
  let arg: ErrorArg | null = null;
  for (const [field, wire] of fields) {
    if (field === 1 && wire.type === "len") {
      const value = textOf(wire.value);
      if (value === undefined) return undefined;
      arg = { kind: "string", value };
    } else if (field === 2 && wire.type === "varint") {
      arg = { kind: "int64", value: BigInt.asIntN(64, wire.value) };
    } else if (field === 3 && wire.type === "fixed64") {
      const raw = new DataView(wire.value.buffer, wire.value.byteOffset, 8);
      arg = { kind: "double", value: raw.getFloat64(0, true) };
    } else if (field === 4 && wire.type === "len") {
      const timestamp = wireFields(wire.value);
      if (!timestamp) return undefined;
      let seconds = 0n;
      let nanos = 0;
      for (const [tsField, tsWire] of timestamp) {
        if (tsWire.type !== "varint") continue;
        if (tsField === 1) seconds = BigInt.asIntN(64, tsWire.value);
        else if (tsField === 2) nanos = Number(BigInt.asIntN(32, tsWire.value));
      }
      arg = { kind: "timestamp", seconds, nanos };
    }
  }
  return arg;
}

/**
 * Decode the `args` map entries among `fields`. `undefined` is malformed or
 * out of bounds; an entry with no value is dropped and the last duplicate key
 * wins.
 */
function decodeArgs(fields: Array<[number, WireValue]>, field: number): Map<string, ErrorArg> | undefined {
  const args = new Map<string, ErrorArg>();
  for (const [number, wire] of fields) {
    if (number !== field || wire.type !== "len") continue;
    const entry = wireFields(wire.value);
    if (!entry) return undefined;
    let key = "";
    let arg: ErrorArg | null = null;
    for (const [entryField, entryWire] of entry) {
      if (entryField === 1 && entryWire.type === "len") {
        const text = textOf(entryWire.value);
        if (text === undefined) return undefined;
        key = text;
      } else if (entryField === 2 && entryWire.type === "len") {
        const decoded = decodeArg(entryWire.value);
        if (decoded === undefined) return undefined;
        arg = decoded;
      }
    }
    if (arg) args.set(key, arg);
  }
  try {
    validateErrorArgs(args);
  } catch {
    return undefined;
  }
  return args;
}

/**
 * Decode protobuf wire bytes. Unknown fields are skipped; malformed input, or
 * arguments that break the bounds, yield `undefined`.
 */
export function decodeErrorInfo(bytes: Uint8Array): ErrorInfo | undefined {
  const fields = wireFields(bytes);
  if (!fields) return undefined;
  const info: { code: string; emitter: string } = { code: "", emitter: "" };
  for (const [field, wire] of fields) {
    if ((field !== 1 && field !== 2) || wire.type !== "len") continue;
    const text = textOf(wire.value);
    if (text === undefined) return undefined;
    if (field === 1) info.code = text;
    else info.emitter = text;
  }
  const args = decodeArgs(fields, 4);
  return args ? { ...info, args } : undefined;
}

/** The wire shape of `bones.v1.ValidationFailure`. */
export interface ValidationFailureDetail {
  /** JSON Pointer to the failing field. */
  field: string;
  /** The English message, the fallback when the catalog has no string. */
  message: string;
  /** The machine-readable rule that failed. Empty when it names no rule. */
  rule: string;
  /** Typed arguments of the message for `rule`, keyed by placeholder name. */
  args?: ReadonlyMap<string, ErrorArg>;
}

/**
 * Protobuf wire bytes of a validation failure.
 *
 * @throws {@link ErrorArgError} when the arguments break the bounds.
 */
export function encodeValidationFailure(failure: ValidationFailureDetail): Uint8Array {
  const args = failure.args ?? new Map<string, ErrorArg>();
  validateErrorArgs(args);
  const out: number[] = [];
  putString(out, 1, failure.field);
  putString(out, 2, failure.message);
  putString(out, 3, failure.rule);
  putArgs(out, 4, args);
  return Uint8Array.from(out);
}

/** Decode a validation failure; `undefined` when malformed or the arguments break the bounds. */
export function decodeValidationFailure(bytes: Uint8Array): ValidationFailureDetail | undefined {
  const fields = wireFields(bytes);
  if (!fields) return undefined;
  const failure = { field: "", message: "", rule: "" };
  for (const [field, wire] of fields) {
    if (field < 1 || field > 3 || wire.type !== "len") continue;
    const text = textOf(wire.value);
    if (text === undefined) return undefined;
    if (field === 1) failure.field = text;
    else if (field === 2) failure.message = text;
    else failure.rule = text;
  }
  const args = decodeArgs(fields, 4);
  return args ? { ...failure, args } : undefined;
}

/** Whether the code is grammar-valid and the emitter is grammar-valid or not yet stamped. */
function isWellFormed(info: ErrorInfo): boolean {
  return CODE_PATTERN.test(info.code) && (info.emitter === "" || EMITTER_PATTERN.test(info.emitter));
}

/**
 * The well-formed `bones.v1.ErrorInfo` details of a Connect error, in order.
 *
 * Only the first {@link MAX_ERROR_INFOS} such details are examined, and one
 * whose code or emitter breaks the token grammar is dropped.
 */
export function errorInfosOf(err: ConnectError): ErrorInfo[] {
  const found: ErrorInfo[] = [];
  let examined = 0;
  for (const detail of err.details) {
    const { type, value } = detail as { type?: unknown; value?: unknown };
    if (typeof type !== "string" || !(value instanceof Uint8Array)) continue;
    const name = type.startsWith(TYPE_URL_PREFIX) ? type.slice(TYPE_URL_PREFIX.length) : type;
    if (name !== ERROR_INFO_TYPE) continue;
    if (examined++ >= MAX_ERROR_INFOS) break;
    const info = decodeErrorInfo(value);
    if (info && isWellFormed(info)) found.push(info);
  }
  return found;
}

/** The fully-qualified protobuf name of the validation detail message. */
export const VALIDATION_FAILURE_TYPE = "bones.v1.ValidationFailure";

/**
 * The well-formed `bones.v1.ValidationFailure` details of a Connect error, in
 * detail order, each with its typed arguments.
 *
 * A detail that does not decode, or whose arguments break the bounds, is
 * treated as absent. Nothing is truncated: every violation is returned.
 */
export function violationsOf(err: ConnectError): ValidationFailureDetail[] {
  const found: ValidationFailureDetail[] = [];
  for (const detail of err.details) {
    const { type, value } = detail as { type?: unknown; value?: unknown };
    if (typeof type !== "string" || !(value instanceof Uint8Array)) continue;
    const name = type.startsWith(TYPE_URL_PREFIX) ? type.slice(TYPE_URL_PREFIX.length) : type;
    if (name !== VALIDATION_FAILURE_TYPE) continue;
    const failure = decodeValidationFailure(value);
    if (failure) found.push(failure);
  }
  return found;
}

/** The first well-formed `bones.v1.ErrorInfo` detail of a Connect error, if it has one. */
export function errorInfoOf(err: ConnectError): ErrorInfo | undefined {
  return errorInfosOf(err)[0];
}

/**
 * The canonical text token, or `undefined` when the code or emitter
 * is empty or does not match the token grammar.
 */
export function errorToken(info: ErrorInfo): string | undefined {
  if (
    !CODE_PATTERN.test(info.code) ||
    !EMITTER_PATTERN.test(info.emitter)
  ) {
    return undefined;
  }
  return `[bones-error code=${info.code} emitter=${info.emitter}]`;
}

/** `message` followed by the canonical token, when there is a token to render. */
export function messageWithToken(message: string, info: ErrorInfo | undefined): string {
  const token = info ? errorToken(info) : undefined;
  return token ? `${message} ${token}` : message;
}

/**
 * A Connect failure as an SDK Layer 2 error: `code` and `emitter` are
 * readable, and `message` ends with the canonical token.
 */
export class BonesError extends Error implements CarriesErrorInfo {
  override name = "BonesError";
  /** The emitted error code. */
  readonly code: string | undefined;
  /** The emitting deployable. */
  readonly emitter: string | undefined;
  /** The typed arguments of the message for the code, empty when none. */
  readonly args: ReadonlyMap<string, ErrorArg>;
  /** The Connect failure this wraps. */
  readonly cause: ConnectError;

  constructor(cause: ConnectError) {
    const info = errorInfoOf(cause);
    super(messageWithToken(cause.message, info));
    this.cause = cause;
    this.code = info?.code || undefined;
    this.emitter = info?.emitter || undefined;
    this.args = info?.args ?? new Map();
  }

  /** The SDK error for a Connect failure, the way `ConnectError.from` is for any reason. */
  static from(reason: ConnectError): BonesError {
    return new BonesError(reason);
  }
}
