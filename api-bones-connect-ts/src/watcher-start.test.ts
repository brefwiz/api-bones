// SPDX-License-Identifier: MIT
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  WATCHER_ATTEMPTS,
  WorkloadIdentityError,
  workloadClientTlsIdentity,
  workloadPeer,
} from "./workload-identity.js";
import { configureNodeConnectTransport } from "./node.js";

// The 0.44 SPIFFE client takes the attempt count alone; the socket is fixed.
// A leading `undefined` would be read as the attempt count, so each call site
// is pinned to exactly one argument.
vi.mock("@brefwiz/spiffe-client", async () => {
  const actual = await vi.importActual<Record<string, unknown>>("@brefwiz/spiffe-client");
  return {
    ...actual,
    startWatcherSafe: vi.fn(),
    tlsClientConfigFromWatcher: vi.fn((_watcher: unknown, allowedServerIds: string[]) => ({
      cert: "CERT-PEM",
      key: "KEY-PEM",
      ca: "CA-PEM",
      allowedServerIds,
    })),
  };
});

const { startWatcherSafe, tlsClientConfigFromWatcher } = await import("@brefwiz/spiffe-client");
const startWatcher = vi.mocked(startWatcherSafe);

const OWN_ID = "spiffe://prod.example/o/acme/svc/bff";
const watcher = { current: () => ({ spiffeId: OWN_ID }) } as never;

beforeEach(() => {
  startWatcher.mockReset();
  vi.mocked(tlsClientConfigFromWatcher).mockClear();
});

describe("workloadClientTlsIdentity", () => {
  it("starts the watcher with the attempt count alone and refuses when none answers", async () => {
    startWatcher.mockResolvedValue(null);

    await expect(workloadClientTlsIdentity()).rejects.toMatchObject({
      name: "WorkloadIdentityError",
      kind: "workload_api_unavailable",
    });

    expect(startWatcher).toHaveBeenCalledTimes(1);
    expect(startWatcher.mock.calls[0]).toEqual([WATCHER_ATTEMPTS]);
  });

  it("starts the watcher with the attempt count alone and derives the identity from it", async () => {
    startWatcher.mockResolvedValue(watcher);

    await expect(workloadClientTlsIdentity()).resolves.toEqual({
      cert: "CERT-PEM",
      key: "KEY-PEM",
      ca: "CA-PEM",
    });

    expect(startWatcher.mock.calls).toEqual([[WATCHER_ATTEMPTS]]);
    expect(tlsClientConfigFromWatcher).toHaveBeenCalledWith(watcher, ["spiffe://prod.example/*"]);
  });
});

describe("workloadPeer", () => {
  it("starts the watcher with the attempt count alone and refuses when none answers", async () => {
    startWatcher.mockResolvedValue(null);

    await expect(workloadPeer("ledger")).rejects.toMatchObject({
      name: "WorkloadIdentityError",
      kind: "workload_api_unavailable",
    });

    expect(startWatcher.mock.calls).toEqual([[WATCHER_ATTEMPTS]]);
  });

  it("starts the watcher with the attempt count alone and derives the peer from its SVID", async () => {
    startWatcher.mockResolvedValue(watcher);

    const peer = await workloadPeer("ledger");

    expect(startWatcher.mock.calls).toEqual([[WATCHER_ATTEMPTS]]);
    expect(peer.url).toBe("https://ledger:8080");
    expect(peer.spiffeId).toBe("spiffe://prod.example/o/acme/svc/ledger");
  });
});

describe("configureNodeConnectTransport", () => {
  const baseUrl = "https://ledger.example:8080";

  it("starts the watcher with the attempt count alone and refuses an in-fleet transport when none answers", async () => {
    startWatcher.mockResolvedValue(null);

    const failure = await configureNodeConnectTransport({
      baseUrl,
      profile: "service",
      httpVersion: "1.1",
    }).catch((error: unknown) => error);

    expect(failure).toBeInstanceOf(WorkloadIdentityError);
    expect((failure as WorkloadIdentityError).kind).toBe("workload_api_unavailable");
    expect(startWatcher.mock.calls).toEqual([[WATCHER_ATTEMPTS]]);
  });

  it("starts the watcher with the attempt count alone and resolves the transport identity from it", async () => {
    startWatcher.mockResolvedValue(watcher);

    // h2 resolves the identity at composition, so the watcher's SVID is read here.
    const transport = await configureNodeConnectTransport({
      baseUrl,
      profile: "service",
      httpVersion: "2",
    });

    expect(startWatcher.mock.calls).toEqual([[WATCHER_ATTEMPTS]]);
    expect(tlsClientConfigFromWatcher).toHaveBeenCalledWith(watcher, ["spiffe://prod.example/*"]);
    transport.close();
  });

  it("does not start a watcher for a cleartext address", async () => {
    const transport = await configureNodeConnectTransport({
      baseUrl: "http://127.0.0.1:1",
      profile: "service",
      httpVersion: "1.1",
    });

    expect(startWatcher).not.toHaveBeenCalled();
    transport.close();
  });
});
