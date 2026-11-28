// SPDX-License-Identifier: MIT
import { constants as cryptoConstants, createHash } from "node:crypto";
import * as http2 from "node:http2";
import * as net from "node:net";
import * as tls from "node:tls";

// The host half of the Node transport of the wasm core: a byte pipe over
// `node:http2` that the core drives. The core decides every policy (which
// identity, which peer, which trust, when a generation changes); this module
// holds sockets and nothing else.

/** The slot the exchange is installed in; the core reads it on every call. */
export const EXCHANGE_SLOT = Symbol.for("brefwiz.api-bones.node-exchange");

/** PEM text the core supplies for one TLS connection. `cert` and `key` are null together. */
export interface TlsMaterial {
  /** The only trust of the connection; null means Node's default roots. */
  readonly ca: string | null;
  /** The chain presented to the peer, leaf first. */
  readonly cert: string | null;
  /** The private key of `cert` (PKCS8). */
  readonly key: string | null;
}

/** Where a request goes: a TCP endpoint (TLS when `tls` is set, else h2c) or a unix socket. */
export type ExchangeConnect =
  | {
      readonly tcp: {
        readonly host: string;
        readonly port: number;
        /** The SNI name; not sent when `host` is an IP literal. */
        readonly servername: string;
        readonly tls: TlsMaterial | null;
      };
    }
  | { readonly unix: string };

/** One request the core hands to the exchange. */
export interface ExchangeRequest {
  readonly connect: ExchangeConnect;
  /** Names the wasm module that built the request; generations are only compared within one owner. */
  readonly owner: string;
  /** 0 is a connection with no identity; every other value is the identity generation the request was minted under. */
  readonly generation: number;
  readonly method: string;
  /** Path and query. */
  readonly path: string;
  /** Lower-case names, no pseudo-headers. */
  readonly headers: ReadonlyArray<readonly [string, string]>;
  readonly body: Uint8Array;
  readonly signal: AbortSignal;
  /**
   * Present for an in-fleet call: takes the verified peer chain (DER, leaf first)
   * and answers the reason the peer is refused, or undefined to admit it.
   */
  readonly peerAdmissible: ((chain: Uint8Array[]) => string | undefined) | null;
}

/** What the exchange answers: the head, and the body bytes as the server sent them. */
export interface ExchangeResponse {
  readonly status: number;
  /** Lower-case names, no pseudo-headers. */
  readonly headers: Array<[string, string]>;
  readonly body: ReadableStream<Uint8Array>;
}

/** The function installed in {@link EXCHANGE_SLOT}. */
export type Exchange = (request: ExchangeRequest) => Promise<ExchangeResponse>;

/** An exchange with its own session pool. */
export interface NodeExchange {
  readonly exchange: Exchange;
  /** Destroys every pooled session. */
  close(): void;
}

// AEAD suites only, at least security level 2. TLS 1.3 suites are all AEAD.
const CIPHERS = "ECDHE+AESGCM:ECDHE+CHACHA20:@SECLEVEL=2";

interface Entry {
  readonly generation: number;
  readonly session: http2.ClientHttp2Session;
  /** Settles when the connection (including its TLS handshake) is usable. */
  readonly ready: Promise<void>;
  /** Calls that hold the session open for the process: connecting, or a stream not yet closed. */
  active: number;
}

interface Group {
  /** The newest generation seen on this (owner, connect). */
  newest: number;
  readonly entries: Map<string, Entry>;
}

function abortError(): DOMException {
  return new DOMException("the call was aborted", "AbortError");
}

/** Rejects with an AbortError when `signal` aborts first. */
function until<T>(work: Promise<T>, signal: AbortSignal): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const onAbort = () => reject(abortError());
    signal.addEventListener("abort", onAbort, { once: true });
    work.then(resolve, reject).finally(() => signal.removeEventListener("abort", onAbort));
  });
}

function cancelStream(stream: http2.ClientHttp2Stream): void {
  if (!stream.closed) stream.close(http2.constants.NGHTTP2_CANCEL);
}

/** The peer chain as DER, leaf first, each issuer until one repeats. */
function chainOf(leaf: tls.DetailedPeerCertificate): Uint8Array[] {
  const chain: Uint8Array[] = [];
  const seen = new Set<tls.DetailedPeerCertificate>();
  let previous: Buffer | undefined;
  for (let at: tls.DetailedPeerCertificate | undefined = leaf; at !== undefined && !seen.has(at); at = at.issuerCertificate) {
    if (previous?.equals(at.raw) === true) break;
    seen.add(at);
    chain.push(new Uint8Array(at.raw));
    previous = at.raw;
  }
  return chain;
}

function digestOf(parts: ReadonlyArray<string | null>): string {
  return createHash("sha256").update(JSON.stringify(parts)).digest("hex");
}

