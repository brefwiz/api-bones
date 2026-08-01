// SPDX-License-Identifier: MIT
//
// The generated method-policy contract, shared by every transport profile.
//
// `connect-method-policy.json` is emitted from the proto surface by
// ci-workflows' check-connect-read-profile.py (`--write`). It is a pure
// function of the protos, so it is never hand-edited: regenerate and commit.
//
// Everything here is runtime-agnostic on purpose. The browser adapter uses it
// to decide GET eligibility; the Node adapter carries the same vocabulary so a
// service and a webapp describe their surface identically, even though a Node
// transport can never issue a browser GET.

/** Composition-root profile. Browser GET is opt-in through `webapp`. */
export type SdkTransportProfile = "webapp" | "service";

export interface GeneratedBrowserCachePolicy {
  readonly scope: "PRIVATE" | "NO_STORE";
  readonly maxAgeSeconds: number;
}

export interface GeneratedMethodPolicy {
  readonly rpc: string;
  readonly procedure: "unary" | "streaming";
  readonly idempotency: "NO_SIDE_EFFECTS" | "IDEMPOTENT" | "UNSPECIFIED";
  readonly browserCache: GeneratedBrowserCachePolicy;
  readonly sensitivity: "NON_SENSITIVE" | "UNSPECIFIED";
  readonly maxEncodedUrlBytes: number;
}

export interface GeneratedMethodPolicyDocument {
  readonly schemaVersion: 1;
  readonly methods: readonly GeneratedMethodPolicy[];
}

export const MAX_CONNECT_GET_URL_BYTES = 4096;
export const MAX_PRIVATE_CACHE_TTL_SECONDS = 300;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isNonNegativeInteger(value: unknown): value is number {
  return (
    typeof value === "number" && Number.isFinite(value) && Number.isInteger(value) && value >= 0
  );
}

export function parseMethodPolicy(value: unknown): GeneratedMethodPolicy | null {
  if (!isRecord(value) || !isRecord(value.browserCache)) return null;
  const cache = value.browserCache;
  if (
    typeof value.rpc !== "string" ||
    !value.rpc.startsWith("/") ||
    (value.procedure !== "unary" && value.procedure !== "streaming") ||
    (value.idempotency !== "NO_SIDE_EFFECTS" &&
      value.idempotency !== "IDEMPOTENT" &&
      value.idempotency !== "UNSPECIFIED") ||
    (cache.scope !== "PRIVATE" && cache.scope !== "NO_STORE") ||
    !isNonNegativeInteger(cache.maxAgeSeconds) ||
    (value.sensitivity !== "NON_SENSITIVE" && value.sensitivity !== "UNSPECIFIED") ||
    !isNonNegativeInteger(value.maxEncodedUrlBytes) ||
    value.maxEncodedUrlBytes > MAX_CONNECT_GET_URL_BYTES
  ) {
    return null;
  }
  return {
    rpc: value.rpc,
    procedure: value.procedure,
    idempotency: value.idempotency,
    browserCache: {
      scope: cache.scope,
      maxAgeSeconds: cache.maxAgeSeconds,
    },
    sensitivity: value.sensitivity,
    maxEncodedUrlBytes: value.maxEncodedUrlBytes,
  };
}

/**
 * Index a policy document by RPC identity.
 *
 * Fails CLOSED to an empty map on any malformed or duplicated entry: a policy
 * that cannot be trusted in full grants no browser reads at all, rather than
 * silently granting the subset that happened to parse.
 */
export function indexGeneratedPolicy(value: unknown): ReadonlyMap<string, GeneratedMethodPolicy> {
  if (!isRecord(value) || value.schemaVersion !== 1 || !Array.isArray(value.methods)) {
    return new Map();
  }
  const index = new Map<string, GeneratedMethodPolicy>();
  for (const candidate of value.methods) {
    const method = parseMethodPolicy(candidate);
    if (!method || index.has(method.rpc)) return new Map();
    index.set(method.rpc, method);
  }
  return index;
}

/** Return policy only when it grants a bounded, non-sensitive browser read. */
export function eligibleBrowserReadPolicy(value: unknown): GeneratedMethodPolicy | null {
  const method = parseMethodPolicy(value);
  if (
    !method ||
    method.procedure !== "unary" ||
    method.idempotency !== "NO_SIDE_EFFECTS" ||
    method.browserCache.scope !== "PRIVATE" ||
    method.browserCache.maxAgeSeconds > MAX_PRIVATE_CACHE_TTL_SECONDS ||
    method.sensitivity !== "NON_SENSITIVE" ||
    method.maxEncodedUrlBytes <= 0
  ) {
    return null;
  }
  return method;
}
