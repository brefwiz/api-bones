// SPDX-License-Identifier: MIT
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import * as http2 from "node:http2";
import type { AddressInfo, Socket } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { X509Certificate } from "node:crypto";
import * as tls from "node:tls";
import * as v8 from "node:v8";
import * as vm from "node:vm";
import { afterAll, afterEach, beforeAll, describe, expect, it } from "vitest";
import { EXCHANGE_SLOT, createNodeExchange, installNodeExchange, type ExchangeRequest, type NodeExchange } from "../src/node-exchange.js";

v8.setFlagsFromString("--expose-gc");

const { NGHTTP2_CANCEL } = http2.constants;
const encoder = new TextEncoder();

/** An h2c server that records its sessions and hands every stream to `handle`. */
interface Peer {
  readonly port: number;
  readonly sessions: http2.ServerHttp2Session[];
  readonly closed: () => number;
  stop(): Promise<void>;
}

async function listen(either: http2.Http2Server | http2.Http2SecureServer, where: number | string): Promise<Peer> {
  const server = either as http2.Http2Server;
  const sessions: http2.ServerHttp2Session[] = [];
  let closed = 0;
  server.on("session", (session) => {
    sessions.push(session);
    session.on("close", () => {
      closed += 1;
    });
    session.on("error", () => undefined);
  });
  await new Promise<void>((resolve) => (typeof where === "number" ? server.listen(where, "127.0.0.1", resolve) : server.listen(where, resolve)));
  const port = typeof where === "number" ? (server.address() as AddressInfo).port : 0;
  return {
    port,
    sessions,
    closed: () => closed,
    stop: () =>
      new Promise<void>((resolve) => {
        for (const session of sessions) session.destroy();
        server.close(() => resolve());
      }),
  };
}

function h2c(handle: (stream: http2.ServerHttp2Stream, headers: http2.IncomingHttpHeaders) => void): Promise<Peer> {
  const server = http2.createServer();
  server.on("stream", (stream, headers) => {
    stream.on("error", () => undefined);
    handle(stream, headers);
  });
  return listen(server, 0);
}

function request(port: number, overrides: Partial<ExchangeRequest> = {}): ExchangeRequest {
  return {
    connect: { tcp: { host: "127.0.0.1", port, servername: "localhost", tls: null } },
    owner: "module-a",
    generation: 0,
    method: "POST",
    path: "/pkg.Svc/Method",
    headers: [],
    body: new Uint8Array(),
    signal: new AbortController().signal,
    peerAdmissible: null,
    ...overrides,
  };
}

async function read(body: ReadableStream<Uint8Array>): Promise<string> {
  const decoder = new TextDecoder();
  let text = "";
  for await (const chunk of body as unknown as AsyncIterable<Uint8Array>) text += decoder.decode(chunk, { stream: true });
  return text;
}

/** The RST_STREAM code of the next stream the peer closes. */
function resetCode(): { watch(stream: http2.ServerHttp2Stream): void; code: Promise<number> } {
  let resolve: (code: number) => void = () => undefined;
  const code = new Promise<number>((done) => {
    resolve = done;
  });
  return { watch: (stream) => void stream.once("close", () => resolve(stream.rstCode)), code };
}

async function settled(condition: () => boolean): Promise<void> {
  for (let i = 0; i < 400 && !condition(); i += 1) await new Promise((resolve) => globalThis.setImmediate(resolve));
  expect(condition()).toBe(true);
}

let host: NodeExchange;
beforeAll(() => {
  host = createNodeExchange();
});
afterEach(() => {
  host.close();
});
afterAll(() => {
  host.close();
});

