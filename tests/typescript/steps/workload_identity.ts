// SPDX-License-Identifier: MIT
/**
 * `tests/features/connect_workload_identity.feature`, answered through the
 * Node entry of `@brefwiz/api-bones-connect`, where workload identity ships.
 */

import assert from "node:assert/strict";
import { createPrivateKey, X509Certificate } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { access } from "node:fs/promises";
import { createServer, type Server } from "node:http";
import type { AddressInfo, Socket } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { After, Given, Then, When, World } from "@cucumber/cucumber";
import { INITIAL_FETCH_TIMEOUT_ENV, WORKLOAD_API_SOCKET_DEFAULT, type PeerPathRule } from "@brefwiz/spiffe-client";

import {
  type ClosableTransport,
  WATCHER_ATTEMPTS,
  WorkloadIdentityError,
  configureNodeConnectTransport,
  clientTlsIdentityFor,
  workloadClientTlsIdentity,
  trustDomainOf,
  workloadPeerFor,
} from "@brefwiz/api-bones-connect/node";

import { authority, leaf, listen, type Authority, type Listener } from "./pinned_peer_fixtures.ts";
import { unaryMethodFixture, unaryPolicyDoc } from "./support.ts";

const admit: PeerPathRule = () => {};

type SvidWatcher = Parameters<typeof clientTlsIdentityFor>[0];

// Messages of an error and of each cause beneath it, outermost first.
function causeMessages(failure: unknown): string[] {
  const messages: string[] = [];
  for (let node = failure; node instanceof Error && messages.length < 10; node = node.cause) {
    messages.push(node.message);
  }
  return messages;
}

interface WorkloadWorld extends World {
  spiffeId: string;
  peer?: ReturnType<typeof workloadPeerFor>;
  peerFailure?: unknown;
  server?: Server;
  sockets: Set<Socket>;
  fetchWindowSet?: boolean;
  priorFetchWindow?: string;
  transport?: ClosableTransport;
  served?: Awaited<ReturnType<ClosableTransport["unary"]>>;
  fleetDir?: string;
  fleetCa?: Authority;
  fleetListener?: Listener;
  fleetCall?: { completed: boolean; failure?: unknown };
}

Given("a workload whose SVID names {string}", function (this: WorkloadWorld, spiffeId: string) {
  this.spiffeId = spiffeId;
});

Then("its trust domain is {string}", function (this: WorkloadWorld, expected: string) {
  assert.equal(trustDomainOf(this.spiffeId), expected === "none" ? null : expected);
});

Then(
  "deriving its TLS identity fails as {string}",
  function (this: WorkloadWorld, kind: string) {
    // Only `current()` is read before the trust domain is refused.
    const watcher = { current: () => ({ spiffeId: this.spiffeId }) } as unknown as SvidWatcher;
    let failure: unknown;
    try {
      clientTlsIdentityFor(watcher, admit);
    } catch (error) {
      failure = error;
    }
    assert.ok(failure instanceof WorkloadIdentityError, `expected a WorkloadIdentityError, got ${String(failure)}`);
    assert.equal(failure.kind, kind);
  },
);

When("it reaches the workload {string}", function (this: WorkloadWorld, service: string) {
  const watcher = { current: () => ({ spiffeId: this.spiffeId }) } as unknown as SvidWatcher;
  const peer = workloadPeerFor(watcher, service, admit);
  this.peer = peer;
});

When("it tries to reach the workload {string}", function (this: WorkloadWorld, service: string) {
  const watcher = { current: () => ({ spiffeId: this.spiffeId }) } as unknown as SvidWatcher;
  try {
    this.peer = workloadPeerFor(watcher, service, admit);
  } catch (error) {
    this.peerFailure = error;
  }
});

Then("the peer address is {string}", function (this: WorkloadWorld, url: string) {
  assert.equal(this.peer?.url, url);
});

Then("the peer identity is {string}", function (this: WorkloadWorld, spiffeId: string) {
  assert.equal(this.peer?.spiffeId, spiffeId);
});

Then("reaching the peer fails as {string}", function (this: WorkloadWorld, kind: string) {
  const failure = this.peerFailure;
  assert.ok(failure instanceof WorkloadIdentityError, `expected a WorkloadIdentityError, got ${String(failure)}`);
  assert.equal(failure.kind, kind);
});

Given("a process with no Workload API socket", async function (this: WorkloadWorld) {
  // The SPIFFE client dials its fixed default agent socket; only a missing
  // one (not an unreadable one) means this process is outside the mesh.
  await assert.rejects(access(WORKLOAD_API_SOCKET_DEFAULT), { code: "ENOENT" });
});

Given(
  "the workload's first SVID fetch window is {int} second",
  function (this: WorkloadWorld, seconds: number) {
    this.fetchWindowSet = true;
    this.priorFetchWindow = process.env[INITIAL_FETCH_TIMEOUT_ENV];
    process.env[INITIAL_FETCH_TIMEOUT_ENV] = String(seconds);
  },
);

// Budget: WATCHER_ATTEMPTS x (1 s window + 1 s max backoff) + 1.5 s gaps
// between attempts = 7.5 s, doubled for margin.
const REFUSAL_TIMEOUT_MS = (WATCHER_ATTEMPTS * (1_000 + 1_000) + 1_500) * 2;

When(
  "it asks for its workload TLS identity",
  { timeout: REFUSAL_TIMEOUT_MS },
  async function (this: WorkloadWorld) {
    try {
      await workloadClientTlsIdentity(admit);
    } catch (error) {
      this.peerFailure = error;
    }
  },
);

