// SPDX-License-Identifier: MIT
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

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

describe("workload identity and the runtime-agnostic entry", () => {
  // Browser code imports the root for the policy and retry helpers. Workload
  // identity reaches node:crypto, so anything that pulls it into the root's
  // module graph breaks every browser bundle built on this package.
  it("is not reachable from index.ts", () => {
    const seen = new Set<string>();
    const external = new Set<string>();
    const walk = (file: string): void => {
      if (seen.has(file)) return;
      seen.add(file);
      const source = readFileSync(file, "utf8");
      for (const match of source.matchAll(/(?:import|export)[^"';]*?from\s*["']([^"']+)["']/g)) {
        const spec = match[1];
        if (spec.startsWith(".")) walk(resolve(dirname(file), spec.replace(/\.js$/, ".ts")));
        else external.add(spec);
      }
    };
    walk(resolve(dirname(fileURLToPath(import.meta.url)), "index.ts"));
    expect([...seen].some((file) => file.endsWith("workload-identity.ts"))).toBe(false);
    expect(
      [...external].filter(
        (spec) => spec.startsWith("node:") || spec.includes("connect-node") || spec.includes("spiffe"),
      ),
    ).toEqual([]);
  });
});
