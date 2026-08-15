// SPDX-License-Identifier: MIT
//
// Driven against a real HTTP server on loopback. The whole point of this module
// is what Node's socket pool does under a peer close, and a mocked agent would
// assert our own idea of that rather than the runtime's.

import http from "node:http";
import type { AddressInfo } from "node:net";
import { Code, ConnectError } from "@connectrpc/connect";
import { afterEach, describe, expect, it } from "vitest";

import {
  ConnectionFactsRecorder,
  createDiagnosticAgent,
  formatConnectionFacts,
  makeConnectionDiagnosticsInterceptor,
  resolveTlsIdentity,
  type ConnectionFacts,
} from "./node-diagnostics.js";

const servers: http.Server[] = [];

afterEach(async () => {
  await Promise.all(
    servers.splice(0).map((s) => new Promise<void>((r) => s.close(() => r()))),
  );
});

async function listen(handler: http.RequestListener): Promise<number> {
  const server = http.createServer(handler);
  servers.push(server);
  await new Promise<void>((r) => server.listen(0, r));
  return (server.address() as AddressInfo).port;
}

function post(agent: http.Agent, port: number, body: string): Promise<number> {
  return new Promise((resolve, reject) => {
    const req = http.request(
      { host: "127.0.0.1", port, method: "POST", path: "/rpc", agent },
      (res) => {
        res.resume();
        res.on("end", () => resolve(res.statusCode ?? 0));
      },
    );
    req.on("error", reject);
    req.end(body);
  });
}

const facts = (over: Partial<ConnectionFacts> = {}): ConnectionFacts => ({
  socketId: 1,
  authority: "origin:8443",
  reused: true,
  priorRequests: 2,
  ageAtAssignMs: 5_000,
  ageAtFailureMs: 5_001,
  idleBeforeAssignMs: 4_900,
  finBeforeFailureMs: 3,
  serverConnectionHeader: "keep-alive",
  serverKeepAliveHeader: "timeout=5",
  bytesWritten: 148,
  bytesRead: 0,
  ...over,
});

describe("formatConnectionFacts", () => {
  it("names a stale pooled socket and when the peer hung up", () => {
    expect(formatConnectionFacts(facts())).toBe(
      "socket=1, authority=origin:8443, reused after 2 request(s), age=5001ms, " +
        "idle-before-write=4900ms, peer FIN 3ms before the write failed, " +
        "server keep-alive: timeout=5, bytes w/r=148/0",
    );
  });

  it("distinguishes a fresh connection with no FIN observed", () => {
    const line = formatConnectionFacts(
      facts({ reused: false, priorRequests: 0, idleBeforeAssignMs: null, finBeforeFailureMs: null }),
    );
    expect(line).toContain("freshly connected");
    expect(line).toContain("no peer FIN observed");
    expect(line).not.toContain("idle-before-write");
  });

  it("falls back to the Connection header when no Keep-Alive is advertised", () => {
    expect(formatConnectionFacts(facts({ serverKeepAliveHeader: null }))).toContain(
      "server connection: keep-alive",
    );
  });
});

describe("ConnectionFactsRecorder", () => {
  it("hands back the most recent facts and only once", () => {
    const recorder = new ConnectionFactsRecorder();
    const since = Date.now();
    recorder.record(facts({ socketId: 1 }));
    recorder.record(facts({ socketId: 2 }));
    expect(recorder.take(since)?.socketId).toBe(2);
    expect(recorder.take(since)?.socketId).toBe(1);
    expect(recorder.take(since)).toBeNull();
  });

  it("refuses facts recorded before the call that is asking", () => {
    const recorder = new ConnectionFactsRecorder();
    recorder.record(facts());
    expect(recorder.take(Date.now() + 1_000)).toBeNull();
  });
});