describe("the exchange over h2c", () => {
  it("carries method, path, headers and body, and returns the response bytes as sent", async () => {
    const seen: http2.IncomingHttpHeaders[] = [];
    const sent: string[] = [];
    const peer = await h2c((stream, headers) => {
      seen.push(headers);
      let text = "";
      stream.on("data", (chunk: Buffer) => {
        text += chunk.toString("utf8");
      });
      stream.on("end", () => {
        sent.push(text);
        stream.respond({ ":status": 201, "content-type": "application/proto", "x-reply": "yes" });
        stream.end("answer");
      });
    });
    try {
      const response = await host.exchange(
        request(peer.port, {
          headers: [
            ["content-type", "application/proto"],
            ["x-twice", "one"],
            ["x-twice", "two"],
          ],
          body: encoder.encode("question"),
        }),
      );
      expect(response.status).toBe(201);
      expect(response.headers.filter(([name]) => name.startsWith(":"))).toEqual([]);
      expect(response.headers).toContainEqual(["x-reply", "yes"]);
      expect(await read(response.body)).toBe("answer");
      expect(sent).toEqual(["question"]);
      expect(seen[0][":method"]).toBe("POST");
      expect(seen[0][":path"]).toBe("/pkg.Svc/Method");
      expect(seen[0]["x-twice"]).toBe("one, two");
      expect(seen[0]["accept-encoding"]).toBeUndefined();
    } finally {
      await peer.stop();
    }
  });

  it("serves every call of one (owner, connect, generation) from one session", async () => {
    const peer = await h2c((stream) => {
      stream.respond({ ":status": 200 });
      stream.end("ok");
    });
    try {
      for (let i = 0; i < 3; i += 1) await read((await host.exchange(request(peer.port, { generation: 1 }))).body);
      expect(peer.sessions.length).toBe(1);
    } finally {
      await peer.stop();
    }
  });

  it("does not share sessions between owners that use the same generation number", async () => {
    const peer = await h2c((stream) => {
      stream.respond({ ":status": 200 });
      stream.end("ok");
    });
    try {
      await read((await host.exchange(request(peer.port, { owner: "module-a", generation: 2 }))).body);
      await read((await host.exchange(request(peer.port, { owner: "module-b", generation: 2 }))).body);
      expect(peer.sessions.length).toBe(2);
    } finally {
      await peer.stop();
    }
  });

  it("compares generations only within one owner", async () => {
    const peer = await h2c((stream) => {
      stream.respond({ ":status": 200 });
      stream.end("ok");
    });
    try {
      // Module a has seen generation 5; module b's generation 1 is not older for b.
      await read((await host.exchange(request(peer.port, { owner: "module-a", generation: 5 }))).body);
      await read((await host.exchange(request(peer.port, { owner: "module-b", generation: 1 }))).body);
      // Both stay open, and b's next call reuses its session.
      await read((await host.exchange(request(peer.port, { owner: "module-b", generation: 1 }))).body);
      expect(peer.sessions.length).toBe(2);
      expect(peer.closed()).toBe(0);
      // Module b moving on closes only b's older session.
      await read((await host.exchange(request(peer.port, { owner: "module-b", generation: 2 }))).body);
      await settled(() => peer.closed() === 1);
      expect(peer.sessions.length).toBe(3);
      expect(peer.sessions[0].closed || peer.sessions[0].destroyed).toBe(false);
    } finally {
      await peer.stop();
    }
  });

  it("closes the older generation once its call finished and never serves a call from it", async () => {
    let release: () => void = () => undefined;
    const held = new Promise<void>((resolve) => {
      release = resolve;
    });
    const peer = await h2c((stream, headers) => {
      stream.respond({ ":status": 200 });
      stream.write("head;");
      if (headers["x-hold"] === "yes") void held.then(() => stream.end("tail"));
      else stream.end("only");
    });
    try {
      const first = await host.exchange(request(peer.port, { generation: 1, headers: [["x-hold", "yes"]] }));
      const second = await host.exchange(request(peer.port, { generation: 2 }));
      expect(peer.sessions.length).toBe(2);
      expect(await read(second.body)).toBe("head;only");
      // The call in flight finishes on its own generation's session.
      expect(peer.closed()).toBe(0);
      release();
      expect(await read(first.body)).toBe("head;tail");
      await settled(() => peer.closed() === 1);
      expect(peer.sessions[1].closed).toBe(false);
    } finally {
      await peer.stop();
    }
  });

  it("serves a call minted under an older generation from a session of its own", async () => {
    const peer = await h2c((stream) => {
      stream.respond({ ":status": 200 });
      stream.end("ok");
    });
    try {
      await read((await host.exchange(request(peer.port, { generation: 2 }))).body);
      await read((await host.exchange(request(peer.port, { generation: 1 }))).body);
      await settled(() => peer.closed() === 1);
      expect(peer.sessions.length).toBe(2);
      // The newer generation's session is untouched and still pooled.
      await read((await host.exchange(request(peer.port, { generation: 2 }))).body);
      expect(peer.sessions.length).toBe(2);
    } finally {
      await peer.stop();
    }
  });

  it("resets the stream with CANCEL when the signal aborts after the head", async () => {
    const reset = resetCode();
    const peer = await h2c((stream) => {
      reset.watch(stream);
      stream.respond({ ":status": 200 });
      stream.write("first");
    });
    try {
      const abort = new AbortController();
      const response = await host.exchange(request(peer.port, { signal: abort.signal }));
      const reader = response.body.getReader();
      expect(new TextDecoder().decode((await reader.read()).value)).toBe("first");
      abort.abort();
      await expect(reader.read()).rejects.toMatchObject({ name: "AbortError" });
      expect(await reset.code).toBe(NGHTTP2_CANCEL);
    } finally {
      await peer.stop();
    }
  });

  it("rejects with an AbortError and resets with CANCEL when the signal aborts before the head", async () => {
    const reset = resetCode();
    let arrived = false;
    const peer = await h2c((stream) => {
      arrived = true;
      reset.watch(stream);
    });
    try {
      const abort = new AbortController();
      const pending = host.exchange(request(peer.port, { signal: abort.signal }));
      pending.catch(() => undefined);
      await settled(() => arrived);
      abort.abort();
      await expect(pending).rejects.toMatchObject({ name: "AbortError" });
      expect(await reset.code).toBe(NGHTTP2_CANCEL);
    } finally {
      await peer.stop();
    }
  });

  it("does not connect for a signal that is already aborted", async () => {
    const peer = await h2c(() => undefined);
    try {
      await expect(host.exchange(request(peer.port, { signal: AbortSignal.abort() }))).rejects.toMatchObject({ name: "AbortError" });
      expect(peer.sessions.length).toBe(0);
    } finally {
      await peer.stop();
    }
  });

  it("resets the stream with CANCEL when the body stream is cancelled", async () => {
    const reset = resetCode();
    const peer = await h2c((stream) => {
      reset.watch(stream);
      stream.respond({ ":status": 200 });
      stream.write("first");
    });
    try {
      const response = await host.exchange(request(peer.port));
      const reader = response.body.getReader();
      await reader.read();
      await reader.cancel();
      expect(await reset.code).toBe(NGHTTP2_CANCEL);
    } finally {
      await peer.stop();
    }
  });

  it("rejects with the connection's error when nothing listens", async () => {
    const peer = await h2c(() => undefined);
    const port = peer.port;
    await peer.stop();
    await expect(host.exchange(request(port))).rejects.toBeInstanceOf(Error);
  });

  it("reopens a session after the peer closed the pooled one", async () => {
    const peer = await h2c((stream) => {
      stream.respond({ ":status": 200 });
      stream.end("ok");
    });
    try {
      await read((await host.exchange(request(peer.port, { generation: 1 }))).body);
      peer.sessions[0].close();
      await settled(() => peer.closed() === 1);
      await new Promise((resolve) => globalThis.setTimeout(resolve, 25));
      expect(await read((await host.exchange(request(peer.port, { generation: 1 }))).body)).toBe("ok");
      expect(peer.sessions.length).toBe(2);
    } finally {
      await peer.stop();
    }
  });

  it("does not leave a failed connection in the pool", async () => {
    const first = await h2c(() => undefined);
    const { port } = first;
    await first.stop();
    await expect(host.exchange(request(port, { generation: 1 }))).rejects.toBeInstanceOf(Error);
    const server = http2.createServer();
    server.on("stream", (stream) => {
      stream.on("error", () => undefined);
      stream.respond({ ":status": 200 });
      stream.end("back");
    });
    const second = await listen(server, port);
    try {
      expect(await read((await host.exchange(request(port, { generation: 1 }))).body)).toBe("back");
      expect(second.sessions.length).toBe(1);
    } finally {
      await second.stop();
    }
  });

  it("returns every value of a repeated response header", async () => {
    const peer = await h2c((stream) => {
      stream.respond({ ":status": 200, "set-cookie": ["a=1", "b=2"] });
      stream.end("ok");
    });
    try {
      const response = await host.exchange(request(peer.port));
      expect(response.headers.filter(([name]) => name === "set-cookie")).toEqual([
        ["set-cookie", "a=1"],
        ["set-cookie", "b=2"],
      ]);
      await read(response.body);
    } finally {
      await peer.stop();
    }
  });

  it("errors the body when the connection drops before the stream ended", async () => {
    const connections: Socket[] = [];
    const server = http2.createServer();
    server.on("connection", (socket) => connections.push(socket));
    server.on("stream", (stream) => {
      stream.on("error", () => undefined);
      stream.respond({ ":status": 200 });
      stream.write("first");
      globalThis.setTimeout(() => connections[0].destroy(), 30);
    });
    const peer = await listen(server, 0);
    try {
      const response = await host.exchange(request(peer.port));
      await expect(read(response.body)).rejects.toThrow(/closed before it ended/);
    } finally {
      await peer.stop();
    }
  });
});

