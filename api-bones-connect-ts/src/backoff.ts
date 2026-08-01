// SPDX-License-Identifier: MIT
//
// Retry backoff maths, moved verbatim from core-ui's
// src/data/subscriptions/backoff.ts. Pure arithmetic with no runtime
// dependencies, so the browser transport, the Node transport, and core-ui's
// subscription hooks can all share one copy — a Node service transport must not
// import from a UI package to get its retry schedule.
//
// Behaviour is deliberately unchanged: same option names, same defaults
// (500ms initial, 30s cap, x2), same floor-and-full-jitter delay, same
// injectable `random` for deterministic tests. Retiming retries while relocating
// them would be an invisible behaviour change to every existing consumer.

export interface BackoffOptions {
  initialDelayMs?: number;
  maxDelayMs?: number;
  multiplier?: number;
}

export interface BackoffConfig {
  initialDelayMs: number;
  maxDelayMs: number;
  multiplier: number;
}

export const DEFAULT_BACKOFF: BackoffConfig = {
  initialDelayMs: 500,
  maxDelayMs: 30000,
  multiplier: 2,
};

export function resolveBackoff(opts?: BackoffOptions): BackoffConfig {
  return {
    initialDelayMs: opts?.initialDelayMs ?? DEFAULT_BACKOFF.initialDelayMs,
    maxDelayMs: opts?.maxDelayMs ?? DEFAULT_BACKOFF.maxDelayMs,
    multiplier: opts?.multiplier ?? DEFAULT_BACKOFF.multiplier,
  };
}

export function computeBackoffDelay(
  attempt: number,
  config: BackoffConfig,
  random: () => number = Math.random,
): number {
  const safeAttempt = Math.max(0, Math.floor(attempt));
  const exp = config.initialDelayMs * Math.pow(config.multiplier, safeAttempt);
  const capped = Math.min(exp, config.maxDelayMs);
  return Math.floor(random() * capped);
}
