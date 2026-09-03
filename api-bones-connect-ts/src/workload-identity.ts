// SPDX-License-Identifier: MIT
//
// Client transport identity acquired from the SPIFFE Workload API.
//
// A consumer calling an in-fleet service supplies no provider coordinates: no
// CA path, no socket path, no trust bundle, no endpoint trust config. The
// identity, the trust bundle and the set of servers worth talking to are all
// derived from the SVID the Workload API already issues to this workload.
//
// The peer allow-list is the caller's OWN trust domain. A workload holding
// `spiffe://prod.brefwiz/svc/bff` will talk to any `spiffe://prod.brefwiz/...`
// server and refuse everything else. That is a derivation, not a knob —
// nothing about it is answerable by a consumer.
//
// This is the TypeScript half of the same contract implemented in Rust by
// `api-bones-connect`'s `workload_identity` module. The two are kept
// behaviourally equivalent deliberately: a target that only fetched an
// identity, or only pinned the peer, would be a per-language subset.

import {
  startWatcherSafe,
  tlsClientConfigFromWatcher,
  type SvidWatcher,
} from "@brefwiz/spiffe-client";

import type { NodeTlsIdentity } from "./node-diagnostics.js";

/** Attempts made to reach the Workload API before reporting it absent. */
export const WATCHER_ATTEMPTS = 3;

/** Why a transport could not take its identity from the Workload API. */
export type WorkloadIdentityErrorKind =
  | "workload_api_unavailable"
  | "unusable_trust_domain";

/**
 * A terminal, nameable reason the Workload API could not supply an identity.
 *
 * None of these is a reason to fall back to an anonymous transport — the caller
 * decides what to do, and the transport treats absence as "serve no in-fleet
 * traffic".
 */
export class WorkloadIdentityError extends Error {
  readonly kind: WorkloadIdentityErrorKind;

  constructor(message: string, kind: WorkloadIdentityErrorKind) {
    super(message);
    this.name = "WorkloadIdentityError";
    this.kind = kind;
  }
}

/**
 * The trust domain of a SPIFFE ID, i.e. the authority of `spiffe://<td>/path`.
 *
 * Returns null when the id does not have the SPIFFE shape, so the caller can
 * report which SVID was unusable rather than throwing an opaque URL error.
 */
export function trustDomainOf(spiffeId: string): string | null {
  const withoutScheme = spiffeId.startsWith("spiffe://")
    ? spiffeId.slice("spiffe://".length)
    : null;
  if (withoutScheme === null) return null;

  const slash = withoutScheme.indexOf("/");
  const domain = slash === -1 ? withoutScheme : withoutScheme.slice(0, slash);
  return domain.length > 0 ? domain : null;
}

/**
 * Derive the peer allow-list from `watcher`'s own SVID and build the identity.
 *
 * Split from {@link workloadClientTlsIdentity} so the derivation is testable
 * against a watcher built in-process, without a live agent socket.
 */
export function clientTlsIdentityFor(watcher: SvidWatcher): NodeTlsIdentity {
  const svid = watcher.current();
  const trustDomain = trustDomainOf(svid.spiffeId);

  if (trustDomain === null) {
    throw new WorkloadIdentityError(
      `SVID ${svid.spiffeId} carries no usable trust domain`,
      "unusable_trust_domain",
    );
  }

  const options = tlsClientConfigFromWatcher(watcher, [
    `spiffe://${trustDomain}/*`,
  ]);

  return { cert: options.cert, key: options.key, ca: options.ca };
}

/**
 * Build a Node TLS identity whose certificate and trust both come from the
 * Workload API.
 *
 * @throws {WorkloadIdentityError} when no Workload API answers, or when its
 * SVID names no trust domain. It never resolves to an anonymous identity.
 */
export async function workloadClientTlsIdentity(): Promise<NodeTlsIdentity> {
  const watcher = await startWatcherSafe(undefined, WATCHER_ATTEMPTS);

  if (watcher === null) {
    throw new WorkloadIdentityError(
      `SPIFFE Workload API unavailable after ${WATCHER_ATTEMPTS} attempts; ` +
        `this workload has no in-fleet identity`,
      "workload_api_unavailable",
    );
  }

  return clientTlsIdentityFor(watcher);
}
