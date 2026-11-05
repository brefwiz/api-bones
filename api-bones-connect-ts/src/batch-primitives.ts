// SPDX-License-Identifier: MIT
import { Code, ConnectError } from "@connectrpc/connect";
import { create, toBinary } from "@bufbuild/protobuf";
import { encodeErrorInfo, type ErrorInfo } from "./error-info.js";
import { ValidationFailureSchema } from "./generated/bones/v1/errors_pb.js";

export const BATCH_MAX_ITEMS = 200;
export const BATCH_MAX_REQUEST_BYTES = 1_048_576;
export interface ItemError {
  readonly code: string;
  readonly message: string;
  readonly info?: ErrorInfo;
  readonly violations?: readonly { readonly field: string; readonly message: string; readonly rule: string }[];
}
export type ItemResult<T> = { readonly ok: true; readonly value: T } | { readonly ok: false; readonly error: ConnectError };
export interface BatchResults<T> extends ReadonlyArray<ItemResult<T>> {}
/** Provider entry point: reject oversized requests before invoking any handler work. */
export async function serveBatchRequest<T>(ids: readonly string[], serve: () => T | Promise<T>): Promise<T> {
  if (ids.length > BATCH_MAX_ITEMS) throw batchError(Code.InvalidArgument, "BATCH_TOO_LARGE", "too many items in one batch request");
  return serve();
}
function itemError(error: unknown): ConnectError {
  if (error instanceof ConnectError && [Code.NotFound, Code.PermissionDenied, Code.InvalidArgument, Code.FailedPrecondition].includes(error.code)) return error;
  throw error;
}
/** Server item paths apply the single operation's object authorizer before encoding. */
export async function authorizeBatchResults<T>(outcomes: readonly ItemResult<T>[], authorize: (value: T) => void | Promise<void>): Promise<BatchResults<T>> {
  return Promise.all(outcomes.map(async (outcome): Promise<ItemResult<T>> => {
    if (!outcome.ok) return { ok: false, error: itemError(outcome.error) };
    try { await authorize(outcome.value); return outcome; }
    catch (error) { return { ok: false, error: itemError(error) }; }
  }));
}
export function allOk<T>(results: BatchResults<T>): T[] {
  return results.map(result => { if (!result.ok) throw result.error; return result.value; });
}
const codes: Readonly<Record<string, Code>> = {
  canceled: Code.Canceled, unknown: Code.Unknown, invalid_argument: Code.InvalidArgument,
  deadline_exceeded: Code.DeadlineExceeded, not_found: Code.NotFound, already_exists: Code.AlreadyExists,
  permission_denied: Code.PermissionDenied, resource_exhausted: Code.ResourceExhausted,
  failed_precondition: Code.FailedPrecondition, aborted: Code.Aborted, out_of_range: Code.OutOfRange,
  unimplemented: Code.Unimplemented, internal: Code.Internal, unavailable: Code.Unavailable,
  data_loss: Code.DataLoss, unauthenticated: Code.Unauthenticated,
};
export function itemErrorToConnect(error: ItemError): ConnectError {
  const code = Object.prototype.hasOwnProperty.call(codes, error.code) ? codes[error.code] : Code.Unknown;
  const result = new ConnectError(error.message, code);
  if (error.info) result.details.push({ type: "bones.v1.ErrorInfo", value: encodeErrorInfo(error.info) });
  for (const violation of error.violations ?? []) result.details.push({ type: "bones.v1.ValidationFailure", value: toBinary(ValidationFailureSchema, create(ValidationFailureSchema, violation)) });
  return result;
}
export function batchError(code: Code, token: string, message: string): ConnectError {
  const error = new ConnectError(message, code);
  error.details.push({ type: "bones.v1.ErrorInfo", value: encodeErrorInfo({ code: token, emitter: "api-bones-connect" }) });
  return error;
}
export function identityChanged(): ConnectError {
  return batchError(Code.Unauthenticated, "BATCH_IDENTITY_CHANGED", "batch identity changed");
}
/** Session-source failures and changes both fail closed. */
export function checkBatchIdentity(source: () => string, captured: string): void {
  try { if (source() === captured) return; } catch { /* Fail closed below. */ }
  throw identityChanged();
}
export interface BatchPlan {
  readonly chunks: readonly (readonly string[])[];
  readonly positions: ReadonlyMap<string, readonly number[]>;
  readonly rejected: ReadonlyMap<string, ConnectError>;
}
/** Encoder must encode complete generated request, including template fields. */
export function planBatch(ids: readonly string[], encode: (ids: readonly string[]) => Uint8Array): BatchPlan {
  const positions = new Map<string, number[]>();
  ids.forEach((id, index) => { const indices = positions.get(id); if (indices) indices.push(index); else positions.set(id, [index]); });
  const chunks: string[][] = [];
  const rejected = new Map<string, ConnectError>();
  let chunk: string[] = [];
  for (const id of positions.keys()) {
    if (encode([id]).byteLength > BATCH_MAX_REQUEST_BYTES) {
      rejected.set(id, batchError(Code.InvalidArgument, "BATCH_ITEM_TOO_LARGE", "batch item exceeds request size limit"));
      continue;
    }
    if (chunk.length && (chunk.length === BATCH_MAX_ITEMS || encode([...chunk, id]).byteLength > BATCH_MAX_REQUEST_BYTES)) { chunks.push(chunk); chunk = []; }
    chunk.push(id);
  }
  if (chunk.length) chunks.push(chunk);
  return { chunks, positions, rejected };
}
/** Validate before exposing any item: malformed chunk never partially succeeds. */
export function validateBatchResponse<T extends { readonly id: string }>(ids: readonly string[], results: readonly T[]): ConnectError | undefined {
  return results.length !== ids.length || results.some((result, index) => result.id !== ids[index])
    ? batchError(Code.Internal, "BATCH_RESPONSE_MISMATCH", "batch response does not match requested ids") : undefined;
}
export interface CapturedBatchKey {
  readonly method: string;
  readonly requestHash: string;
  readonly headersHash: string;
  readonly identity: string;
}
export interface CapturedBatchHeaders { readonly headers: Headers; readonly headersHash: string; readonly credentialHash: string; }
/** Unambiguous uint32-length-prefixed UTF-8 pairs, sorted by name and value. */
function headerBytes(headers: Headers, credentialsOnly: boolean): Uint8Array {
  const pairs: [string, string][] = [];
  headers.forEach((value, name) => {
    name = name.toLowerCase();
    if (!credentialsOnly || name === "authorization" || name === "brefwiz-relayed-credential" || name === "cookie" || name === "proxy-authorization") pairs.push([name, value]);
  });
  pairs.sort((a, b) => a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : a[1] < b[1] ? -1 : a[1] > b[1] ? 1 : 0);
  const encoded = pairs.flatMap(pair => pair.map(value => new TextEncoder().encode(value)));
  const out = new Uint8Array(encoded.reduce((sum, bytes) => sum + 4 + bytes.length, 0));
  const view = new DataView(out.buffer);
  let offset = 0;
  for (const bytes of encoded) { view.setUint32(offset, bytes.length, false); offset += 4; out.set(bytes, offset); offset += bytes.length; }
  return out;
}
export async function batchHash(bytes: Uint8Array): Promise<string> {
  const digest = await globalThis.crypto.subtle.digest("SHA-256", Uint8Array.from(bytes));
  return Array.from(new Uint8Array(digest), byte => byte.toString(16).padStart(2, "0")).join("");
}
/** Input must already include interceptor credentials; snapshot precedes async hashing. */
export async function captureBatchHeaders(effective: HeadersInit, timeoutMs?: number): Promise<CapturedBatchHeaders> {
  const headers = new Headers(effective);
  if (timeoutMs !== undefined) {
    if (!Number.isFinite(timeoutMs) || timeoutMs < 0) throw new ConnectError("invalid timeout", Code.InvalidArgument);
    headers.set("connect-timeout-ms", String(Math.ceil(timeoutMs)));
  }
  const all = headerBytes(headers, false), credentials = headerBytes(headers, true);
  const [headersHash, credentialHash] = await Promise.all([batchHash(all), batchHash(credentials)]);
  return { headers, headersHash, credentialHash };
}
/** Compare individual tuple components, never ambiguous joined strings. */
export function sameBatchKey(a: CapturedBatchKey, b: CapturedBatchKey): boolean {
  return a.method === b.method && a.requestHash === b.requestHash && a.headersHash === b.headersHash && a.identity === b.identity;
}
