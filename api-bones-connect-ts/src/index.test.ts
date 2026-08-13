// SPDX-License-Identifier: MIT
import { Code, ConnectError } from "@connectrpc/connect";
import { describe, expect, it } from "vitest";

import {
  DEFAULT_BACKOFF,
  computeBackoffDelay,
  resolveBackoff,
} from "./backoff.js";
import {
  MAX_CONNECT_GET_URL_BYTES,
  eligibleBrowserReadPolicy,
  indexGeneratedPolicy,
} from "./policy.js";
import { configureNodeConnectTransport } from "./node.js";
import {
  RetryThrottle,
  isConnectionWriteFailure,
  isRetryableMethod,
  serverPushbackMs,
} from "./retry.js";
import type { GeneratedMethodPolicy } from "./policy.js";

const method = (over: Record<string, unknown> = {}) => ({
  rpc: "/svc.v1.S/Get",
  procedure: "unary",
  idempotency: "NO_SIDE_EFFECTS",
  browserCache: { scope: "PRIVATE", maxAgeSeconds: 60 },
  sensitivity: "NON_SENSITIVE",
  maxEncodedUrlBytes: 1024,
  ...over,
});

describe("backoff", () => {
  // Pinned by value, not shape. These numbers moved from core-ui and are
  // consumed by its subscription hooks; changing them while relocating the
  // module would silently retime every reconnect.
  it("keeps the defaults it was moved with", () => {
    expect(DEFAULT_BACKOFF).toEqual({
      initialDelayMs: 500,
      maxDelayMs: 30000,
      multiplier: 2,
    });
  });

  it("grows exponentially, caps, and floors with injected randomness", () => {
    const config = resolveBackoff();
    expect(computeBackoffDelay(0, config, () => 1)).toBe(500);
    expect(computeBackoffDelay(1, config, () => 1)).toBe(1000);
    // capped, not 500 * 2^10
    expect(computeBackoffDelay(10, config, () => 1)).toBe(30000);
  });

  it("treats negative attempts as zero rather than inverting the delay", () => {
    const config = resolveBackoff();
    expect(computeBackoffDelay(-5, config, () => 1)).toBe(500);
  });
});

describe("policy", () => {
  it("indexes a well-formed document", () => {
    const index = indexGeneratedPolicy({ schemaVersion: 1, methods: [method()] });
    expect(index.size).toBe(1);
  });

  // Fail-closed is the security property: a partially-parseable policy must
  // grant nothing, not the subset that happened to parse.
  it("fails closed to an empty index on a duplicate rpc", () => {
    const index = indexGeneratedPolicy({
      schemaVersion: 1,
      methods: [method(), method()],
    });
    expect(index.size).toBe(0);
  });

  it("fails closed on a malformed entry", () => {
    const index = indexGeneratedPolicy({
      schemaVersion: 1,
      methods: [method(), method({ browserCache: { scope: "BOGUS", maxAgeSeconds: 1 } })],
    });
    expect(index.size).toBe(0);
  });

  it("rejects a url budget above the protocol ceiling", () => {
    expect(eligibleBrowserReadPolicy(method({ maxEncodedUrlBytes: MAX_CONNECT_GET_URL_BYTES + 1 })))
      .toBeNull();
  });

  it("refuses browser reads for sensitive or side-effecting methods", () => {
    expect(eligibleBrowserReadPolicy(method({ sensitivity: "UNSPECIFIED" }))).toBeNull();
    expect(eligibleBrowserReadPolicy(method({ idempotency: "IDEMPOTENT" }))).toBeNull();
    expect(eligibleBrowserReadPolicy(method({ browserCache: { scope: "NO_STORE", maxAgeSeconds: 0 } })))
      .toBeNull();
  });
});

describe("node transport", () => {
  // A Node process cannot issue a browser request, so accepting "webapp" would
  // accept a composition-root mistake silently.
  it("rejects the webapp profile instead of downgrading it", () => {
    expect(() =>
      configureNodeConnectTransport({ baseUrl: "https://svc", profile: "webapp" }),
    ).toThrow(/must be "service"/);
  });

  it("builds a transport for the service profile", () => {
    const transport = configureNodeConnectTransport({
      baseUrl: "https://svc",
      profile: "service",
    });
    expect(transport).toHaveProperty("unary");
    expect(transport).toHaveProperty("stream");
  });

  it("accepts a well-formed policy", () => {
    expect(() =>
      configureNodeConnectTransport({
        baseUrl: "https://svc",
        profile: "service",
        policy: { schemaVersion: 1, methods: [method()] },
      }),
    ).not.toThrow();
  });

  // Services validate the same artifact webapps do, so a drifted policy is
  // caught wherever it is first composed rather than only in a browser.
  it("rejects a policy that fails closed while claiming methods", () => {
    expect(() =>
      configureNodeConnectTransport({
        baseUrl: "https://svc",
        profile: "service",
        policy: { schemaVersion: 1, methods: [method(), method()] },
      }),
    ).toThrow(/malformed or has duplicate RPC entries/);
  });
});

