// SPDX-License-Identifier: MIT
import { describe, expect, it, vi } from "vitest";

import {
  clientTlsIdentityFor,
  trustDomainOf,
  WATCHER_ATTEMPTS,
  WorkloadIdentityError,
} from "./workload-identity.js";

vi.mock("@brefwiz/spiffe-client", async () => {
  const actual = await vi.importActual<Record<string, unknown>>(
    "@brefwiz/spiffe-client",
  );
  return {
    ...actual,
    // Record the allow-list the transport derives, so a test can assert the
    // caller never has to name a peer.
    tlsClientConfigFromWatcher: vi.fn(
      (_watcher: unknown, allowedServerIds: string[]) => ({
        cert: "CERT-PEM",
        key: "KEY-PEM",
        ca: "CA-PEM",
        allowedServerIds,
      }),
    ),
  };
});

const { tlsClientConfigFromWatcher } = await import("@brefwiz/spiffe-client");

function watcherWith(spiffeId: string): never {
  return { current: () => ({ spiffeId }) } as never;
}

describe("trustDomainOf", () => {
  it("takes the authority out of a SPIFFE id", () => {
    expect(trustDomainOf("spiffe://prod.brefwiz/svc/bff")).toBe("prod.brefwiz");
  });

  it("handles a trust-domain-only id", () => {
    expect(trustDomainOf("spiffe://prod.brefwiz")).toBe("prod.brefwiz");
  });

  it("rejects a non-SPIFFE id rather than inventing a domain", () => {
    expect(trustDomainOf("https://prod.brefwiz/svc")).toBeNull();
    expect(trustDomainOf("spiffe:///svc/bff")).toBeNull();
  });
});

describe("clientTlsIdentityFor", () => {
  it("derives the peer allow-list from the caller's own trust domain", () => {
    clientTlsIdentityFor(watcherWith("spiffe://prod.brefwiz/svc/bff"));

    expect(tlsClientConfigFromWatcher).toHaveBeenCalledWith(expect.anything(), [
      "spiffe://prod.brefwiz/*",
    ]);
  });

  it("returns cert, key and ca — the consumer supplies none of them", () => {
    const identity = clientTlsIdentityFor(
      watcherWith("spiffe://prod.brefwiz/svc/bff"),
    );

    expect(identity).toEqual({ cert: "CERT-PEM", key: "KEY-PEM", ca: "CA-PEM" });
  });

  it("names the unusable SVID rather than failing opaquely", () => {
    expect(() => clientTlsIdentityFor(watcherWith("not-a-spiffe-id"))).toThrow(
      WorkloadIdentityError,
    );

    try {
      clientTlsIdentityFor(watcherWith("not-a-spiffe-id"));
      expect.unreachable("must throw");
    } catch (err) {
      expect(err).toBeInstanceOf(WorkloadIdentityError);
      expect((err as WorkloadIdentityError).kind).toBe("unusable_trust_domain");
      expect((err as Error).message).toContain("not-a-spiffe-id");
    }
  });
});

describe("WorkloadIdentityError", () => {
  it("carries a machine-readable kind alongside the message", () => {
    const err = new WorkloadIdentityError("nope", "workload_api_unavailable");
    expect(err.kind).toBe("workload_api_unavailable");
    expect(err.name).toBe("WorkloadIdentityError");
    expect(err).toBeInstanceOf(Error);
  });

  it("agrees with Rust on how many attempts count as absent", () => {
    expect(WATCHER_ATTEMPTS).toBe(3);
  });
});