/** What distinguishes one connect from another; the generation and the TLS material are not part of it. */
function connectOf(connect: ExchangeConnect): string {
  if ("unix" in connect) return JSON.stringify(["unix", connect.unix]);
  const { host, port, servername, tls: material } = connect.tcp;
  return JSON.stringify(["tcp", material !== null, host, port, servername]);
}

function materialOf(connect: ExchangeConnect): string {
  if ("unix" in connect || connect.tcp.tls === null) return "";
  const { ca, cert, key } = connect.tcp.tls;
  return digestOf([ca, cert, key]);
}

function authorityOf(host: string, port: number, secure: boolean): string {
  return `${secure ? "https" : "http"}://${host.includes(":") ? `[${host}]` : host}:${port}`;
}

/** The TLS socket of one connection. `hold` is the request's peer check, dropped as soon as the handshake ended. */
function secureSocket(host: string, port: number, servername: string, material: TlsMaterial, hold: { admit: ExchangeRequest["peerAdmissible"] }): tls.TLSSocket {
  const options: tls.ConnectionOptions = {
    host,
    port,
    servername: net.isIP(host) === 0 ? servername : undefined,
    ca: material.ca ?? undefined,
    cert: material.cert ?? undefined,
    key: material.key ?? undefined,
    ALPNProtocols: ["h2"],
    minVersion: "TLSv1.2",
    ciphers: CIPHERS,
    // No session tickets and no `session` option: every connection is a full handshake.
    secureOptions: cryptoConstants.SSL_OP_NO_TICKET,
    rejectUnauthorized: true,
  };
  if (hold.admit !== null) {
    // Node calls this only after the chain verified against `ca`. An SVID names no DNS host, so it replaces the hostname check.
    options.checkServerIdentity = (_hostname, peer) => {
      const admit = hold.admit;
      if (admit === null) return new Error("the peer check is no longer available");
      try {
        const refusal = admit(chainOf(peer as tls.DetailedPeerCertificate));
        return refusal === undefined ? undefined : new Error(refusal);
      } catch (error) {
        return error instanceof Error ? error : new Error(String(error));
      }
    };
  }
  const socket = tls.connect(options);
  const release = () => {
    hold.admit = null;
  };
  socket.once("secureConnect", () => {
    release();
    if (socket.alpnProtocol !== "h2") socket.destroy(new Error("the peer did not negotiate h2"));
  });
  socket.once("error", release);
  socket.once("close", release);
  return socket;
}

function open(request: ExchangeRequest): Entry {
  const { connect } = request;
  let session: http2.ClientHttp2Session;
  if ("unix" in connect) {
    const path = connect.unix;
    session = http2.connect("http://localhost", { createConnection: () => net.connect({ path }) });
    // The stream it carries is read forever and must not keep the process alive.
    session.unref();
  } else {
    const { host, port, servername, tls: material } = connect.tcp;
    const hold = { admit: request.peerAdmissible };
    session =
      material === null
        ? http2.connect(authorityOf(host, port, false))
        : http2.connect(authorityOf(host, port, true), { createConnection: () => secureSocket(host, port, servername, material, hold) });
  }
  const ready = new Promise<void>((resolve, reject) => {
    session.once("connect", () => resolve());
    session.once("error", reject);
    session.once("close", () => reject(new Error("the connection closed before it was ready")));
  });
  ready.catch(() => undefined);
  // Failures reach the calls through `ready` and their streams.
  session.on("error", () => undefined);
  return { generation: request.generation, session, ready, active: 0 };
}

function outgoing(request: ExchangeRequest): http2.OutgoingHttpHeaders {
  const headers: http2.OutgoingHttpHeaders = { ":method": request.method, ":path": request.path };
  for (const [name, value] of request.headers) {
    const earlier = headers[name];
    headers[name] = earlier === undefined ? value : [...(Array.isArray(earlier) ? earlier : [String(earlier)]), value];
  }
  return headers;
}

function incoming(head: http2.IncomingHttpHeaders): Array<[string, string]> {
  const out: Array<[string, string]> = [];
  for (const [name, value] of Object.entries(head)) {
    if (name.startsWith(":") || value === undefined) continue;
    for (const each of Array.isArray(value) ? value : [value]) out.push([name, each]);
  }
  return out;
}

/**
 * Keeps the process alive while a call uses the session, and lets it exit once
 * the session is idle. The Workload API session is never held: it is read for
 * the life of the process and unreferenced when opened.
 */
function retain(request: ExchangeRequest, entry: Entry): () => void {
  if ("unix" in request.connect) return () => undefined;
  entry.active += 1;
  entry.session.ref();
  let held = true;
  return () => {
    if (!held) return;
    held = false;
    entry.active -= 1;
    if (entry.active === 0 && !entry.session.destroyed) entry.session.unref();
  };
}

