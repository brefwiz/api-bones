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
// One class of failure is exempt from that gate: a request the transport can
// PROVE never left the client (the connection died before its TLS handshake
// finished, or before any request byte was written). No server saw the call, so
// replaying it cannot run anything twice, whatever the method declares. That
// proof is structured evidence recorded by the transport (`delivery-evidence.ts`),
// never inferred from an error message, and the replay stays inside the same
// bounded attempt count, backoff and throttle as every other retry.
//
// Why this matters concretely: single-use credential redemption (bootstrap-token
// enrolment) is exactly the shape that must never be blind-retried. Connect can
// surface Unavailable AFTER the server accepted the request, so a retry burns
// the token and the operator sees "already used" instead of a network error.

import { Code, ConnectError, type Interceptor } from "@connectrpc/connect";

// The vocabulary lives in its Rust-paired module; retry.ts is the pipeline
// that consumes it and has no Rust counterpart.
import {
  connectionFailureAsUnavailable,
  isConnectionWriteFailure,
  isReplayableTransportFailure,
} from "./retry-eligibility.js";

import { isNotDelivered, isResetBeforeResponse } from "./delivery-evidence.js";
import type { BackoffOptions } from "./backoff.js";
import { computeBackoffDelay, resolveBackoff } from "./backoff.js";
import type { GeneratedMethodPolicy } from "./policy.js";

export {
  isNotDelivered,
  isResetBeforeResponse,
  markNotDelivered,
  markResetBeforeResponse,
} from "./delivery-evidence.js";
export {
  connectionFailureAsUnavailable,
  isConnectionWriteFailure,
  isReplayableTransportFailure,
  isUnpromptedRetryable,
} from "./retry-eligibility.js";

/**
 * Interceptor form of {@link connectionFailureAsUnavailable}.
 *
 * The mapping itself is the paired vocabulary and lives in
 * `retry-eligibility.ts`, where Rust has the same function. Only this wrapper
 * is TypeScript-only, because Rust has no client interceptor pipeline for one
 * to live in — a Rust caller applies the mapping directly.
 *
 * Installed OUTSIDE the retry interceptor: the mapping recognises these
 * failures by their Internal code, so re-coding before retry runs would make
 * every one of them unretryable. Only what escapes retry is re-coded.
 */
export function makeConnectionFailureNormalizer(): Interceptor {
  return (next) => async (req) => {
    try {
      return await next(req);
    } catch (err) {
      throw connectionFailureAsUnavailable(ConnectError.from(err));
    }
  };
}

/** Attempts after the initial call. */
export const MAX_RETRY_ATTEMPTS = 3;

/** Longest server pushback a call is held for; a longer one fails fast. */
export const MAX_THROTTLE_WAIT_MS = 5_000;

/** Upper bound of the random extra wait, as a fraction of the pushback. */
const THROTTLE_JITTER_FRACTION = 0.25;

/**
 * A call the server refused for rate limiting and that was not served.
 *
 * It is a `ResourceExhausted` error, never `Unauthenticated`: the caller's
 * session is intact and only needs to try again after `retryAfterMs`.
 */
export class RateLimitedError extends ConnectError {
  /**
   * `ConnectError` recognises its instances structurally, by `name`, and a
   * subclass inherits that check: `instanceof RateLimitedError` would hold for
   * every `ConnectError`. This restores the ordinary prototype-chain test. The
   * `name` stays `ConnectError`, so the instance is still one to `ConnectError`
   * checks and `ConnectError.from` passes it through unchanged.
   */
  static override [Symbol.hasInstance](value: unknown): boolean {
    return Function.prototype[Symbol.hasInstance].call(RateLimitedError, value);
  }

  constructor(
    /** Milliseconds the server (or the origin's admission gate) asked callers to wait. */
    readonly retryAfterMs: number,
    cause: ConnectError,
  ) {
    super(cause.rawMessage, Code.ResourceExhausted, cause.metadata, undefined, cause);
  }
}

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