Then("asking fails as {string}", function (this: WorkloadWorld, kind: string) {
  const failure = this.peerFailure;
  assert.ok(failure instanceof WorkloadIdentityError, `expected a WorkloadIdentityError, got ${String(failure)}`);
  assert.equal(failure.kind, kind);
});

When(
  "it configures an in-fleet https transport without a peer path rule",
  async function (this: WorkloadWorld) {
    try {
      await configureNodeConnectTransport({ baseUrl: "https://ledger:8080", profile: "service" });
    } catch (error) {
      this.peerFailure = error;
    }
  },
);

Then("configuring fails because the peer path rule is missing", function (this: WorkloadWorld) {
  const failure = this.peerFailure;
  assert.ok(failure instanceof Error, `expected an Error, got ${String(failure)}`);
  assert.match(failure.message, /needs `pathRule`/);
});

Given(
  "a Node transport that has served a call over HTTP 1.1",
  async function (this: WorkloadWorld) {
    this.sockets = new Set();
    const server = createServer((_req, res) => {
      res.writeHead(200, { "content-type": "application/proto" });
      res.end();
    });
    server.on("connection", (socket) => this.sockets.add(socket));
    this.server = server;
    await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
    const { port } = server.address() as AddressInfo;
    const transport = await configureNodeConnectTransport({
      baseUrl: `http://127.0.0.1:${port}`,
      profile: "service",
      httpVersion: "1.1",
    });
    this.transport = transport;
    this.served = await transport.unary(
      unaryMethodFixture("pkg.v1.CloseService", "Get"),
      undefined,
      undefined,
      undefined,
      { value: "x" },
    );
    assert.ok(this.sockets.size > 0, "the call opened no connection");
  },
);

When("the transport is closed twice", function (this: WorkloadWorld) {
  this.transport?.close();
  this.transport?.close();
});

Then("every connection it opened is destroyed", async function (this: WorkloadWorld) {
  for (let i = 0; i < 50 && [...this.sockets].some((s) => !s.destroyed); i++) {
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
  assert.ok(this.served?.message, "the served call returned no response");
  assert.ok([...this.sockets].every((s) => s.destroyed));
});

// The in-fleet transport takes its identity from the Workload API, whose socket
// path is fixed, so the identity here is derived by the same function the
// transport calls and handed over as `tls`. The handshake itself is a real TLS
// exchange against a loopback server whose leaf a test CA issued.
Given(
  "a server in its trust domain presenting an SVID for {string}",
  async function (this: WorkloadWorld, serverId: string) {
    this.fleetDir = mkdtempSync(join(tmpdir(), "in-fleet-path-rule-"));
    const ca = authority(this.fleetDir);
    this.fleetCa = ca;
    this.fleetListener = await listen(leaf(this.fleetDir, ca, [serverId]));
  },
);

When(
  /^it calls that server over an in-fleet transport whose peer path rule (admits|refuses) every path$/,
  async function (this: WorkloadWorld, verdict: string) {
    assert.ok(this.fleetDir && this.fleetCa && this.fleetListener, "no server is listening");
    const own = leaf(this.fleetDir, this.fleetCa, [this.spiffeId]);
    const watcher = {
      current: () => ({
        spiffeId: this.spiffeId,
        leafDer: new X509Certificate(own.cert).raw,
        intermediatesDer: [],
        privateKeyDer: createPrivateKey(own.key).export({ type: "pkcs8", format: "der" }),
        bundleDer: new X509Certificate(readFileSync(this.fleetCa.cert, "utf8")).raw,
      }),
    } as unknown as SvidWatcher;
    const rule: PeerPathRule =
      verdict === "admits"
        ? () => {}
        : () => {
            throw { kind: "peer_chain_refused", message: "path not admitted" };
          };

    const transport = await configureNodeConnectTransport({
      baseUrl: `https://127.0.0.1:${this.fleetListener.port}`,
      profile: "service",
      policy: unaryPolicyDoc("/pkg.v1.Fleet/Get", "UNSPECIFIED"),
      httpVersion: "1.1",
      retry: { initialDelayMs: 0, maxDelayMs: 0 },
      defaultTimeoutMs: 8000,
      tls: clientTlsIdentityFor(watcher, rule),
    });
    try {
      await transport.unary(
        unaryMethodFixture("pkg.v1.Fleet", "Get"),
        undefined,
        undefined,
        undefined,
        { value: "x" },
      );
      this.fleetCall = { completed: true };
    } catch (failure) {
      this.fleetCall = { completed: false, failure };
    } finally {
      transport.close();
    }
  },
);

Then("the call completes", function (this: WorkloadWorld) {
  assert.ok(this.fleetCall, "no call was made");
  assert.ok(this.fleetCall.completed, `the call failed: ${String(this.fleetCall.failure)}`);
});

Then("the call is refused by the peer path rule", function (this: WorkloadWorld) {
  assert.ok(this.fleetCall, "no call was made");
  assert.ok(!this.fleetCall.completed, "the call succeeded, a refusal was expected");
  assert.match(causeMessages(this.fleetCall.failure).join(" <- "), /peer chain refused: path not admitted/);
});

After(function (this: WorkloadWorld) {
  this.fleetListener?.close();
  if (this.fleetDir !== undefined) rmSync(this.fleetDir, { recursive: true, force: true });
});

After(function (this: WorkloadWorld) {
  this.server?.close();
  if (this.fetchWindowSet) {
    if (this.priorFetchWindow === undefined) delete process.env[INITIAL_FETCH_TIMEOUT_ENV];
    else process.env[INITIAL_FETCH_TIMEOUT_ENV] = this.priorFetchWindow;
  }
});
