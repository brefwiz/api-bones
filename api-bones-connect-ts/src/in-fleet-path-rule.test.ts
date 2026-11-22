// SPDX-License-Identifier: MIT
//
// An in-fleet transport's server verification, exercised end to end: a real
// TLS handshake against a loopback server whose leaf a test CA issued, over
// both HTTP versions. Only the Workload API socket is substituted (its path is
// fixed); the verification that decides the handshake is the SPIFFE client's
// own, so a path rule the caller injected must be able to refuse the peer.
import { createPrivateKey, X509Certificate } from "node:crypto";
import http2 from "node:http2";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";

import type { PeerPathRule } from "@brefwiz/spiffe-client";
import { afterAll, beforeEach, describe, expect, it, vi } from "vitest";

import { authority, leaf, type Leaf } from "../../tests/typescript/steps/pinned_peer_fixtures.ts";
import { unaryMethodFixture, unaryPolicyDoc } from "../../tests/typescript/steps/support.ts";

import { configureNodeConnectTransport } from "./node.js";

vi.mock("@brefwiz/spiffe-client", async () => {
  const actual = await vi.importActual<Record<string, unknown>>("@brefwiz/spiffe-client");
  return { ...actual, startWatcherSafe: vi.fn() };
});

const { startWatcherSafe } = await import("@brefwiz/spiffe-client");

const dir = mkdtempSync(join(tmpdir(), "in-fleet-path-rule-"));
const ca = authority(dir);
const caDer = new X509Certificate(readFileSync(ca.cert, "utf8")).raw;

const OWN = "spiffe://prod.example/o/acme/svc/bff";
// A managed peer: the same multi-segment shape the fleet issues.
const LEDGER = "spiffe://prod.example/o/acme/svc/ledger";
const own = leaf(dir, ca, [OWN]);
const watcher = {
  current: () => ({
    spiffeId: OWN,
    leafDer: new X509Certificate(own.cert).raw,
    intermediatesDer: [],
    privateKeyDer: createPrivateKey(own.key).export({ type: "pkcs8", format: "der" }),
    bundleDer: caDer,
  }),
} as never;

const admit: PeerPathRule = () => {};
const refuse: PeerPathRule = () => {
  throw { kind: "peer_chain_refused", message: "path not admitted" };
};

const method = unaryMethodFixture("pkg.v1.Svc", "Get");
const policy = unaryPolicyDoc("/pkg.v1.Svc/Get", "UNSPECIFIED");

/** A TLS server speaking h2 and HTTP/1.1 whose leaf names `spiffeId`. */
async function serve(spiffeId: string): Promise<{ port: number; close: () => void }> {
  const server: Leaf = leaf(dir, ca, [spiffeId]);
  const https = http2.createSecureServer({ ...server, allowHTTP1: true }, (_req, res) => {
    res.writeHead(200, { "content-type": "application/proto" });
    res.end();
  });
  https.on("tlsClientError", () => {});
  await new Promise<void>((resolve) => https.listen(0, "127.0.0.1", resolve));
  return {
    port: (https.address() as AddressInfo).port,
    close: () => {
      https.close();
    },
  };
}

/** The error and every `cause` beneath it, flattened to the text a reader sees. */
function failureText(error: unknown): string {
  const parts: string[] = [];
  for (let node = error; node instanceof Error && parts.length < 10; node = node.cause) {
    parts.push(node.message);
  }
  return parts.join(" <- ");
}

async function call(
  httpVersion: "1.1" | "2",
  port: number,
  pathRule: PeerPathRule,
): Promise<unknown> {
  const transport = await configureNodeConnectTransport({
    baseUrl: `https://127.0.0.1:${port}`,
    profile: "service",
    policy,
    httpVersion,
    retry: { initialDelayMs: 0, maxDelayMs: 0 },
    defaultTimeoutMs: 8000,
    pathRule,
  });
  try {
    await transport.unary(method, undefined, undefined, undefined, { value: "x" });
    return undefined;
  } catch (error) {
    return error;
  } finally {
    transport.close();
  }
}

beforeEach(() => {
  vi.mocked(startWatcherSafe).mockResolvedValue(watcher);
});

afterAll(() => rmSync(dir, { recursive: true, force: true }));

describe.each(["1.1", "2"] as const)("an in-fleet transport over HTTP %s", (httpVersion) => {
  it("completes the handshake with a peer the caller's path rule admits", async () => {
    const server = await serve(LEDGER);
    try {
      expect(await call(httpVersion, server.port, admit)).toBeUndefined();
    } finally {
      server.close();
    }
  });

  it("refuses a peer the caller's path rule refuses", async () => {
    const server = await serve(LEDGER);
    try {
      expect(failureText(await call(httpVersion, server.port, refuse))).toContain(
        "peer chain refused: path not admitted",
      );
    } finally {
      server.close();
    }
  });

  it.each([
    "spiffe://evil.example/o/acme/svc/ledger",
    "spiffe://prod.example.evil/o/acme/svc/ledger",
    "spiffe://prod.example2/o/acme/svc/ledger",
  ])("refuses %s, outside the caller's trust domain, even when the rule admits it", async (peer) => {
    const server = await serve(peer);
    try {
      expect(failureText(await call(httpVersion, server.port, admit))).toContain(
        "is not permitted",
      );
    } finally {
      server.close();
    }
  });
});
