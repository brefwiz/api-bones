// SPDX-License-Identifier: MIT
//
// Optimistic-concurrency precondition, attached from the transport rather than
// remembered at every call site.
//
// A Connect handler that enforces `If-Match` (api-bones' `check_if_match`
// primitive) rejects a write that omits the header with `failed_precondition`.
// Requiring every UI call site to remember that header for every guarded RPC
// does not hold: nothing in the contract says which RPCs are guarded, so a new
// call site is one omitted header away from every save failing.
//
// Concurrency safety is a property of the METHOD, not of the call site — the
// same insight `retry.ts` already applies to replay safety. A method with
// `idempotency_level` other than `NO_SIDE_EFFECTS` mutates state, so it is
// preconditioned by default; a `NO_SIDE_EFFECTS` read never is. The default
// posture is last-write-wins (`if-match: *`), matching what every product
// call site set by hand before this existed. A caller that already attached
// its own `if-match` (a real ETag, for conflict detection) is never
// overridden — the interceptor only fills a gap, it never second-guesses a
// caller who did the work of tracking a real ETag.
//
// An unannotated method (no policy entry, or a streaming method) is treated
// as unmutating for this purpose: it gets no default header, same as before
// this interceptor existed. That is the same fail-closed posture `retry.ts`
// uses for replay — the blast radius of the default grows only as fast as
// the generated policy actually declares idempotency for a method.

import type { Interceptor } from "@connectrpc/connect";

import type { GeneratedMethodPolicy } from "./policy.js";
import { rpcIdentity } from "./retry.js";

/** The wildcard, last-write-wins precondition every product call site used by hand. */
export const IF_MATCH_ANY = "*";

/**
 * Whether a method mutates state and is therefore precondition-guarded by
 * default.
 *
 * Fails closed: no policy entry, a streaming method, or an explicit
 * `NO_SIDE_EFFECTS` read are all left alone. Only a declared `IDEMPOTENT` or
 * `UNSPECIFIED` unary method — i.e. anything that isn't declared read-only —
 * gets the default precondition.
 */
export function isPreconditionedMethod(policy: GeneratedMethodPolicy | undefined): boolean {
  if (!policy || policy.procedure !== "unary") return false;
  return policy.idempotency !== "NO_SIDE_EFFECTS";
}

export interface PreconditionInterceptorOptions {
  /** Generated policy, indexed by RPC. An empty map guards nothing. */
  readonly policyByRpc: ReadonlyMap<string, GeneratedMethodPolicy>;
}

/**
 * Attach `if-match: *` to a mutating call that named no precondition of its
 * own.
 *
 * Runs ahead of the retry interceptor in composition order so a retried
 * mutating call still carries the header on every attempt.
 */
export function makePreconditionInterceptor(opts: PreconditionInterceptorOptions): Interceptor {
  return (next) => (req) => {
    if (!req.stream && isPreconditionedMethod(opts.policyByRpc.get(rpcIdentity(req.method)))) {
      if (!req.header.has("if-match")) {
        req.header.set("if-match", IF_MATCH_ANY);
      }
    }
    return next(req);
  };
}
