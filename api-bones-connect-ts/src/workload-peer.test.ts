// SPDX-License-Identifier: MIT
//
// The peer's trust is exercised with the real SPIFFE verifier: only the SVID
// material is synthetic, so a certificate for another identity meets the same
// check a live handshake would.
import { execFileSync } from "node:child_process";
import { X509Certificate } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterAll, describe, expect, it } from "vitest";

import {
  MANAGED_APPLICATION_PORT,
  peerSpiffeId,
  WorkloadIdentityError,
  workloadPeerFor,
} from "./workload-identity.js";

const dir = mkdtempSync(join(tmpdir(), "workload-peer-"));
afterAll(() => rmSync(dir, { recursive: true, force: true }));

/** A self-signed certificate whose only URI SAN is `spiffeId`. */
function certFor(spiffeId: string, name: string): Buffer {
  const key = join(dir, `${name}.key`);
  const crt = join(dir, `${name}.crt`);
  execFileSync(
    "openssl",
    [
      "req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1",
      "-nodes", "-keyout", key, "-out", crt, "-days", "1", "-subj", "/CN=peer",
      "-addext", `subjectAltName=URI:${spiffeId}`,
    ],
    { stdio: "ignore" },
  );
  return new X509Certificate(readFileSync(crt, "utf8")).raw;
}

const OWN = "spiffe://prod.example/o/acme/svc/bff";
const ownDer = certFor(OWN, "own");

function watcherWith(spiffeId: string): never {
  return {
    current: () => ({
      spiffeId,
      leafDer: ownDer,
      intermediatesDer: [],
      privateKeyDer: new Uint8Array(),
      bundleDer: ownDer,
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
    const peer = workloadPeerFor(watcherWith(OWN), "ledger");
    expect(peer.url).toBe(`https://ledger:${MANAGED_APPLICATION_PORT}`);
    expect(MANAGED_APPLICATION_PORT).toBe(8080);
  });

  it("names the derived identity as the peer's spiffeId", () => {
    expect(workloadPeerFor(watcherWith(OWN), "ledger").spiffeId).toBe(
      "spiffe://prod.example/o/acme/svc/ledger",
    );
  });

  it.each(["", "Ledger", "led ger", "led/ger", "led:8080", "..", "-ledger", "ledger-", "a".repeat(64)])(
    "refuses %j as a workload name",
    (name) => {
      expect(kindOf(() => workloadPeerFor(watcherWith(OWN), name))).toBe("invalid_workload_name");
    },
  );

  it("accepts a 63 character label and one with digits", () => {
    expect(() => workloadPeerFor(watcherWith(OWN), "a".repeat(63))).not.toThrow();
    expect(() => workloadPeerFor(watcherWith(OWN), "svc-2")).not.toThrow();
  });

  it("presents this workload's SVID as the client certificate", () => {
    const tls = workloadPeerFor(watcherWith(OWN), "ledger").tls();
    expect(tls.cert).toContain("BEGIN CERTIFICATE");
    expect(tls.rejectUnauthorized).toBe(true);
  });

  describe("trust", () => {
    const check = (served: string): Error | undefined => {
      const tls = workloadPeerFor(watcherWith(OWN), "ledger").tls();
      const raw = certFor(served, served.replace(/\W/g, "_"));
      return tls.checkServerIdentity!("ledger", { raw } as never) ?? undefined;
    };

    it("accepts a server presenting the pinned identity", () => {
      expect(check("spiffe://prod.example/o/acme/svc/ledger")).toBeUndefined();
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