/** Sends the request on `session` and answers once the head arrived; the body streams on. */
function send(session: http2.ClientHttp2Session, request: ExchangeRequest, done: () => void): Promise<ExchangeResponse> {
  const { signal } = request;
  const stream = session.request(outgoing(request), { endStream: request.body.byteLength === 0 });
  if (request.body.byteLength > 0) stream.end(request.body);

  let rejectHead: (reason: unknown) => void = () => undefined;
  const head = new Promise<http2.IncomingHttpHeaders>((resolve, reject) => {
    rejectHead = reject;
    stream.once("response", resolve);
  });
  let bodyOpen = true;
  let ended = false;
  let failBody: (reason: unknown) => void = () => undefined;
  const fail = (reason: unknown) => {
    rejectHead(reason);
    failBody(reason);
  };

  const body = new ReadableStream<Uint8Array>(
    {
      start(controller) {
        failBody = (reason) => {
          if (!bodyOpen) return;
          bodyOpen = false;
          controller.error(reason);
        };
        stream.on("data", (chunk: Buffer) => {
          if (!bodyOpen) return;
          controller.enqueue(new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength));
          if ((controller.desiredSize ?? 1) <= 0) stream.pause();
        });
        stream.on("end", () => {
          // Node ends the readable side of a stream the peer reset or a dropped connection took, too.
          const reset = stream.rstCode ?? 0;
          if (reset !== 0) {
            fail(new Error(`the stream closed before it ended (code ${reset})`));
            return;
          }
          ended = true;
          if (!bodyOpen) return;
          bodyOpen = false;
          controller.close();
        });
      },
      pull() {
        stream.resume();
      },
      cancel() {
        bodyOpen = false;
        cancelStream(stream);
      },
    },
    { highWaterMark: 1 },
  );

  const onAbort = () => {
    cancelStream(stream);
    fail(abortError());
  };
  signal.addEventListener("abort", onAbort, { once: true });
  stream.on("error", fail);
  stream.once("close", () => {
    signal.removeEventListener("abort", onAbort);
    if (!ended) fail(new Error(`the stream closed before it ended (code ${stream.rstCode ?? "none"})`));
    done();
  });
  if (signal.aborted) onAbort();

  return head.then((headers) => ({ status: Number(headers[":status"]), headers: incoming(headers), body }));
}

/** A fresh exchange with its own session pool. */
export function createNodeExchange(): NodeExchange {
  const groups = new Map<string, Group>();

  /** The session that serves `request`, and whether the pool keeps it. */
  function sessionFor(request: ExchangeRequest): { entry: Entry; pooled: boolean } {
    const key = JSON.stringify([request.owner, connectOf(request.connect)]);
    let group = groups.get(key);
    if (group === undefined) {
      group = { newest: request.generation, entries: new Map() };
      groups.set(key, group);
    }
    // A request minted under an older generation than one already seen is served by a session of its own.
    if (request.generation < group.newest) return { entry: open(request), pooled: false };
    if (request.generation > group.newest) {
      group.newest = request.generation;
      for (const [name, older] of group.entries) {
        if (older.generation >= group.newest) continue;
        // Closing lets the calls in flight finish; no new call is served from it.
        group.entries.delete(name);
        older.session.close();
      }
    }
    const name = `${request.generation}:${materialOf(request.connect)}`;
    const existing = group.entries.get(name);
    if (existing !== undefined && !existing.session.closed && !existing.session.destroyed) return { entry: existing, pooled: true };
    const entry = open(request);
    const entries = group.entries;
    entries.set(name, entry);
    const drop = () => {
      if (entries.get(name) === entry) entries.delete(name);
    };
    entry.session.once("close", drop);
    entry.ready.catch(drop);
    return { entry, pooled: true };
  }

  const exchange: Exchange = async (request) => {
    if (request.signal.aborted) throw abortError();
    const { entry, pooled } = sessionFor(request);
    const release = retain(request, entry);
    try {
      await until(entry.ready, request.signal);
      return await send(entry.session, request, () => {
        release();
        if (!pooled) entry.session.close();
      });
    } catch (error) {
      release();
      if (!pooled) entry.session.destroy();
      throw error;
    }
  };

  return {
    exchange,
    close() {
      for (const group of groups.values()) for (const entry of group.entries.values()) entry.session.destroy();
      groups.clear();
    },
  };
}

/** Installs one exchange in {@link EXCHANGE_SLOT} unless one is already there. */
export function installNodeExchange(): void {
  const host = globalThis as unknown as Record<symbol, unknown>;
  if (host[EXCHANGE_SLOT] === undefined) host[EXCHANGE_SLOT] = createNodeExchange().exchange;
}