describe("the exchange over a unix socket", () => {
  it("carries the Workload API stream and reads frames as they arrive", async () => {
    const directory = mkdtempSync(join(tmpdir(), "abx-"));
    const path = join(directory, "s.sock");
    const seen: http2.IncomingHttpHeaders[] = [];
    const server = http2.createServer();
    server.on("stream", (stream, headers) => {
      seen.push(headers);
      stream.on("error", () => undefined);
      stream.respond({ ":status": 200, "content-type": "application/grpc" });
      stream.write("frame-1");
      setImmediate(() => stream.write("frame-2"));
    });
    const peer = await listen(server, path);
    try {
      const response = await host.exchange(
        request(0, {
          connect: { unix: path },
          headers: [
            ["content-type", "application/grpc"],
            ["te", "trailers"],
            ["workload.spiffe.io", "true"],
          ],
          body: new Uint8Array(5),
          path: "/SpiffeWorkloadAPI/FetchX509SVID",
        }),
      );
      expect(response.status).toBe(200);
      const reader = response.body.getReader();
      let text = "";
      while (!text.includes("frame-2")) text += new TextDecoder().decode((await reader.read()).value);
      expect(text).toContain("frame-1");
      expect(seen[0]["workload.spiffe.io"]).toBe("true");
      await reader.cancel();
    } finally {
      await peer.stop();
      rmSync(directory, { recursive: true, force: true });
    }
  });
});

