// SPDX-License-Identifier: MIT
//
// Retry eligibility, derived from the proto surface rather than configured.
//
// Both adapters previously retried every unary call on Unavailable, Aborted and
// ResourceExhausted, with no knowledge of what the call did. That is the one
// thing every mature RPC client refuses to do: gRPC ships retries off by default
// and requires a per-method service config (gRFC A6); Google's client libraries
// retry only methods their API config declares idempotent; Stripe makes POST
// retries safe by requiring a client-supplied idempotency key. The common thread
// is that retry safety is a property of the METHOD, not of the transport.
//
// The generated policy artifact already carries that property per RPC —
// `idempotency` is emitted straight from the proto — so eligibility is read from
// it instead of being restated as a knob. A method nobody has annotated is not
// retried: unknown is treated as unsafe, so the blast radius of adding retries
// grows only as fast as somebody deliberately declares a method safe.
//
// Why this matters concretely: single-use credential redemption (bootstrap-token
// enrolment) is exactly the shape that must never be blind-retried. Connect can
// surface Unavailable AFTER the server accepted the request, so a retry burns
// the token and the operator sees "already used" instead of a network error.

import { Code, ConnectError, type Interceptor } from "@connectrpc/connect";

import type { BackoffOptions } from "./backoff.js";
import { computeBackoffDelay, resolveBackoff } from "./backoff.js";
import type { GeneratedMethodPolicy } from "./policy.js";

/**
 * Codes retried without any server instruction.
 *
 * Only Unavailable: it is the one code that reliably means "this connection did
 * not carry the call". Aborted is deliberately absent — it signals a concurrency
 * or transaction conflict, and the correct response is to re-run the enclosing
 * transaction, not to replay one RPC inside it. ResourceExhausted is absent too;
 * it is handled below only under explicit server pushback, because retrying a
 * quota rejection on our own schedule is how a degraded service is converted
 * into a fully saturated one.
 */
const UNPROMPTED_RETRYABLE_CODES: ReadonlySet<Code> = new Set([Code.Unavailable]);

/** Attempts after the initial call. */
export const MAX_RETRY_ATTEMPTS = 3;

export interface RetryThrottleOptions {
  /** Bucket capacity. Default 10. */
  readonly maxTokens?: number;
  /** Tokens returned per success. Default 0.1. */
  readonly tokenRatio?: number;
}

/**
 * A retry budget, modelled on gRPC's retry throttling.
 *
 * Backoff alone paces a single caller; it does nothing about many callers all
 * retrying a service that is already failing. The bucket drains on retryable
 * failures and refills slowly on success, so a broad outage suspends retries
 * altogether instead of multiplying the load that caused it — the standard
 * defence against a retry storm turning a partial outage into a total one.
 */
export class RetryThrottle {
  readonly #maxTokens: number;
  readonly #tokenRatio: number;
  #tokens: number;

  constructor(opts: RetryThrottleOptions = {}) {
    this.#maxTokens = opts.maxTokens ?? 10;
    this.#tokenRatio = opts.tokenRatio ?? 0.1;
    this.#tokens = this.#maxTokens;
  }

  /** Retries are permitted while the bucket is above half full. */
  get canRetry(): boolean {
    return this.#tokens > this.#maxTokens / 2;
  }

  recordFailure(): void {
    this.#tokens = Math.max(0, this.#tokens - 1);
  }

  recordSuccess(): void {
    this.#tokens = Math.min(this.#maxTokens, this.#tokens + this.#tokenRatio);
  }
}

/** `/package.Service/Method`, the identity the policy artifact is keyed by. */
export function rpcIdentity(method: {
  readonly name: string;
  readonly parent: { readonly typeName: string };
}): string {
  return `/${method.parent.typeName}/${method.name}`;
}

/**
 * Whether a method may be replayed at all.
 *
 * Fails closed on every uncertain input: no policy entry, a streaming method, or
 * an unannotated one all return false. An SDK that ships without its generated
 * policy therefore behaves exactly as it did before retries existed.
 */
export function isRetryableMethod(policy: GeneratedMethodPolicy | undefined): boolean {
  if (!policy || policy.procedure !== "unary") return false;
  return policy.idempotency === "NO_SIDE_EFFECTS" || policy.idempotency === "IDEMPOTENT";
}

/**
 * Server-requested delay in milliseconds, or null when the server said nothing.
 *
 * Accepts both `Retry-After` forms: delay-seconds and an HTTP-date.
 */
export function serverPushbackMs(err: ConnectError, now: number = Date.now()): number | null {
  const raw = err.metadata?.get("retry-after");
  if (raw === null || raw === undefined || raw.trim() === "") return null;

  const seconds = Number(raw);
  if (Number.isFinite(seconds)) return seconds >= 0 ? Math.round(seconds * 1000) : null;

  const at = Date.parse(raw);
  if (Number.isNaN(at)) return null;
  return Math.max(0, at - now);
}

export interface RetryInterceptorOptions {
  /** Generated policy, indexed by RPC. An empty map disables retries entirely. */
  readonly policyByRpc: ReadonlyMap<string, GeneratedMethodPolicy>;
  readonly backoff?: BackoffOptions;
  readonly throttle?: RetryThrottleOptions;
  /** Injected so tests need no real delay. */
  readonly sleep?: (ms: number) => Promise<void>;
}

const defaultSleep = (ms: number): Promise<void> =>
  new Promise<void>((resolve) => setTimeout(resolve, ms));

/** Retry interceptor shared by the browser and Node adapters. */
export function makeRetryInterceptor(opts: RetryInterceptorOptions): Interceptor {
  const backoff = resolveBackoff(opts.backoff);
  const throttle = new RetryThrottle(opts.throttle);
  const sleep = opts.sleep ?? defaultSleep;

  return (next) => async (req) => {
    // Streams reconnect on their own terms; replaying one here would restart a
    // subscription mid-flight rather than resume it.
    if (req.stream) return next(req);
    if (!isRetryableMethod(opts.policyByRpc.get(rpcIdentity(req.method)))) return next(req);

    let attempt = 0;
    for (;;) {
      try {
        const response = await next(req);
        throttle.recordSuccess();
        return response;
      } catch (err) {
        if (!(err instanceof ConnectError) || attempt >= MAX_RETRY_ATTEMPTS) throw err;

        const pushbackMs = serverPushbackMs(err);
        const retryable =
          UNPROMPTED_RETRYABLE_CODES.has(err.code) ||
          (err.code === Code.ResourceExhausted && pushbackMs !== null);
        if (!retryable) throw err;

        throttle.recordFailure();
        if (!throttle.canRetry) throw err;

        // Server pushback outranks our own schedule: it is the only party that
        // knows when it will be ready.
        await sleep(pushbackMs ?? computeBackoffDelay(attempt, backoff));
        attempt++;
      }
    }
  };
}
