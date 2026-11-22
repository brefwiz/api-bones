// SPDX-License-Identifier: MIT
//
// The peer's trust is exercised with the real SPIFFE verifier: only the SVID
// material is synthetic, so a certificate for another identity meets the same
// check a live handshake would.
import { X509Certificate } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import type { PeerPathRule } from "@brefwiz/spiffe-client";
import { afterAll, describe, expect, it } from "vitest";

import { authority, leaf } from "../../tests/typescript/steps/pinned_peer_fixtures.ts";

import {
  MANAGED_APPLICATION_PORT,
  peerSpiffeId,
  WorkloadIdentityError,
  workloadPeerFor,
} from "./workload-identity.js";

const dir = mkdtempSync(join(tmpdir(), "workload-peer-"));
afterAll(() => rmSync(dir, { recursive: true, force: true }));

const ca = authority(dir);
const caDer = new X509Certificate(readFileSync(ca.cert, "utf8")).raw;

/** A non-CA certificate issued by the test CA whose only URI SAN is `spiffeId`. */
function certFor(spiffeId: string): Buffer {
  return new X509Certificate(leaf(dir, ca, [spiffeId]).cert).raw;
}

/** Test rules: the caller injects the path rule; this package has no default. */
const admit: PeerPathRule = () => {};
const refuse: PeerPathRule = () => {
  throw { kind: "peer_chain_refused", message: "path not admitted" };
};

const OWN = "spiffe://prod.example/o/acme/svc/bff";
const ownDer = certFor(OWN);

function watcherWith(spiffeId: string): never {
  return {
    current: () => ({
      spiffeId,
      leafDer: ownDer,
      intermediatesDer: [],
      privateKeyDer: new Uint8Array(),
      bundleDer: caDer,
    }),
  } as never;
}

function kindOf(run: () => unknown): string | undefined {
  try {
    run();
  } catch (err) {
    expect(err).toBeInstanceOf(WorkloadIdentityError);
    return (err as WorkloadIdentityError).kind;
  }
  return undefined;
}

describe("peerSpiffeId", () => {
  it("replaces the trailing service, keeping trust domain and organization", () => {
    expect(peerSpiffeId(OWN, "ledger")).toBe("spiffe://prod.example/o/acme/svc/ledger");
  });

  it("works for an identity that names no organization", () => {
    expect(peerSpiffeId("spiffe://prod.example/svc/bff", "ledger")).toBe(
      "spiffe://prod.example/svc/ledger",
    );
  });

  it.each(["spiffe://prod.example", "spiffe://prod.example/o/acme", "not-spiffe"])(
    "refuses %j, which names no service to replace",
    (own) => {
      expect(kindOf(() => peerSpiffeId(own, "ledger"))).toBe("unusable_peer_identity");
    },
  );
});

describe("workloadPeerFor", () => {
  it("addresses the workload on the managed application port", () => {
    const peer = workloadPeerFor(watcherWith(OWN), "ledger", admit);
    expect(peer.url).toBe(`https://ledger:${MANAGED_APPLICATION_PORT}`);
    expect(MANAGED_APPLICATION_PORT).toBe(8080);
  });

  it("names the derived identity as the peer's spiffeId", () => {
    expect(workloadPeerFor(watcherWith(OWN), "ledger", admit).spiffeId).toBe(
      "spiffe://prod.example/o/acme/svc/ledger",
    );
  });

  it.each(["", "Ledger", "led ger", "led/ger", "led:8080", "..", "-ledger", "ledger-", "a".repeat(64)])(
    "refuses %j as a workload name",
    (name) => {
      expect(kindOf(() => workloadPeerFor(watcherWith(OWN), name, admit))).toBe("invalid_workload_name");
    },
  );

  it("accepts a 63 character label and one with digits", () => {
    expect(() => workloadPeerFor(watcherWith(OWN), "a".repeat(63, admit))).not.toThrow();
    expect(() => workloadPeerFor(watcherWith(OWN), "svc-2", admit)).not.toThrow();
  });

  it("presents this workload's SVID as the client certificate", () => {
    const tls = workloadPeerFor(watcherWith(OWN), "ledger", admit).tls();
    expect(tls.cert).toContain("BEGIN CERTIFICATE");
    expect(tls.rejectUnauthorized).toBe(true);
  });

  describe("trust", () => {
    const check = (served: string, rule: PeerPathRule = admit): Error | undefined => {
      const tls = workloadPeerFor(watcherWith(OWN), "ledger", rule).tls();
      const raw = certFor(served);
      return tls.checkServerIdentity!("ledger", { raw } as never) ?? undefined;
    };

    it("accepts a server presenting the pinned identity", () => {
      expect(check("spiffe://prod.example/o/acme/svc/ledger")).toBeUndefined();
    });

    it("refuses the pinned identity when the injected path rule refuses its path", () => {
      expect(check("spiffe://prod.example/o/acme/svc/ledger", refuse)).toBeInstanceOf(Error);
    });

    it("refuses a server presenting another workload's identity", () => {
      expect(check("spiffe://prod.example/o/acme/svc/other")).toBeInstanceOf(Error);
    });

    it("refuses the same workload in another organization", () => {
      expect(check("spiffe://prod.example/o/rival/svc/ledger")).toBeInstanceOf(Error);
    });

    it("refuses the same workload in another trust domain", () => {
      expect(check("spiffe://evil.example/o/acme/svc/ledger")).toBeInstanceOf(Error);
    });
  });
});
