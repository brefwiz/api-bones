// SPDX-License-Identifier: MIT
/**
 * `tests/features/connect_workload_identity.feature`, answered through the
 * Node entry of `@brefwiz/api-bones-connect`, where workload identity ships.
 */

import assert from "node:assert/strict";
import { createServer, type Server } from "node:http";
import type { AddressInfo, Socket } from "node:net";

import { After, Given, Then, When, World } from "@cucumber/cucumber";

import {
  type ClosableTransport,
  WorkloadIdentityError,
  configureNodeConnectTransport,
  clientTlsIdentityFor,
  trustDomainOf,
  workloadPeerFor,
} from "@brefwiz/api-bones-connect/node";

import { unaryMethodFixture } from "./support.ts";
import { createPackedConsumerProof } from "./packed_consumer_support.ts";

type SvidWatcher = Parameters<typeof clientTlsIdentityFor>[0];

interface WorkloadWorld extends World {
  spiffeId: string;
  peer?: ReturnType<typeof workloadPeerFor>;
  peerFailure?: unknown;
  server?: Server;
  sockets: Set<Socket>;
  transport?: ClosableTransport;
  served?: Awaited<ReturnType<ClosableTransport["unary"]>>;
}

Given(
  "a workload whose SVID names {string}",
  function (this: WorkloadWorld, spiffeId: string) {
    this.spiffeId = spiffeId;
  },
);

Then(
  "its trust domain is {string}",
  function (this: WorkloadWorld, expected: string) {
    assert.equal(
      trustDomainOf(this.spiffeId),
      expected === "none" ? null : expected,
    );
  },
);

Then(
  "deriving its TLS identity fails as {string}",
  function (this: WorkloadWorld, kind: string) {
    // Only `current()` is read before the trust domain is refused.
    const watcher = {
      current: () => ({ spiffeId: this.spiffeId }),
    } as unknown as SvidWatcher;
    let failure: unknown;
    try {
      clientTlsIdentityFor(watcher);
    } catch (error) {
      failure = error;
    }
    assert.ok(
      failure instanceof WorkloadIdentityError,
      `expected a WorkloadIdentityError, got ${String(failure)}`,
    );
    assert.equal(failure.kind, kind);
  },
);

When(
  "it reaches the workload {string}",
  function (this: WorkloadWorld, service: string) {
    const watcher = {
      current: () => ({ spiffeId: this.spiffeId }),
    } as unknown as SvidWatcher;
    const peer = workloadPeerFor(watcher, service);
    this.peer = peer;
  },
);

When(
  "it tries to reach the workload {string}",
  function (this: WorkloadWorld, service: string) {
    const watcher = {
      current: () => ({ spiffeId: this.spiffeId }),
    } as unknown as SvidWatcher;
    try {
      this.peer = workloadPeerFor(watcher, service);
    } catch (error) {
      this.peerFailure = error;
    }
  },
);

Then(
  "the peer address is {string}",
  function (this: WorkloadWorld, url: string) {
    assert.equal(this.peer?.url, url);
  },
);

Then(
  "the peer identity is {string}",
  function (this: WorkloadWorld, spiffeId: string) {
    assert.equal(this.peer?.spiffeId, spiffeId);
  },
);

Then(
  "reaching the peer fails as {string}",
  function (this: WorkloadWorld, kind: string) {
    const failure = this.peerFailure;
    assert.ok(
      failure instanceof WorkloadIdentityError,
      `expected a WorkloadIdentityError, got ${String(failure)}`,
    );
    assert.equal(failure.kind, kind);
  },
);

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
    await new Promise<void>((resolve) =>
      server.listen(0, "127.0.0.1", resolve),
    );
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

Then(
  "every connection it opened is destroyed",
  async function (this: WorkloadWorld) {
    for (
      let i = 0;
      i < 50 && [...this.sockets].some((s) => !s.destroyed);
      i++
    ) {
      await new Promise((resolve) => setTimeout(resolve, 20));
    }
    assert.ok(this.served?.message, "the served call returned no response");
    assert.ok([...this.sockets].every((s) => s.destroyed));
  },
);

After(function (this: WorkloadWorld) {
  this.server?.close();
});

interface PackageWorld extends World {
  packedConsumer?: ReturnType<typeof createPackedConsumerProof>;
}

Given(
  "the published transport package installed in isolated consumers",
  { timeout: 120_000 },
  function (this: PackageWorld) {
    this.packedConsumer = createPackedConsumerProof();
  },
);
Then(
  "the neutral entry works without adapters and selected missing adapters are refused",
  { timeout: 180_000 },
  function (this: PackageWorld) {
    assert.ok(this.packedConsumer);
    this.packedConsumer.proveNeutral();
  },
);
Then(
  "the installed {string} adapters load their public entries",
  { timeout: 180_000 },
  function (this: PackageWorld, entries: string) {
    assert.ok(this.packedConsumer);
    this.packedConsumer.proveSelected(entries.split(","));
  },
);
Then(
  "the incompatible {string} adapter is refused by strict resolution",
  { timeout: 180_000 },
  function (this: PackageWorld, entry: string) {
    assert.ok(this.packedConsumer);
    this.packedConsumer.proveIncompatible(entry);
  },
);
Then(
  "a linked SDK resolves the transport within its installed dependency boundary",
  { timeout: 360_000 },
  function (this: PackageWorld) {
    assert.ok(this.packedConsumer);
    this.packedConsumer.proveLinked();
  },
);
After({ tags: "@packed-package" }, function (this: PackageWorld) {
  this.packedConsumer?.close();
});