describe("the installed slot", () => {
  it("is filled once under the shared symbol", () => {
    installNodeExchange();
    const slot = (globalThis as unknown as Record<symbol, unknown>)[Symbol.for("brefwiz.api-bones.node-exchange")];
    expect(typeof slot).toBe("function");
    expect(EXCHANGE_SLOT).toBe(Symbol.for("brefwiz.api-bones.node-exchange"));
    installNodeExchange();
    expect((globalThis as unknown as Record<symbol, unknown>)[EXCHANGE_SLOT]).toBe(slot);
  });
});

describe("the exchange over TLS", () => {
  let directory: string;
  let ca: string;
  let leaf: string;
  let leafKey: string;
  let stranger: string;

  beforeAll(() => {
    directory = mkdtempSync(join(tmpdir(), "abx-tls-"));
    const run = (...args: string[]) => execFileSync("openssl", args, { cwd: directory, stdio: "pipe" });
    const authority = (name: string) => {
      run("req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1", "-nodes", "-keyout", `${name}.key`, "-out", `${name}.pem`, "-subj", `/CN=${name}`, "-days", "2", "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign");
    };
    authority("ca");
    authority("stranger");
    run("req", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1", "-nodes", "-keyout", "leaf.key", "-out", "leaf.csr", "-subj", "/CN=leaf");
    writeFileSync(join(directory, "leaf.ext"), "subjectAltName=URI:spiffe://td.example/svc/peer\nbasicConstraints=CA:FALSE\nextendedKeyUsage=serverAuth,clientAuth\n");
    run("x509", "-req", "-in", "leaf.csr", "-CA", "ca.pem", "-CAkey", "ca.key", "-CAcreateserial", "-out", "leaf.pem", "-days", "2", "-extfile", "leaf.ext");
    const text = (name: string) => readFileSync(join(directory, name), "utf8");
    ca = text("ca.pem");
    leaf = text("leaf.pem");
    leafKey = text("leaf.key");
    stranger = text("stranger.pem");
  });
  afterAll(() => {
    rmSync(directory, { recursive: true, force: true });
  });

  async function secure(onClient?: (certificate: string | undefined) => void, extra: http2.SecureServerOptions = {}): Promise<Peer> {
    const server = http2.createSecureServer({ key: leafKey, cert: leaf, ca, requestCert: onClient !== undefined, rejectUnauthorized: false, ...extra });
    server.on("secureConnection", (socket) => {
      const presented = socket.getPeerCertificate().raw;
      onClient?.(presented === undefined ? undefined : new X509Certificate(presented).subject);
    });
    server.on("stream", (stream) => {
      stream.on("error", () => undefined);
      stream.respond({ ":status": 200 });
      stream.end("secure");
    });
    return listen(server, 0);
  }

  function tlsRequest(port: number, material: { ca: string | null; cert: string | null; key: string | null }, extra: Partial<ExchangeRequest> = {}): ExchangeRequest {
    return request(port, { connect: { tcp: { host: "127.0.0.1", port, servername: "ignored.example", tls: material } }, generation: 1, ...extra });
  }

  it("hands the verified chain, leaf first, to the peer check instead of a hostname check", async () => {
    const peer = await secure();
    try {
      const chains: Uint8Array[][] = [];
      const response = await host.exchange(
        tlsRequest(peer.port, { ca, cert: null, key: null }, {
          peerAdmissible: (chain) => {
            chains.push(chain);
            return undefined;
          },
        }),
      );
      expect(await read(response.body)).toBe("secure");
      expect(chains.length).toBe(1);
      expect(Buffer.from(chains[0][0]).equals(new X509Certificate(leaf).raw)).toBe(true);
      expect(Buffer.from(chains[0][chains[0].length - 1]).equals(new X509Certificate(ca).raw)).toBe(true);
      // A second call rides the pooled connection and never asks again.
      await read((await host.exchange(tlsRequest(peer.port, { ca, cert: null, key: null }, { peerAdmissible: () => "must not be asked" }))).body);
      expect(chains.length).toBe(1);
      expect(peer.sessions.length).toBe(1);
    } finally {
      await peer.stop();
    }
  });

  it("fails the handshake with the reason the peer check returned", async () => {
    const peer = await secure();
    try {
      await expect(host.exchange(tlsRequest(peer.port, { ca, cert: null, key: null }, { peerAdmissible: () => "not the peer we expect" }))).rejects.toThrow("not the peer we expect");
    } finally {
      await peer.stop();
    }
  });

  it("refuses the connection when the peer check throws, as a dropped closure does", async () => {
    const peer = await secure();
    try {
      await expect(
        host.exchange(
          tlsRequest(peer.port, { ca, cert: null, key: null }, {
            peerAdmissible: () => {
              throw new Error("closure dropped");
            },
          }),
        ),
      ).rejects.toThrow("closure dropped");
    } finally {
      await peer.stop();
    }
  });

  it("verifies against the supplied trust only and never asks the peer check for a chain it does not anchor", async () => {
    const peer = await secure();
    try {
      let asked = false;
      await expect(
        host.exchange(
          tlsRequest(peer.port, { ca: stranger, cert: null, key: null }, {
            peerAdmissible: () => {
              asked = true;
              return undefined;
            },
          }),
        ),
      ).rejects.toMatchObject({ code: expect.stringMatching(/UNABLE_TO_VERIFY_LEAF_SIGNATURE|SELF_SIGNED_CERT_IN_CHAIN|UNABLE_TO_GET_ISSUER_CERT/) });
      expect(asked).toBe(false);
    } finally {
      await peer.stop();
    }
  });

  it("keeps the hostname check when there is no peer check", async () => {
    const peer = await secure();
    try {
      await expect(host.exchange(tlsRequest(peer.port, { ca, cert: null, key: null }))).rejects.toMatchObject({ code: "ERR_TLS_CERT_ALTNAME_INVALID" });
    } finally {
      await peer.stop();
    }
  });

  it("presents the supplied client certificate", async () => {
    const subjects: Array<string | undefined> = [];
    const peer = await secure((subject) => subjects.push(subject));
    try {
      await read((await host.exchange(tlsRequest(peer.port, { ca, cert: leaf, key: leafKey }, { peerAdmissible: () => undefined }))).body);
      expect(subjects).toEqual(["CN=leaf"]);
    } finally {
      await peer.stop();
    }
  });

  it("does not share a session between owners on one generation number", async () => {
    const peer = await secure();
    try {
      const admit = () => undefined;
      await read((await host.exchange(tlsRequest(peer.port, { ca, cert: null, key: null }, { owner: "module-a", peerAdmissible: admit }))).body);
      await read((await host.exchange(tlsRequest(peer.port, { ca, cert: null, key: null }, { owner: "module-b", peerAdmissible: admit }))).body);
      expect(peer.sessions.length).toBe(2);
    } finally {
      await peer.stop();
    }
  });

  it("refuses a peer that does not negotiate h2", async () => {
    const sockets: tls.TLSSocket[] = [];
    const server = tls.createServer({ key: leafKey, cert: leaf, ALPNProtocols: ["http/1.1"] }, (socket) => {
      sockets.push(socket);
      socket.on("error", () => undefined);
    });
    server.on("tlsClientError", () => undefined);
    await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
    const { port } = server.address() as AddressInfo;
    try {
      await expect(host.exchange(tlsRequest(port, { ca, cert: null, key: null }, { peerAdmissible: () => undefined }))).rejects.toThrow(/h2|ALPN|application protocol/i);
    } finally {
      for (const socket of sockets) socket.destroy();
      await new Promise<void>((resolve) => server.close(() => resolve()));
    }
  });

  it("sends no SNI for an IP host and the supplied name for a DNS host", async () => {
    const names: string[] = [];
    const peer = await secure(undefined, {
      SNICallback: (name, done) => {
        names.push(name);
        done(null, undefined);
      },
    });
    try {
      const material = { ca, cert: null, key: null };
      await read((await host.exchange(tlsRequest(peer.port, material, { peerAdmissible: () => undefined }))).body);
      expect(names).toEqual([]);
      const named = request(peer.port, {
        connect: { tcp: { host: "localhost", port: peer.port, servername: "named.example", tls: material } },
        generation: 1,
        peerAdmissible: () => undefined,
      });
      await read((await host.exchange(named)).body);
      expect(names).toEqual(["named.example"]);
    } finally {
      await peer.stop();
    }
  });

  it("gives the same generation with different TLS material a session of its own", async () => {
    const peer = await secure();
    try {
      const admit = () => undefined;
      const anonymous = { ca, cert: null, key: null };
      const identified = { ca, cert: leaf, key: leafKey };
      await read((await host.exchange(tlsRequest(peer.port, anonymous, { peerAdmissible: admit }))).body);
      await read((await host.exchange(tlsRequest(peer.port, identified, { peerAdmissible: admit }))).body);
      expect(peer.sessions.length).toBe(2);
      // Each is pooled under its own material, and neither closed the other.
      await read((await host.exchange(tlsRequest(peer.port, anonymous))).body);
      await read((await host.exchange(tlsRequest(peer.port, identified))).body);
      expect(peer.sessions.length).toBe(2);
      expect(peer.closed()).toBe(0);
    } finally {
      await peer.stop();
    }
  });

  it("lets go of the peer check once the handshake ended", async () => {
    const collect = vm.runInNewContext("gc") as () => void;
    const peer = await secure();
    try {
      const watched = await (async () => {
        const admit = (): undefined => undefined;
        const ref = new WeakRef(admit);
        await read((await host.exchange(tlsRequest(peer.port, { ca, cert: null, key: null }, { peerAdmissible: admit }))).body);
        return ref;
      })();
      for (let round = 0; round < 50 && watched.deref() !== undefined; round += 1) {
        await new Promise((resolve) => globalThis.setTimeout(resolve, 10));
        collect();
      }
      expect(watched.deref()).toBeUndefined();
      // The connection the check was made for is still the one that serves.
      await read((await host.exchange(tlsRequest(peer.port, { ca, cert: null, key: null }))).body);
      expect(peer.sessions.length).toBe(1);
    } finally {
      await peer.stop();
    }
  });
});
