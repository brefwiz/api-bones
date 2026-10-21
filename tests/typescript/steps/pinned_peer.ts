// SPDX-License-Identifier: MIT
/**
 * `tests/features/connect_pinned_peer.feature`, answered through the Node entry
 * of `@brefwiz/api-bones-connect`: a real Connect unary call over
 * `configureNodeConnectTransport({ pinnedPeer })` to a real TLS listener on
 * loopback, issued by a test CA. Nothing about the handshake is stubbed, and no
 * verification lives here: a refusal is read from the error the call fails with.
 */

import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import type { DescMethodUnary } from "@bufbuild/protobuf";
import { EmptySchema, StringValueSchema } from "@bufbuild/protobuf/wkt";
import { After, Given, Then, When, World } from "@cucumber/cucumber";

import { configureNodeConnectTransport } from "@brefwiz/api-bones-connect/node";

import {
  authority,
  bareCertificate,
  leaf,
  listen,
  type Authority,
  type Leaf,
  type Listener,
} from "./pinned_peer_fixtures.ts";

const getMethod = {
  kind: "rpc",
  name: "Get",
  localName: "get",
  parent: { typeName: "pkg.v1.Svc" },
  methodKind: "unary",
  input: StringValueSchema,
  output: EmptySchema,
  idempotency: 0,
  deprecated: false,
} as unknown as DescMethodUnary<typeof StringValueSchema, typeof EmptySchema>;

const policy = {
  schemaVersion: 1,
  methods: [
    {
      rpc: "/pkg.v1.Svc/Get",
      procedure: "unary",
      idempotency: "UNSPECIFIED",
      browserCache: { scope: "PRIVATE", maxAgeSeconds: 60 },
      sensitivity: "NON_SENSITIVE",
      maxEncodedUrlBytes: 1024,
    },
  ],
};

/** A call that has not finished by now never will; fail it instead of holding the lane open. */
const CALL_TIMEOUT_MS = 8000;

interface PinnedPeerWorld extends World {
  dir?: string;
  testCa?: Authority;
  strangerCa?: Authority;
  bundle: string;
  listener?: Listener;
  error?: unknown;
  called: boolean;
}

function workDir(world: PinnedPeerWorld): string {
  world.dir ??= mkdtempSync(join(tmpdir(), "pinned-peer-"));
  return world.dir;
}

function testCa(world: PinnedPeerWorld): Authority {
  world.testCa ??= authority(workDir(world));
  return world.testCa;
}

function strangerCa(world: PinnedPeerWorld): Authority {
  world.strangerCa ??= authority(workDir(world));
  return world.strangerCa;
}

After(function (this: PinnedPeerWorld) {
  this.listener?.close();
  if (this.dir !== undefined) rmSync(this.dir, { recursive: true, force: true });
});

const SIGNER = "spiffe://prod.example/svc/signer";

// -- the bundle --------------------------------------------------------------

Given("a trust bundle holding the test CA", function (this: PinnedPeerWorld) {
  this.bundle = readFileSync(testCa(this).cert, "utf8");
});

Given("a trust bundle holding nothing", function (this: PinnedPeerWorld) {
  this.bundle = "";
});

Given("a trust bundle holding only a leaf certificate", function (this: PinnedPeerWorld) {
  this.bundle = leaf(workDir(this), testCa(this), [SIGNER]).cert;
});

Given("a trust bundle holding the test CA and a leaf certificate", function (this: PinnedPeerWorld) {
  const ca = readFileSync(testCa(this).cert, "utf8");
  this.bundle = ca + leaf(workDir(this), testCa(this), [SIGNER]).cert;
});

Given(
  "a trust bundle holding a certificate with no basicConstraints",
  function (this: PinnedPeerWorld) {
    this.bundle = bareCertificate(workDir(this));
  },
);

// -- the server --------------------------------------------------------------

async function present(world: PinnedPeerWorld, server: Leaf): Promise<void> {
  world.listener = await listen(server);
}

const issuerOf = (world: PinnedPeerWorld, who: string): Authority => {
  assert.ok(who === "the test CA" || who === "a stranger CA", `unknown issuer: ${who}`);
  return who === "the test CA" ? testCa(world) : strangerCa(world);
};

Given(
  /^a server presenting an SVID for "([^"]*)" issued by (the test CA|a stranger CA)$/,
  async function (this: PinnedPeerWorld, uri: string, issuer: string) {
    await present(this, leaf(workDir(this), issuerOf(this, issuer), [uri]));
  },
);

Given(
  /^a server presenting a CA certificate for "([^"]*)" issued by (the test CA|a stranger CA)$/,
  async function (this: PinnedPeerWorld, uri: string, issuer: string) {
    await present(this, leaf(workDir(this), issuerOf(this, issuer), [uri], true));
  },
);

Given(
  /^a server presenting a certificate with no URI SAN issued by (the test CA|a stranger CA)$/,
  async function (this: PinnedPeerWorld, issuer: string) {
    await present(this, leaf(workDir(this), issuerOf(this, issuer), []));
  },
);

Given(
  /^a server presenting a certificate with the URI SANs "([^"]*)" issued by (the test CA|a stranger CA)$/,
  async function (this: PinnedPeerWorld, uris: string, issuer: string) {
    await present(this, leaf(workDir(this), issuerOf(this, issuer), uris.split(",")));
  },
);

