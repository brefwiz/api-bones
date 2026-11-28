// SPDX-License-Identifier: MIT
/**
 * The TypeScript answer to `tests/sdk-call/features/sdk_node_transport.feature`.
 *
 * Every peer here is a real `node:http2` server on loopback and every call
 * goes through the generated wrapper of the test service, which runs the wasm
 * core and its Node host exchange. The calls that need a trust anchor the
 * process cannot take at run time run in a child process started with
 * `NODE_EXTRA_CA_CERTS`, the way an operator sets it.
 */

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import * as http from "node:http";
import * as http2 from "node:http2";
import * as tls from "node:tls";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";

import { After, Given, Then, When } from "@cucumber/cucumber";
import { apiBonesSdk } from "@brefwiz/api-bones-test-service-sdk";

import { defineHostGlobal, restoreHost, withdrawExchange, withdrawRandomValues } from "./host_globals.ts";
import { settle } from "./sdk_call.ts";
import type { SdkCallWorld } from "./sdk_call.ts";

const SOCKET_VARIABLE = "SPIFFE_ENDPOINT_SOCKET";

/** What the scenario changed in the process and must give back. */
const scenario = {
  socket: undefined as { value: string | undefined } | undefined,
  servers: [] as (http2.Http2Server | http2.Http2SecureServer)[],
  sessions: [] as http2.ServerHttp2Session[],
  directory: undefined as string | undefined,
  /** Streams the peers saw and the RST_STREAM code each was closed with. */
  streams: [] as { code: number | undefined; closed: boolean }[],
  port: 0,
  connections: 0,
  clientCertificates: [] as (string | undefined)[],
  /** Node's default roots as they were before a Given added a peer's certificate. */
  roots: undefined as string[] | undefined,
  plain: [] as http.Server[],
  requests: 0,
  /** Gives `crypto` back to a host that lost its randomness mid-call. */
  restoreRandom: undefined as (() => void) | undefined,
};