describe("retry eligibility", () => {
  const unary = (idempotency: GeneratedMethodPolicy["idempotency"]): GeneratedMethodPolicy => ({
    rpc: "/pkg.v1.Svc/Method",
    procedure: "unary",
    idempotency,
    browserCache: { scope: "NO_STORE", maxAgeSeconds: 0 },
    sensitivity: "UNSPECIFIED",
    maxEncodedUrlBytes: 4096,
  });

  it("retries only methods the proto declares safe", () => {
    expect(isRetryableMethod(unary("NO_SIDE_EFFECTS"))).toBe(true);
    expect(isRetryableMethod(unary("IDEMPOTENT"))).toBe(true);
    // The case that matters: an unannotated method is NOT replayed. Bootstrap
    // token redemption is single-use, and Connect can report Unavailable after
    // the server already accepted the call.
    expect(isRetryableMethod(unary("UNSPECIFIED"))).toBe(false);
  });

  it("fails closed with no policy entry or on streams", () => {
    expect(isRetryableMethod(undefined)).toBe(false);
    expect(isRetryableMethod({ ...unary("NO_SIDE_EFFECTS"), procedure: "streaming" })).toBe(false);
  });

  it("reads Retry-After in both delay-seconds and HTTP-date form", () => {
    const withHeader = (value: string): ConnectError => {
      const err = new ConnectError("throttled", Code.ResourceExhausted);
      err.metadata.set("retry-after", value);
      return err;
    };
    expect(serverPushbackMs(withHeader("2"))).toBe(2000);
    const now = Date.parse("2026-01-01T00:00:00Z");
    expect(serverPushbackMs(withHeader("Thu, 01 Jan 2026 00:00:30 GMT"), now)).toBe(30000);
    expect(serverPushbackMs(withHeader("nonsense"))).toBeNull();
    expect(serverPushbackMs(new ConnectError("x", Code.Unavailable))).toBeNull();
  });

  it("treats a failed connection write as never having reached the server", () => {
    // A pooled keep-alive socket the peer already closed: the write fails
    // before any byte ships, and both adapters report that as Internal.
    expect(isConnectionWriteFailure(new ConnectError("write EPIPE", Code.Internal))).toBe(true);
    expect(isConnectionWriteFailure(new ConnectError("socket hang up", Code.Internal))).toBe(true);
    expect(isConnectionWriteFailure(new ConnectError("read ECONNRESET", Code.Internal))).toBe(true);
  });

  it("does not mistake a server-side Internal for a connection write failure", () => {
    // The server ran the call and failed inside it. Replaying that is exactly
    // the blind retry this module exists to refuse.
    expect(isConnectionWriteFailure(new ConnectError("nil pointer", Code.Internal))).toBe(false);
    // Same words, but the peer answered — the code is what separates them.
    expect(isConnectionWriteFailure(new ConnectError("write EPIPE", Code.Unknown))).toBe(false);
  });

  it("suspends retries once the budget drains", () => {
    const throttle = new RetryThrottle({ maxTokens: 4, tokenRatio: 0.1 });
    expect(throttle.canRetry).toBe(true);
    throttle.recordFailure();
    throttle.recordFailure();
    // 4 -> 2, which is not above half of 4: a sustained outage stops retrying
    // instead of multiplying the load that caused it.
    expect(throttle.canRetry).toBe(false);
    for (let i = 0; i < 20; i++) throttle.recordSuccess();
    expect(throttle.canRetry).toBe(true);
  });
});

describe("package resolution", () => {
  // The published 0.2.0 emitted ESM with extensionless relative imports while
  // declaring no module type, so Node refused to resolve `./policy` from
  // `dist/web.js`. Every test here imports from `src`, which a bundler-style
  // resolver handles, so the suite stayed green while the artifact consumers
  // actually install was unusable. Assert the shape the emit depends on.
  it("declares the module type its emit requires", async () => {
    const pkg = await import("../package.json", { with: { type: "json" } });
    expect(pkg.default.type).toBe("module");
  });

  it("gives every relative specifier an explicit extension", async () => {
    const { readdirSync, readFileSync } = await import("node:fs");
    const { join } = await import("node:path");
    const dir = new URL("../src", import.meta.url).pathname;
    const offenders: string[] = [];
    for (const name of readdirSync(dir).filter((f) => f.endsWith(".ts"))) {
      const text = readFileSync(join(dir, name), "utf8");
      for (const line of text.split("\n")) {
        const trimmed = line.trimStart();
        if (!/^(import|export|\} from)\b/.test(trimmed)) continue;
        const m = /from\s+"(\.\.?\/[^"]+)"/.exec(trimmed);
        if (m && !/\.(js|json)$/.test(m[1])) offenders.push(`${name}: ${m[1]}`);
      }
    }
    expect(offenders).toEqual([]);
  });
});