// -- the caller --------------------------------------------------------------

function pinnedOptions(world: PinnedPeerWorld, peer: string, baseUrl: string) {
  return {
    baseUrl,
    profile: "service" as const,
    policy,
    httpVersion: "1.1" as const,
    retry: { initialDelayMs: 0, maxDelayMs: 0 },
    pinnedPeer: { bundle: world.bundle, peer },
  };
}

When("a caller pinning {string} connects", async function (this: PinnedPeerWorld, peer: string) {
  assert.ok(this.listener, "no server is listening");
  this.called = true;
  const timeout = new Promise<never>((_, reject) => {
    setTimeout(() => reject(new Error(`no answer within ${CALL_TIMEOUT_MS}ms`)), CALL_TIMEOUT_MS).unref();
  });
  try {
    const transport = await configureNodeConnectTransport(
      pinnedOptions(this, peer, `https://127.0.0.1:${this.listener.port}`),
    );
    await Promise.race([
      transport.unary(getMethod, undefined, undefined, undefined, { value: "x" }),
      timeout,
    ]);
  } catch (error) {
    this.error = error;
  }
});

When(
  "a caller pinning {string} is built from that bundle",
  async function (this: PinnedPeerWorld, peer: string) {
    try {
      await configureNodeConnectTransport(pinnedOptions(this, peer, "https://signer.example"));
    } catch (error) {
      this.error = error;
    }
  },
);

When(
  "a caller pinning {string} is asked to call the cleartext address {string}",
  async function (this: PinnedPeerWorld, peer: string, url: string) {
    try {
      await configureNodeConnectTransport(pinnedOptions(this, peer, url));
    } catch (error) {
      this.error = error;
    }
  },
);

// -- the verdicts ------------------------------------------------------------

/** Every error from `error` down its `cause` chain. */
function chain(error: unknown): Array<Error & { kind?: string; code?: string }> {
  const out: Array<Error & { kind?: string; code?: string }> = [];
  for (let node = error; node instanceof Error && out.length < 10; node = node.cause) out.push(node);
  return out;
}

/** The SPIFFE client's typed refusal anywhere in the chain, by its `kind`. */
function spiffeKind(error: unknown): string | undefined {
  return chain(error).find((e) => e.name === "SpiffeError")?.kind;
}

Then("the connection is established", function (this: PinnedPeerWorld) {
  assert.ok(this.called, "no call was made");
  assert.equal(this.error, undefined, `the call failed: ${String(this.error)}`);
});

Then(
  "the server completed its handshake having seen no client certificate",
  function (this: PinnedPeerWorld) {
    assert.ok(this.listener, "no server is listening");
    assert.equal(this.listener.handshakes(), 1, "the server completed no handshake");
    assert.equal(this.listener.sawClientCert(), false, "the server saw a client certificate");
  },
);

function refused(world: PinnedPeerWorld): unknown {
  assert.ok(world.called, "no call was made");
  assert.ok(world.error, "the call succeeded, a refusal was expected");
  return world.error;
}

Then("the connection is refused for a wrong identity", function (this: PinnedPeerWorld) {
  assert.equal(spiffeKind(refused(this)), "peer_not_allowed", String(this.error));
});

Then("the connection is refused for a non-SVID leaf", function (this: PinnedPeerWorld) {
  assert.equal(spiffeKind(refused(this)), "invalid_svid", String(this.error));
});

// The chain refusal is the TLS stack's own: no SPIFFE refusal in the chain, and
// OpenSSL names an issuer it could not reach.
Then("the connection is refused for an untrusted chain", function (this: PinnedPeerWorld) {
  const error = refused(this);
  assert.equal(spiffeKind(error), undefined, `a SPIFFE refusal, not a chain one: ${String(error)}`);
  const codes = chain(error).map((e) => e.code);
  assert.ok(
    codes.some((code) =>
      [
        "UNABLE_TO_VERIFY_LEAF_SIGNATURE",
        "UNABLE_TO_GET_ISSUER_CERT",
        "UNABLE_TO_GET_ISSUER_CERT_LOCALLY",
        "SELF_SIGNED_CERT_IN_CHAIN",
      ].includes(code ?? ""),
    ),
    `expected an unknown-issuer code , got ${JSON.stringify(codes)}: ${String(error)}`,
  );
});

Then("it is refused as an invalid trust bundle", function (this: PinnedPeerWorld) {
  assert.equal(spiffeKind(this.error), "invalid_trust_bundle", String(this.error));
});

Then("it is refused as an invalid peer id", function (this: PinnedPeerWorld) {
  assert.equal(spiffeKind(this.error), "invalid_peer_id", String(this.error));
});

// The refusal is the transport's own up-front error, thrown while configuring and
// not a SPIFFE verdict; it carries no code, so its subject is the only handle.
Then("it is refused as a cleartext address", function (this: PinnedPeerWorld) {
  assert.ok(this.error instanceof Error, "a cleartext address must be refused");
  assert.equal(spiffeKind(this.error), undefined, String(this.error));
  assert.match(this.error.message, /`pinnedPeer` needs an https:\/\/ baseUrl/);
});