After({ tags: "@node-transport" }, async function (this: SdkCallWorld) {
  if (scenario.socket !== undefined) {
    if (scenario.socket.value === undefined) delete process.env[SOCKET_VARIABLE];
    else process.env[SOCKET_VARIABLE] = scenario.socket.value;
    scenario.socket = undefined;
  }
  scenario.restoreRandom?.();
  scenario.restoreRandom = undefined;
  restoreHost();
  for (const server of scenario.plain.splice(0)) {
    server.closeAllConnections();
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
  scenario.requests = 0;
  for (const session of scenario.sessions.splice(0)) session.destroy();
  for (const server of scenario.servers.splice(0)) {
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
  if (scenario.directory !== undefined) rmSync(scenario.directory, { recursive: true, force: true });
  scenario.directory = undefined;
  scenario.streams = [];
  scenario.connections = 0;
  scenario.port = 0;
  scenario.clientCertificates = [];
  if (scenario.roots !== undefined) {
    (tls as unknown as { setDefaultCACertificates(roots: string[]): void }).setDefaultCACertificates(scenario.roots);
    scenario.roots = undefined;
  }
});

function holdSocketVariable(): void {
  scenario.socket ??= { value: process.env[SOCKET_VARIABLE] };
}

async function listen(server: http2.Http2Server | http2.Http2SecureServer): Promise<number> {
  scenario.servers.push(server);
  server.on("session", (session) => {
    scenario.sessions.push(session);
    session.on("error", () => undefined);
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  return (server.address() as AddressInfo).port;
}

/** A plaintext HTTP/2 peer that hands every stream to `onStream`; resolves to its port. */
async function h2cPeer(onStream: (stream: http2.ServerHttp2Stream) => void): Promise<number> {
  const server = http2.createServer();
  server.on("stream", (stream) => {
    stream.on("error", () => undefined);
    onStream(stream);
  });
  const port = await listen(server);
  scenario.port = port;
  return port;
}

Given("the SDK is opened at {string}", async function (this: SdkCallWorld, baseUrl: string) {
  this.opened = await apiBonesSdk(this.session(), { baseUrl, defaultPageSize: this.defaultPageSize });
});

Given("the host names no Workload API socket", function () {
  holdSocketVariable();
  delete process.env[SOCKET_VARIABLE];
});

Given("the host names a Workload API socket nothing listens on", function () {
  holdSocketVariable();
  scenario.directory = mkdtempSync(join(tmpdir(), "abx-spire-"));
  process.env[SOCKET_VARIABLE] = `unix://${join(scenario.directory, "agent.sock")}`;
});

Given("the host defines the global {string}", function (name: string) {
  defineHostGlobal(name);
});

Given("the host has no api-bones exchange", function () {
  withdrawExchange();
});

Given("a public HTTP\\/1.1 peer that answers every request with 404", async function (this: SdkCallWorld) {
  const server = http.createServer((_request, response) => {
    scenario.requests += 1;
    response.writeHead(404).end();
  });
  scenario.plain.push(server);
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const { port } = server.address() as AddressInfo;
  this.opened = await apiBonesSdk(this.session(), { baseUrl: `http://127.0.0.1:${port}`, defaultPageSize: this.defaultPageSize });
});

Then("the peer served {int} request(s)", function (count: number) {
  assert.equal(scenario.requests, count);
});

type Refusal = { emitter?: string; connectCode?: string };

const refusalOf = (world: SdkCallWorld): Refusal => {
  assert.ok(world.refusal, "the call was refused");
  return world.refusal as Refusal;
};

Then("the call is refused by the emitter {string}", function (this: SdkCallWorld, emitter: string) {
  assert.equal(refusalOf(this).emitter, emitter, `refused with ${String(this.refusal)}`);
});

Then("the call is refused with the Connect code {string}", function (this: SdkCallWorld, code: string) {
  assert.equal(refusalOf(this).connectCode, code, `refused with ${String(this.refusal)}`);
});

// --- a host with no randomness ---

/** The host loses `crypto.getRandomValues` after it opened the SDK; the scenario gives `crypto` back. */
Given("the host loses crypto.getRandomValues", function () {
  scenario.restoreRandom ??= withdrawRandomValues();
});

/**
 * The peer answers `unavailable` to its first request, and the host loses its
 * randomness the moment that request arrives, so a retry of the call would
 * need randomness the host no longer has.
 */
Given("a public peer that answers unavailable once and then the host loses its random source", async function (this: SdkCallWorld) {
  const port = await h2cPeer((stream) => {
    scenario.requests += 1;
    scenario.restoreRandom ??= withdrawRandomValues();
    stream.respond({ ":status": 503, "content-type": "application/json" });
    stream.end(JSON.stringify({ code: "unavailable", message: "try again" }));
  });
  this.opened = await apiBonesSdk(this.session(), { baseUrl: `http://127.0.0.1:${port}`, defaultPageSize: this.defaultPageSize });
});

// --- the public net: no client certificate ---

Given("a public https peer that asks every client for a certificate", async function (this: SdkCallWorld) {
  const directory = mkdtempSync(join(tmpdir(), "abx-public-"));
  scenario.directory = directory;
  execFileSync(
    "openssl",
    ["req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1", "-nodes", "-keyout", "peer.key", "-out", "peer.pem", "-subj", "/CN=peer", "-days", "2", "-addext", "subjectAltName=IP:127.0.0.1"],
    { cwd: directory, stdio: "pipe" },
  );
  const read = (name: string): string => readFileSync(join(directory, name), "utf8");
  const server = http2.createSecureServer({ key: read("peer.key"), cert: read("peer.pem"), requestCert: true, rejectUnauthorized: false });
  server.on("secureConnection", (socket) => {
    scenario.connections += 1;
    const presented = socket.getPeerCertificate().raw;
    scenario.clientCertificates.push(presented === undefined ? undefined : presented.toString("hex"));
  });
  server.on("stream", (stream) => {
    stream.on("error", () => undefined);
    stream.respond({ ":status": 404 });
    stream.end();
  });
  const port = await listen(server);
  scenario.port = port;
  // The exchange asks for Node's default roots, so the peer's certificate joins them for this scenario.
  const roots = tls as unknown as { getCACertificates(kind: string): string[]; setDefaultCACertificates(roots: string[]): void };
  scenario.roots = roots.getCACertificates("default");
  roots.setDefaultCACertificates([...scenario.roots, read("peer.pem")]);
  this.opened = await apiBonesSdk(this.session(), { baseUrl: `https://127.0.0.1:${port}`, defaultPageSize: this.defaultPageSize });
});

Then("the peer served one connection", function () {
  assert.equal(scenario.connections, 1);
});

Then("the peer saw no client certificate", function () {
  assert.deepEqual(scenario.clientCertificates, [undefined]);
});

// --- abort ---

Given("a public peer that never answers", async function (this: SdkCallWorld) {
  const port = await h2cPeer((stream) => {
    const seen = { code: undefined as number | undefined, closed: false };
    scenario.streams.push(seen);
    stream.once("close", () => {
      seen.code = stream.rstCode;
      seen.closed = true;
    });
  });
  this.opened = await apiBonesSdk(this.session(), { baseUrl: `http://127.0.0.1:${port}`, defaultPageSize: this.defaultPageSize });
});

Given("a read of the first page of the list from that peer is pending", async function (this: SdkCallWorld) {
  assert.ok(this.walk, "a Given opened the list");
  this.pending = settle(this.walk.next());
  for (let waited = 0; waited < 500 && scenario.streams.length === 0; waited += 1) await delay(10);
  assert.equal(scenario.streams.length, 1, "the read never reached the peer");
});

Then("the peer saw its stream reset with CANCEL", async function () {
  const [seen] = scenario.streams;
  assert.ok(seen, "the peer saw a stream");
  for (let waited = 0; waited < 500 && !seen.closed; waited += 1) await delay(10);
  assert.ok(seen.closed, "the peer's stream never closed");
  assert.equal(seen.code, http2.constants.NGHTTP2_CANCEL);
});

Then("the abandoned read gives no page", async function (this: SdkCallWorld) {
  assert.ok(this.pending, "a Given started the read");
  assert.ok(this.walk, "a Given opened the list");
  const outcome = await this.pending;
  assert.ok(!outcome.ok || outcome.value.done === true, "the abandoned read gave a page");
  assert.equal((await this.walk.next()).done, true);
});