/**
 * One retried attempt, reported just before the interceptor sleeps and
 * replays the call.
 *
 * `attempt` is 1-based and counts the failed attempt that triggered this
 * sleep (1 on the first retry, matching what a human reading a log expects).
 * `delayMs` is the sleep the interceptor is about to take before the next
 * attempt, whichever source set it — server pushback or computed backoff.
 */
export interface RetryEvent {
  /** `/package.Service/Method`, matching {@link rpcIdentity}. */
  readonly method: string;
  readonly attempt: number;
  readonly code: Code;
  readonly message?: string;
  readonly delayMs: number;
}

export interface RetryInterceptorOptions {
  /**
   * Generated policy, indexed by RPC. An empty map disables retries for every
   * failure that may have reached the server; a failure the transport proved
   * never left the client is still replayed.
   */
  readonly policyByRpc: ReadonlyMap<string, GeneratedMethodPolicy>;
  readonly backoff?: BackoffOptions;
  readonly throttle?: RetryThrottleOptions;
  /**
   * Injected so tests need no real delay. A sleep that honours `signal` should
   * release its timer when the signal aborts.
   */
  readonly sleep?: (ms: number, signal?: AbortSignal) => Promise<void>;
  /** Random source in [0, 1) for throttle jitter; injected so tests are deterministic. */
  readonly random?: () => number;
  /**
   * Observes each retried attempt, called synchronously right before the
   * interceptor sleeps. Never called on the call's first attempt, on a
   * non-retryable method, or when the failure isn't eligible for retry.
   *
   * An observer that throws is caught and ignored: instrumentation must never
   * turn a successful retry into a failed call.
   */
  readonly onRetry?: (event: RetryEvent) => void;
}

const defaultSleep = (ms: number, signal?: AbortSignal): Promise<void> =>
  new Promise<void>((resolve, reject) => {
    if (signal?.aborted) {
      reject(ConnectError.from(signal.reason));
      return;
    }
    const onAbort = (): void => {
      clearTimeout(timer);
      reject(ConnectError.from(signal?.reason));
    };
    const timer = setTimeout(() => {
      signal?.removeEventListener("abort", onAbort);
      resolve();
    }, ms);
    signal?.addEventListener("abort", onAbort, { once: true });
  });

/**
 * Pause shared by every call to one origin after the origin refused one for
 * rate limiting, so queued calls wait for the same deadline instead of each
 * spending a refusal. Holds a deadline only; no timer outlives a wait.
 */
class OriginGate {
  #until = 0;
  #pushbackMs = 0;

  refuse(pushbackMs: number, now: number): void {
    const until = now + pushbackMs;
    if (until > this.#until) {
      this.#until = until;
      this.#pushbackMs = pushbackMs;
    }
  }