describe("createDiagnosticAgent", () => {
  it("reports a failure on a pooled socket as reused, with the prior count", async () => {
    const recorder = new ConnectionFactsRecorder();
    const agent = createDiagnosticAgent(false, recorder);
    const since = Date.now();
    let seen = 0;
    // First call succeeds and leaves the socket in the pool with a keep-alive
    // policy on it; the second is killed mid-flight, which is the shape this
    // module exists to explain.
    const port = await listen((req, res) => {
      req.resume();
      seen += 1;
      if (seen === 1) {
        res.setHeader("keep-alive", "timeout=5");
        res.end("ok");
        return;
      }
      req.socket.destroy();
    });

    expect(await post(agent, port, "a")).toBe(200);
    await expect(post(agent, port, "b")).rejects.toThrow();
    await new Promise<void>((r) => setImmediate(r));

    const recorded = recorder.take(since);
    expect(recorded?.reused).toBe(true);
    expect(recorded?.priorRequests).toBe(1);
    expect(recorded?.idleBeforeAssignMs).not.toBeNull();
    expect(recorded?.serverKeepAliveHeader).toBe("timeout=5");
    agent.destroy();
  });

  it("records a peer close as connection facts", async () => {
    const recorder = new ConnectionFactsRecorder();
    const agent = createDiagnosticAgent(false, recorder);
    const since = Date.now();
    const port = await listen((req) => {
      req.socket.destroy();
    });

    await expect(post(agent, port, "x".repeat(64))).rejects.toThrow();
    await new Promise<void>((r) => setImmediate(r));

    const recorded = recorder.take(since);
    expect(recorded).not.toBeNull();
    expect(recorded?.reused).toBe(false);
    expect(recorded?.priorRequests).toBe(0);
    expect(recorded?.bytesWritten).toBeGreaterThan(0);
    agent.destroy();
  });
});

describe("resolveTlsIdentity", () => {
  it("passes static material through", () => {
    const material = { ca: "root", rejectUnauthorized: true };
    expect(resolveTlsIdentity(material)).toEqual(material);
  });

  it("calls a provider every time, so a rotated identity is picked up", () => {
    let generation = 0;
    const provider = (): { cert: string } => ({ cert: `svid-${(generation += 1)}` });
    expect(resolveTlsIdentity(provider)).toEqual({ cert: "svid-1" });
    expect(resolveTlsIdentity(provider)).toEqual({ cert: "svid-2" });
  });

  it("resolves an absent identity to no material rather than undefined", () => {
    expect(resolveTlsIdentity(undefined)).toEqual({});
  });
});

describe("createDiagnosticAgent tls", () => {
  it("leaves a plaintext agent's identity unresolved", async () => {
    const recorder = new ConnectionFactsRecorder();
    let resolved = 0;
    // A plaintext agent never reaches TLS. Resolving there would hand cert
    // material to a connection that cannot present it and quietly mask the
    // misconfiguration.
    const agent = createDiagnosticAgent(false, recorder, () => {
      resolved += 1;
      return {};
    });
    const port = await listen((req, res) => {
      req.resume();
      res.end("ok");
    });
    expect(await post(agent, port, "a")).toBe(200);
    expect(resolved).toBe(0);
    agent.destroy();
  });
});

describe("makeConnectionDiagnosticsInterceptor", () => {
  const unary = { stream: false } as never;

  it("appends the facts to a connection-write failure", async () => {
    const recorder = new ConnectionFactsRecorder();
    const interceptor = makeConnectionDiagnosticsInterceptor(recorder);
    const next = async (): Promise<never> => {
      recorder.record(facts());
      throw new ConnectError("write EPIPE", Code.Internal);
    };

    await expect(interceptor(next)(unary)).rejects.toThrow(
      /write EPIPE \(socket=1, .*peer FIN 3ms before the write failed/,
    );
  });

  it("leaves the code and cause intact", async () => {
    const recorder = new ConnectionFactsRecorder();
    const cause = new Error("underlying");
    const next = async (): Promise<never> => {
      recorder.record(facts());
      throw new ConnectError("write EPIPE", Code.Internal, undefined, undefined, cause);
    };

    const err = await makeConnectionDiagnosticsInterceptor(recorder)(next)(unary).catch(
      (e: unknown) => e as ConnectError,
    );
    expect(err.code).toBe(Code.Internal);
    expect(err.cause).toBe(cause);
  });

  it("passes every other failure through untouched", async () => {
    const recorder = new ConnectionFactsRecorder();
    const original = new ConnectError("nope", Code.PermissionDenied);
    const next = async (): Promise<never> => {
      throw original;
    };
    await expect(makeConnectionDiagnosticsInterceptor(recorder)(next)(unary)).rejects.toBe(
      original,
    );
  });

  it("leaves the failure alone when no facts were recorded for it", async () => {
    const recorder = new ConnectionFactsRecorder();
    const original = new ConnectError("write EPIPE", Code.Internal);
    const next = async (): Promise<never> => {
      throw original;
    };
    await expect(makeConnectionDiagnosticsInterceptor(recorder)(next)(unary)).rejects.toBe(
      original,
    );
  });
});
