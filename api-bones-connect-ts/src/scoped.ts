// SPDX-License-Identifier: MIT
//
// Per-call credential scoping, TypeScript side of ADR platform/0290.
//
// The Rust `ScopedClient` this mirrors exists because Rust's auth injector is
// `Fn() -> String` — no request context, so a caller whose credential varies
// per call has to stash it in ambient state and hope no concurrent call
// observes the wrong value. That is a real data race in Rust.
//
// It is not a race here. Every generated Connect-ES method already takes
// `CallOptions` as its last argument, per call, with no shared state between
// concurrent calls — `client.method(req, { headers })` on one call cannot be
// observed by another in flight on the same client. There is nothing to make
// safe that Connect-ES does not already make safe.
//
// What is missing is ergonomics, not safety: building the header by hand at
// every call site is boilerplate, and it gives per-call scoping no name a
// reader can search for. `scopedCallOptions` is that name — a thin helper
// around the mechanism that already exists, kept symmetric with the Rust
// side's `with_credential` so the same concept reads the same way in both
// languages.

import type { CallOptions } from "@connectrpc/connect";

/**
 * Build {@link CallOptions} that present `credential` on this call only.
 *
 * Merges into any `options` already being built for the call — an existing
 * `Authorization` header is overwritten (the scoped credential is the one
 * that must reach the server), everything else in `options` passes through
 * unchanged.
 *
 * @example
 * ```ts
 * import { scopedCallOptions } from "@brefwiz/api-bones-connect";
 *
 * // `claim` varies per call; the client and its transport are shared.
 * await client.createNamespace(req, scopedCallOptions(claim));
 * ```
 */
export function scopedCallOptions(credential: string, options?: CallOptions): CallOptions {
  const headers = new Headers(options?.headers);
  headers.set("Authorization", `Bearer ${credential}`);
  return { ...options, headers };
}
