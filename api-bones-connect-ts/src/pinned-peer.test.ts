// SPDX-License-Identifier: MIT
//
// The pin itself (chain, SVID leaf shape, identity match, session scope) is
// the SPIFFE client's and is tested there. These tests cover only how the
// Node transport composes it. The handshake behaviour is the
// `connect_pinned_peer` contract, answered over real loopback TLS.
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { pinnedPeerTlsOptions } from "@brefwiz/spiffe-client";
import { afterAll, describe, expect, it } from "vitest";

import { authority } from "../../tests/typescript/steps/pinned_peer_fixtures.ts";
import { configureNodeConnectTransport } from "./node.js";
import { resolveTlsIdentity } from "./node-diagnostics.js";

const PEER = "spiffe://prod.example/svc/signer";

const dir = mkdtempSync(join(tmpdir(), "pinned-peer-"));
afterAll(() => rmSync(dir, { recursive: true, force: true }));

const bundle = readFileSync(authority(dir).cert, "utf8");
const base = { profile: "service" as const, baseUrl: "https://signer.example" };

describe("configureNodeConnectTransport pinnedPeer", () => {
  it("is exclusive of tls", async () => {
    await expect(
      configureNodeConnectTransport({ ...base, tls: {}, pinnedPeer: { bundle, peer: PEER } }),
    ).rejects.toThrow(/exclusive/);
  });

  it("needs https", async () => {
    await expect(
      configureNodeConnectTransport({
        ...base,
        baseUrl: "http://signer.example",
        pinnedPeer: { bundle, peer: PEER },
      }),
    ).rejects.toThrow(/https/);
  });

  it("surfaces the SPIFFE client's refusals before any connection", async () => {
    await expect(
      configureNodeConnectTransport({
        ...base,
        pinnedPeer: { bundle, peer: "spiffe://prod.example/svc/*" },
      }),
    ).rejects.toMatchObject({ kind: "invalid_peer_id" });
    await expect(
      configureNodeConnectTransport({ ...base, pinnedPeer: { bundle: "", peer: PEER } }),
    ).rejects.toMatchObject({ kind: "invalid_trust_bundle" });
  });

  it("builds a transport without reading the Workload API", async () => {
    const transport = await configureNodeConnectTransport({
      ...base,
      pinnedPeer: { bundle, peer: PEER },
    });
    expect(transport).toHaveProperty("unary");
  });
});

describe("the connection options of a pin", () => {
  it("reach a connection with their session scope and TLS floor intact", () => {
    const options = pinnedPeerTlsOptions({ bundle, peer: PEER });
    expect(resolveTlsIdentity(options)).toBe(options);
    expect(options.sessionIdContext).toBeDefined();
    expect(options.minVersion).toBe("TLSv1.2");
  });
});