  /**
   * Resolves once the deadline has passed, then waits `jitterMs` (default: a
   * random share of the last pushback) so woken calls do not land together.
   * The deadline never exceeds {@link MAX_THROTTLE_WAIT_MS}, because a longer
   * pushback is never recorded.
   */
  async admit(
    sleep: (ms: number, signal?: AbortSignal) => Promise<void>,
    random: () => number,
    signal: AbortSignal | undefined,
    jitterMs?: number,
  ): Promise<void> {
    let waited = false;
    for (let remaining = this.#until - Date.now(); remaining > 0; remaining = this.#until - Date.now()) {
      waited = true;
      await sleep(remaining, signal);
    }
    if (waited || jitterMs !== undefined) {
      const jitter = jitterMs ?? Math.floor(random() * THROTTLE_JITTER_FRACTION * this.#pushbackMs);
      if (jitter > 0) await sleep(jitter, signal);
    }
  }
}

function logStaleConnectionReplay(method: string, procedure: string, cause: string): void {
  console.debug(
    JSON.stringify({ event: "connect.stale_connection_replay", method, procedure, cause }),
  );
}

function reportRetry(
  onRetry: RetryInterceptorOptions["onRetry"],
  req: { readonly method: Parameters<typeof rpcIdentity>[0] },
  attempt: number,
  err: ConnectError,
  delayMs: number,
): void {
  if (!onRetry) return;
  try {
    onRetry({
      method: rpcIdentity(req.method),
      attempt: attempt + 1,
      code: err.code,
      message: err.rawMessage,
      delayMs,
    });
  } catch {
    // Instrumentation is not allowed to affect the call it observes.
  }
}

/** Retry interceptor shared by the browser and Node adapters. */
export function makeRetryInterceptor(opts: RetryInterceptorOptions): Interceptor {
  const backoff = resolveBackoff(opts.backoff);
  const throttle = new RetryThrottle(opts.throttle);
  const sleep = opts.sleep ?? defaultSleep;
  const random = opts.random ?? Math.random;
  const gates = new Map<string, OriginGate>();
  const gateFor = (url: string | undefined): OriginGate => {
    // A relative base URL has no origin of its own: its calls share one gate.
    let origin = "";
    try {
      origin = new URL(url ?? "").origin;
    } catch {
      origin = "";
    }
    let gate = gates.get(origin);
    if (gate === undefined) {
      gate = new OriginGate();
      gates.set(origin, gate);
    }
    return gate;
  };

  return (next) => async (req) => {
    // Streams reconnect on their own terms; replaying one here would restart a
    // subscription mid-flight rather than resume it.
    if (req.stream) return next(req);
    const policy = opts.policyByRpc.get(rpcIdentity(req.method));
    const methodRetryable = isRetryableMethod(policy);
    const maxRetries = policy?.paging ? 2 : MAX_RETRY_ATTEMPTS;
    // A reset before any response byte replays only a read that cannot change
    // anything, and only once: a second reset is not a stale pooled socket.
    const replaysStaleReset = policy?.procedure === "unary" && policy.idempotency === "NO_SIDE_EFFECTS";
    let staleResetReplayed = false;

    const gate = gateFor(req.url);
    let attempt = 0;
    let jitterMs: number | undefined;
    for (;;) {
      await gate.admit(sleep, random, req.signal, jitterMs);
      jitterMs = undefined;
      try {
        const response = await next(req);
        throttle.recordSuccess();
        return response;
      } catch (err) {
        if (!(err instanceof ConnectError)) throw err;

        const pushbackMs = serverPushbackMs(err);
        // A throttle is refused before the handler runs, so any method is
        // replayed after the server's own deadline without drawing on the retry
        // budget that guards blind retries. A refusal that names no deadline is
        // not a throttle and is never replayed. Spent attempts surface as a rate
        // limit the caller can act on, not as a signed-out session.
        if (err.code === Code.ResourceExhausted && pushbackMs !== null) {
          if (err instanceof RateLimitedError) throw err;
          if (attempt >= maxRetries || pushbackMs > MAX_THROTTLE_WAIT_MS) {
            throw new RateLimitedError(pushbackMs, err);
          }
          gate.refuse(pushbackMs, Date.now());
          jitterMs = Math.floor(random() * THROTTLE_JITTER_FRACTION * pushbackMs);
          reportRetry(opts.onRetry, req, attempt, err, pushbackMs + jitterMs);
          attempt++;
          continue;
        }
        if (attempt >= maxRetries) throw err;
        // A request that provably never left the client is replay-safe whatever
        // the method declares; every other failure may have run on the server,
        // so it needs the method to have been declared safe.
        const staleReset =
          replaysStaleReset && !staleResetReplayed && isResetBeforeResponse(err);
        const retryable =
          staleReset ||
          isNotDelivered(err) ||
          (methodRetryable && isReplayableTransportFailure(err));
        if (!retryable) throw err;

        throttle.recordFailure();
        if (!throttle.canRetry) throw err;
        if (staleReset) {
          staleResetReplayed = true;
          logStaleConnectionReplay(req.method.name, rpcIdentity(req.method), err.rawMessage);
        }

        // Server pushback outranks our own schedule: it is the only party that
        // knows when it will be ready.
        const delayMs = pushbackMs ?? computeBackoffDelay(attempt, backoff);
        reportRetry(opts.onRetry, req, attempt, err, delayMs);
        await sleep(delayMs, req.signal);
        attempt++;
      }
    }
  };
}
