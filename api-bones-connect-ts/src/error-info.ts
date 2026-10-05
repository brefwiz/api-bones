// SPDX-License-Identifier: MIT
/**
 * The emitted error code, carried across a Connect call.
 *
 * A refused call reaches the caller as a Connect code (`not_found`,
 * `permission_denied`, ...), which is too coarse to say why the emitter
 * refused. The emitter names the reason with an error code and attaches it to
 * the Connect error as a `bones.v1.ErrorInfo` detail, together with the
 * emitting deployable and its build version.
 *
 * This is the TypeScript half of `src/connect/error_info.rs`; both answer
 * `tests/features/connect_error_info.feature`.
 *
 * {@link BonesError.message} ends with the canonical token
 * `[bones-error code=<CODE> emitter=<name>@<version>]`, so the code survives
 * any reporter that keeps only text. The token is rendered only for values
 * that match its grammar: the values arrive from the remote peer and must not
 * be able to forge or break the surrounding text.
 */

import type { ConnectError } from "@connectrpc/connect";

/** The fully-qualified protobuf name of the detail message. */
export const ERROR_INFO_TYPE = "bones.v1.ErrorInfo";

const TYPE_URL_PREFIX = "type.googleapis.com/";

const CODE_PATTERN = /^[A-Z][A-Z0-9_]{0,127}$/;
const EMITTER_PATTERN = /^[a-z0-9][a-z0-9._-]{0,127}$/;
const VERSION_PATTERN = /^[0-9A-Za-z.+-]{1,64}$/;

/** The wire shape of `bones.v1.ErrorInfo`. Empty strings mean "not stamped". */
export interface ErrorInfo {
  /** The emitted error code, e.g. `GRANT_MISSING`. */
  code: string;
  /** The emitting deployable. */
  emitter: string;
  /** The emitting deployable's build version. */
  emitterVersion: string;
}

/** What an SDK error type exposes so a caller never parses text for the code. */
export interface CarriesErrorInfo {
  /** The emitted error code. */
  readonly code: string | undefined;
  /** The emitting deployable. */
  readonly emitter: string | undefined;
}

function putString(out: number[], field: number, value: string): void {
  if (value === "") return;
  const bytes = new TextEncoder().encode(value);
  out.push((field << 3) | 2);
  let length = bytes.length;
  while (length > 0x7f) {
    out.push((length & 0x7f) | 0x80);
    length >>>= 7;
  }
  out.push(length);
  for (const byte of bytes) out.push(byte);
}

/** Protobuf wire bytes. Empty fields are omitted, as proto3 specifies. */
export function encodeErrorInfo(info: ErrorInfo): Uint8Array {
  const out: number[] = [];
  putString(out, 1, info.code);
  putString(out, 2, info.emitter);
  putString(out, 3, info.emitterVersion);
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

/**
 * Decode protobuf wire bytes. Unknown fields are skipped; malformed input
 * yields `undefined`.
 */
export function decodeErrorInfo(bytes: Uint8Array): ErrorInfo | undefined {
  const info: ErrorInfo = { code: "", emitter: "", emitterVersion: "" };
  const decoder = new TextDecoder("utf-8", { fatal: true });
  let at = 0;
  while (at < bytes.length) {
    const tag = takeVarint(bytes, at);
    if (!tag) return undefined;
    at = tag[1];
    const field = Math.floor(tag[0] / 8);
    switch (tag[0] % 8) {
      case 0: {
        const skipped = takeVarint(bytes, at);
        if (!skipped) return undefined;
        at = skipped[1];
        break;
      }
      case 1:
        at += 8;
        break;
      case 5:
        at += 4;
        break;
      case 2: {
        const length = takeVarint(bytes, at);
        if (!length) return undefined;
        const start = length[1];
        const end = start + length[0];
        if (end > bytes.length) return undefined;
        at = end;
        if (field >= 1 && field <= 3) {
          let text: string;
          try {
            text = decoder.decode(bytes.subarray(start, end));
          } catch {
            return undefined;
          }
          if (field === 1) info.code = text;
          else if (field === 2) info.emitter = text;
          else info.emitterVersion = text;
        }
        break;
      }
      default:
        return undefined;
    }
    if (at > bytes.length) return undefined;
  }
  return info;
}

/** Decode the `bones.v1.ErrorInfo` detail of a Connect error, if it has one. */
export function errorInfoOf(err: ConnectError): ErrorInfo | undefined {
  for (const detail of err.details) {
    const { type, value } = detail as { type?: unknown; value?: unknown };
    if (typeof type !== "string" || !(value instanceof Uint8Array)) continue;
    const name = type.startsWith(TYPE_URL_PREFIX) ? type.slice(TYPE_URL_PREFIX.length) : type;
    if (name !== ERROR_INFO_TYPE) continue;
    const info = decodeErrorInfo(value);
    if (info) return info;
  }
  return undefined;
}

/**
 * The canonical text token, or `undefined` when the code, emitter or version
 * is empty or does not match the token grammar.
 */
export function errorToken(info: ErrorInfo): string | undefined {
  if (
    !CODE_PATTERN.test(info.code) ||
    !EMITTER_PATTERN.test(info.emitter) ||
    !VERSION_PATTERN.test(info.emitterVersion)
  ) {
    return undefined;
  }
  return `[bones-error code=${info.code} emitter=${info.emitter}@${info.emitterVersion}]`;
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
  /** The emitting deployable's build version. */
  readonly emitterVersion: string | undefined;
  /** The Connect failure this wraps. */
  readonly cause: ConnectError;

  constructor(cause: ConnectError) {
    const info = errorInfoOf(cause);
    super(messageWithToken(cause.message, info));
    this.cause = cause;
    this.code = info?.code || undefined;
    this.emitter = info?.emitter || undefined;
    this.emitterVersion = info?.emitterVersion || undefined;
  }

  /** The SDK error for a Connect failure, the way `ConnectError.from` is for any reason. */
  static from(reason: ConnectError): BonesError {
    return new BonesError(reason);
  }
}
