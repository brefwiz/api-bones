// SPDX-License-Identifier: MIT
//
// Why a connection-write failure died, not just that it did.
//
// `isConnectionWriteFailure` (retry.ts) already recognises the shape: the peer
// closed the connection before the request reached it. That classification is
// enough to decide replay eligibility, and for a method the generated policy
// declares idempotent the caller never sees the failure at all.
//
// For every other method — anything `UNSPECIFIED`, which is most of them until
// somebody deliberately annotates otherwise — the failure is correctly NOT
// replayed and surfaces to the caller as a bare `[internal] write EPIPE`. That
// sentence names the symptom and nothing else. It does not say whether the
// socket came from the pool or was freshly connected, how old it was, or
// whether the peer's FIN had already arrived — which is the whole difference
// between "our keep-alive window outlives the server's" (a client-side fix) and
// "the server is tearing down healthy connections" (a server-side one).
//
// Recovering those facts after the fact is impossible: the socket is gone by
// the time anyone reads the log. They have to be recorded as the connection
// lives, which is what this module does. The transport owns the agent — no
// consumer wires one, sets a flag, or opts in. The one thing a consumer does
// supply is its TLS identity, and that is not a knob: a mesh workload's
// identity is runtime state nothing else can derive.
//
// Scope: HTTP/1.1, where a pooling `Agent` is the thing that hands back a stale
// socket. The HTTP/2 path multiplexes over a session and fails with
// GOAWAY/stream-closed instead; those already name their own cause, and
// connect-node owns the session rather than accepting one, so there is nothing
// here to instrument without patching it.

import { AsyncLocalStorage } from "node:async_hooks";
import http from "node:http";
import https from "node:https";
import type { ClientRequest, IncomingMessage } from "node:http";
import type { Socket } from "node:net";

import { Code, ConnectError, type Interceptor } from "@connectrpc/connect";

import { markNotDelivered, markResetBeforeResponse } from "./delivery-evidence.js";
import { isConnectionWriteFailure } from "./retry.js";

/**
 * Client TLS material for an mTLS peer.
 *
 * A narrow slice of Node's agent options on purpose: everything else there is
 * transport composition the helper already owns, and accepting the whole type
 * would reopen the bypass this module exists to close.
 */
export type NodeTlsIdentity = Pick<
  https.AgentOptions,
  | "cert"
  | "key"
  | "ca"
  | "rejectUnauthorized"
  | "checkServerIdentity"
  // A SPIFFE bundle is a trust anchor, not a full chain to a public root, so a
  // workload's own leaf legitimately validates against a partial chain. Omit
  // this and every SPIFFE caller's handshake fails — which is the shape of gap
  // that makes a "canonical" helper unusable in practice.
  | "allowPartialTrustChain"
>;

/**
 * What the transport hands to a connection: a caller's {@link NodeTlsIdentity}
 * or a SPIFFE client's pin, whose TLS floor and session scope ride along
 * unchanged. Not accepted from callers, who pass {@link NodeTlsIdentity}.
 */
export type NodeTlsOptions = NodeTlsIdentity &
  Pick<https.AgentOptions, "minVersion" | "sessionIdContext">;

/** Live identity, resolved per connection so a rotating SVID needs no rebuild. */
export type NodeTlsIdentitySource = NodeTlsOptions | (() => NodeTlsOptions);

export function resolveTlsIdentity(
  source: NodeTlsIdentitySource | undefined,
): NodeTlsOptions {
  if (source === undefined) return {};
  return typeof source === "function" ? source() : source;
}

/** Connection facts as of the moment a request failed on it. */
export interface ConnectionFacts {
  /** Monotonic-ish id, unique per agent, for correlating repeat failures. */
  readonly socketId: number;
  /** `host:port` the socket was opened to. */
  readonly authority: string;
  /** True when this request was written onto an already-open pooled socket. */
  readonly reused: boolean;
  /** Requests this socket had already carried before the failing one. */
  readonly priorRequests: number;
  /** Socket age in ms when the failing request was assigned to it. */
  readonly ageAtAssignMs: number;
  /** Socket age in ms when the request failed. */
  readonly ageAtFailureMs: number;
  /** Idle ms between the previous response on this socket and the assignment. */
  readonly idleBeforeAssignMs: number | null;
  /** Ms between the peer's FIN and the failure. Null when no FIN was seen. */
  readonly finBeforeFailureMs: number | null;
  /** The peer's advertised keep-alive policy, from the last response. */
  readonly serverConnectionHeader: string | null;
  readonly serverKeepAliveHeader: string | null;
  readonly bytesWritten: number;
  readonly bytesRead: number;
  /**
   * Whether the response head had arrived when the request failed.
   *
   * Socket byte counters span every request a pooled socket ever carried, so
   * they cannot answer this; the request's own response event does.
   */
  readonly responseStarted: boolean;
  /**
   * Whether the socket ever completed its TCP connect.
   *
   * False for a peer that refused, a host that did not resolve, a connect that
   * timed out and a route that did not exist: the connection never opened, so
   * nothing of the request can have reached anyone.
   */
  readonly connected: boolean;
  /**
   * Whether the peer itself declared the request unprocessed.
   *
   * HTTP/2 promises it: a refused stream, or a stream numbered above the last
   * one a GOAWAY says was handled, was never acted on, however many request
   * frames had already been written. Always false on HTTP/1.1, which has no
   * such statement.
   */
  readonly unprocessed: boolean;
  /**
   * True for facts taken from an HTTP/2 session.
   *
   * A session is shared, so its byte counters say nothing about one request:
   * on HTTP/2 only a connect failure or the peer's own statement proves a
   * request never left.
   */
  readonly http2: boolean;
  /**
   * Whether the TLS handshake had finished when the request failed.
   *
   * A socket can be young, unreused and already broken, and those three facts
   * alone cannot tell a handshake that never completed from a peer that hung up
   * on a completed one. The first is the client's problem and the second the
   * server's, so the distinction decides which side is even worth looking at.
   *
   * Null on a plaintext socket, where there is no handshake to report.
   */
  readonly tlsEstablished: boolean | null;
  /** Negotiated ALPN, which is what decides whether this is an h1 or h2 path. */
  readonly alpnProtocol: string | null;
  /** Whether the peer's certificate verified, and what refused it if not. */
  readonly tlsAuthorized: boolean | null;
  readonly tlsAuthorizationError: string | null;
}

/** The TLS half of a socket, absent on a plaintext one. */
interface TlsSocketFacets {
  encrypted?: boolean;
  authorized?: boolean;
  authorizationError?: Error | string | null;
  alpnProtocol?: string | false | null;
  getProtocol?: () => string | null;
}

/**
 * TLS state as the socket holds it right now.
 *
 * Read at failure rather than at handshake, because a socket that never
 * completed one has nothing to report at handshake time -- which is precisely
 * the case worth naming.
 */
export function readTlsState(
  socket: unknown,
): Pick<
  ConnectionFacts,
  "tlsEstablished" | "alpnProtocol" | "tlsAuthorized" | "tlsAuthorizationError"
> {
  const tls = socket as TlsSocketFacets | null;
  if (tls === null || tls === undefined || tls.encrypted !== true) {
    return {
      tlsEstablished: null,
      alpnProtocol: null,
      tlsAuthorized: null,
      tlsAuthorizationError: null,
    };
  }
  // `getProtocol()` answers null until the handshake completes, and a version
  // string after -- the one signal that separates "never got a session" from
  // "had one and lost it".
  const negotiated = typeof tls.getProtocol === "function" ? tls.getProtocol() : null;
  const failure = tls.authorizationError ?? null;
  return {
    tlsEstablished: negotiated !== null,
    alpnProtocol: typeof tls.alpnProtocol === "string" ? tls.alpnProtocol : null,
    tlsAuthorized: typeof tls.authorized === "boolean" ? tls.authorized : null,
    tlsAuthorizationError:
      failure === null ? null : typeof failure === "string" ? failure : failure.message,
  };
}

/**
 * Whether the connection's own facts prove the request never left the client.
 *
 * A socket that never connected carried nothing, and a stream the peer declared
 * unprocessed was never acted on. Past those, on a TLS socket the bytes written before the handshake completes are the
 * handshake itself, so a non-zero write count proves nothing there; only a
 * finished handshake lets request bytes onto the wire, and the absence of one
 * proves none did. On a plaintext socket every written byte is request, so an
 * empty write count is the proof.
 */
export function requestNeverLeft(facts: ConnectionFacts): boolean {
  if (!facts.connected || facts.unprocessed) return true;
  if (facts.http2) return false;
  if (facts.tlsEstablished === null) return facts.bytesWritten === 0;
  return !facts.tlsEstablished;
}

/** One line, log-greppable, naming the cause rather than the symptom. */
export function formatConnectionFacts(facts: ConnectionFacts): string {
  const parts = [
    `socket=${facts.socketId}`,
    `authority=${facts.authority}`,
    facts.reused ? `reused after ${facts.priorRequests} request(s)` : "freshly connected",
    `age=${facts.ageAtFailureMs}ms`,
  ];
  if (facts.idleBeforeAssignMs !== null) {
    parts.push(`idle-before-write=${facts.idleBeforeAssignMs}ms`);
  }
  parts.push(
    facts.finBeforeFailureMs === null
      ? "no peer FIN observed"
      : `peer FIN ${facts.finBeforeFailureMs}ms before the write failed`,
  );
  if (facts.serverKeepAliveHeader !== null) {
    parts.push(`server keep-alive: ${facts.serverKeepAliveHeader}`);
  } else if (facts.serverConnectionHeader !== null) {
    parts.push(`server connection: ${facts.serverConnectionHeader}`);
  }
  parts.push(`bytes w/r=${facts.bytesWritten}/${facts.bytesRead}`);
  if (!facts.connected) parts.push("connection never opened");
  if (facts.unprocessed) parts.push("peer declared the stream unprocessed");
  if (facts.tlsEstablished === false) {
    // Said first among the TLS facts because it reframes everything above it:
    // no session means the bytes never reached an application at all.
    parts.push("TLS handshake had not completed");
  } else if (facts.tlsEstablished === true) {
    parts.push(`alpn=${facts.alpnProtocol ?? "none"}`);
    if (facts.tlsAuthorized === false) {
      parts.push(`peer unverified: ${facts.tlsAuthorizationError ?? "unknown"}`);
    }
  }
  return parts.join(", ");
}

interface SocketRecord {
  id: number;
  authority: string;
  firstSeenAt: number;
  requestCount: number;
  lastResponseAt: number | null;
  connectionHeader: string | null;
  keepAliveHeader: string | null;
  finAt: number | null;
  connected: boolean;
}

const RECORD = Symbol.for("brefwiz.apiBonesConnect.socketRecord");

type TrackedSocket = Socket & { [RECORD]?: SocketRecord };

type AgentInternals = http.Agent & {
  addRequest(request: ClientRequest, options: unknown): void;
  createConnection(options: object, callback: unknown): Socket;
};

/** Where the facts for one scoped call are delivered. */
export interface FactsSlot {
  facts: ConnectionFacts | null;
}

/**
 * Records connection facts and surrenders them once, to whoever asks first.
 *
 * Attribution is by recency rather than by identity: connect-node does not hand
 * the `ClientRequest` to an interceptor, so there is no key the two sides
 * share. In practice the interceptor asks immediately after its own call
 * rejected, and `take()` only returns facts recorded since that call began, so
 * a stale record is never attached to an unrelated failure. Under concurrent
 * in-flight failures to the same agent the pairing can swap — the facts carry
 * their own authority and timestamps, so a swapped pair is still readable, just
 * possibly attached to the sibling call.
 */
export class ConnectionFactsRecorder {
  #pending: { at: number; facts: ConnectionFacts }[] = [];
  readonly #scope = new AsyncLocalStorage<FactsSlot>();

  /**
   * Run one call so facts recorded on its behalf land in `slot`.
   *
   * HTTP/2 multiplexes many calls over one session, so pairing by arrival
   * order would hand one call another's evidence. A slot pairs exactly: the
   * session manager reads the slot of the call that reached it.
   */
  runScoped<T>(slot: FactsSlot, fn: () => Promise<T>): Promise<T> {
    return this.#scope.run(slot, fn);
  }

  /** The slot of the call now executing, when it runs under {@link runScoped}. */
  currentSlot(): FactsSlot | undefined {
    return this.#scope.getStore();
  }

  record(facts: ConnectionFacts): void {
    this.#pending.push({ at: Date.now(), facts });
    // Bound the queue: anything nobody claimed promptly is not going to be
    // claimed correctly later.
    if (this.#pending.length > 16) this.#pending.shift();
  }

  /** The most recent facts recorded at or after `since`, removed from the queue. */
  take(since: number): ConnectionFacts | null {
    for (let i = this.#pending.length - 1; i >= 0; i -= 1) {
      const entry = this.#pending[i];
      if (entry !== undefined && entry.at >= since) {
        this.#pending.splice(i, 1);
        return entry.facts;
      }
    }
    return null;
  }
}

let socketSeq = 0;

/** A connection id, unique within the process, shared by every transport version. */
export function nextSocketId(): number {
  socketSeq += 1;
  return socketSeq;
}

function trackSocket(socket: TrackedSocket, authority: string): SocketRecord {
  const existing = socket[RECORD];
  if (existing) return existing;

  const record: SocketRecord = {
    id: nextSocketId(),
    authority,
    firstSeenAt: Date.now(),
    requestCount: 0,
    lastResponseAt: null,
    connectionHeader: null,
    keepAliveHeader: null,
    finAt: null,
    // A socket first seen already open came from a pool; a fresh one is still
    // connecting and reports through its `connect` event.
    connected: !socket.connecting && socket.readyState !== "opening",
  };
  socket[RECORD] = record;
  socket.once("connect", () => {
    record.connected = true;
  });
  // `end` is the peer's FIN. Landing on an idle pooled socket, it is the exact
  // event that turns the next write into EPIPE.
  socket.once("end", () => {
    record.finAt = Date.now();
  });
  return record;
}

function headerValue(value: string | string[] | undefined): string | null {
  if (value === undefined) return null;
  return Array.isArray(value) ? value.join(", ") : value;
}

/**
 * How long a pooled socket may sit unused before the client closes it.
 *
 * Strictly below the idle timeout of every server the transport talks to
 * (ingress-nginx keeps client connections 75s and upstream ones 60s), so the
 * client is always the side that closes first and a pooled socket is never
 * handed out after the peer dropped it.
 */
export const POOLED_SOCKET_IDLE_MS = 30_000;

/**
 * Close sockets that stay unused in the pool for {@link POOLED_SOCKET_IDLE_MS}.
 *
 * `keepAliveMsecs` only schedules TCP probes; nothing in a Node agent ends an
 * idle pooled socket unless a timeout is armed when it returns to the pool.
 */
function expireIdleSockets(agent: http.Agent): void {
  agent.on("free", (socket: Socket) => {
    socket.setTimeout(POOLED_SOCKET_IDLE_MS);
    if (socket.listenerCount("timeout") === 0) {
      // The timeout stays armed while a request is in flight on the socket;
      // only a socket that is back in the pool is closed.
      socket.on("timeout", () => {
        const pooled = Object.values(agent.freeSockets).some((list) => list?.includes(socket));
        if (pooled) socket.destroy();
      });
    }
  });
}

/**
 * A pooling agent that records why its connections fail.
 *
 * Keep-alive is on deliberately: pooling is what makes a Node service fast, and
 * turning it off to dodge a stale-socket race trades a rare, now-diagnosable
 * failure for a TLS handshake on every call. Idle pooled sockets are closed
 * after {@link POOLED_SOCKET_IDLE_MS}, below any server idle timeout, so the
 * client is the side that closes first.
 */
export function createDiagnosticAgent(
  secure: boolean,
  recorder: ConnectionFactsRecorder,
  tls?: NodeTlsIdentitySource,
): http.Agent {
  const options = { keepAlive: true, keepAliveMsecs: 1_000 };
  const agent: http.Agent = secure ? new https.Agent(options) : new http.Agent(options);
  const internals = agent as AgentInternals;
  expireIdleSockets(agent);
  const addRequest = internals.addRequest.bind(internals);

  if (tls !== undefined && secure) {
    // Resolved per connection rather than folded into the agent's constructor
    // options, so an identity that rotates is picked up without rebuilding the
    // agent — rebuilding is what a caller doing this by hand must do, and it
    // discards the socket pool every time. The pool key (below) is derived from the
    // resolved material, so a rotation opens a fresh bucket by itself and
    // sockets already established finish under the identity they started with.
    const createConnection = internals.createConnection.bind(internals);
    internals.createConnection = (connectionOptions: object, callback: unknown) =>
      createConnection({ ...connectionOptions, ...resolveTlsIdentity(tls) }, callback);
    // The pool key is computed from the request's options, ahead of the merge
    // above, so fold the same resolved TLS options into it: a pin's session
    // scope and a rotated identity then each get their own bucket, and a socket
    // opened under one pin is never reused under another.
    const getName = agent.getName.bind(agent);
    agent.getName = (nameOptions: Parameters<http.Agent["getName"]>[0]) =>
      getName({ ...nameOptions, ...resolveTlsIdentity(tls) });
  }

  internals.addRequest = (request: ClientRequest, requestOptions: unknown): void => {
    let record: SocketRecord | null = null;
    let socket: TrackedSocket | null = null;
    let assignedAt = 0;
    let reused = false;
    let priorRequests = 0;
    let idleBeforeAssignMs: number | null = null;
    let responseStarted = false;

    request.once("socket", (assigned: TrackedSocket) => {
      const seenBefore = assigned[RECORD] !== undefined;
      const authority = `${request.host ?? "?"}`;
      record = trackSocket(assigned, authority);
      socket = assigned;
      assignedAt = Date.now();
      reused = seenBefore || assigned.readyState === "open";
      priorRequests = record.requestCount;
      idleBeforeAssignMs =
        record.lastResponseAt === null ? null : assignedAt - record.lastResponseAt;
      record.requestCount += 1;
    });

    request.once("response", (response: IncomingMessage) => {
      responseStarted = true;
      if (record === null) return;
      record.lastResponseAt = Date.now();
      record.connectionHeader = headerValue(response.headers.connection);
      record.keepAliveHeader = headerValue(response.headers["keep-alive"]);
    });

    request.on("error", () => {
      if (record === null || socket === null) return;
      const current = record;
      const wire = socket;
      // The peer's FIN and the request error routinely land in the same tick,
      // and whether the FIN preceded the write is the question worth answering.
      // Defer one macrotask so the settled state is what gets recorded.
      setImmediate(() => {
        const now = Date.now();
        recorder.record({
          socketId: current.id,
          authority: current.authority,
          reused,
          priorRequests,
          ageAtAssignMs: assignedAt - current.firstSeenAt,
          ageAtFailureMs: now - current.firstSeenAt,
          idleBeforeAssignMs,
          finBeforeFailureMs: current.finAt === null ? null : now - current.finAt,
          serverConnectionHeader: current.connectionHeader,
          serverKeepAliveHeader: current.keepAliveHeader,
          bytesWritten: wire.bytesWritten,
          bytesRead: wire.bytesRead,
          responseStarted,
          connected: current.connected,
          unprocessed: false,
          http2: false,
          ...readTlsState(wire),
        });
      });
    });

    addRequest(request, requestOptions);
  };

  return agent;
}

const RESET_SIGNATURES: readonly string[] = ["econnreset", "connection reset", "epipe"];

/** An Aborted failure whose underlying cause is a reset connection. */
function isAbortedByReset(err: ConnectError): boolean {
  if (err.code !== Code.Aborted) return false;
  const message = err.rawMessage.toLowerCase();
  return RESET_SIGNATURES.some((signature) => message.includes(signature));
}

/** The host a request URL addresses, in the form the agent records it. */
function hostOf(url: unknown): string | null {
  if (typeof url !== "string") return null;
  try {
    return new URL(url).hostname.replace(/^\[|\]$/g, "");
  } catch {
    return null;
  }
}

/**
 * Explains connection-write failures and marks failures that never opened a
 * connection.
 *
 * A connection-write failure is rewritten to say why the connection was gone,
 * and marked as not delivered when its facts prove the request never left.
 * Any other failure that could be the transport's own (Unavailable, or an
 * Internal that is not a server fault) is only ever marked, and only when the
 * facts recorded for THIS host show a socket that never connected -- refused,
 * unresolved, timed out or unroutable -- so its message and code are untouched.
 * Everything else, including a failure the retry interceptor already replayed
 * away, passes through, so this never changes what a caller catches, only what
 * the message tells them.
 */
export function makeConnectionDiagnosticsInterceptor(
  recorder: ConnectionFactsRecorder,
): Interceptor {
  return (next) => async (req) => {
    const startedAt = Date.now();
    const slot: FactsSlot = { facts: null };
    try {
      return await recorder.runScoped(slot, () => next(req));
    } catch (err) {
      if (!(err instanceof ConnectError)) throw err;
      const writeFailure = isConnectionWriteFailure(err);
      const reset = isAbortedByReset(err);
      if (
        !writeFailure &&
        !reset &&
        err.code !== Code.Unavailable &&
        err.code !== Code.Internal
      ) {
        throw err;
      }
      // The agent records on setImmediate; yield once so the facts for this
      // very failure are in the queue before we look.
      await new Promise<void>((resolve) => setImmediate(resolve));
      const facts = slot.facts ?? recorder.take(startedAt);
      if (facts === null) throw err;
      if (reset) {
        // Connect reports a reset on the read side as Aborted. Whether it hit
        // before any response byte is the evidence a method with no side
        // effects needs to be replayed; a reset mid-response is left alone.
        const host = hostOf((req as { url?: unknown }).url);
        const sameHost = host === null || host === facts.authority;
        throw !facts.responseStarted && sameHost ? markResetBeforeResponse(err) : err;
      }
      if (!writeFailure) {
        // A server-sent Unavailable records nothing, so facts found here belong
        // to a sibling call unless the host matches; a mismatch is not evidence.
        const host = hostOf((req as { url?: unknown }).url);
        const sameHost = host === null || host === facts.authority;
        throw (!facts.connected || facts.unprocessed) && sameHost ? markNotDelivered(err) : err;
      }
      const explained = new ConnectError(
        `${err.rawMessage} (${formatConnectionFacts(facts)})`,
        Code.Internal,
        err.metadata,
        undefined,
        err.cause,
      );
      // The retry interceptor sits outside this one and replays a marked
      // failure for any method: the evidence, not the message, is its input.
      throw requestNeverLeft(facts) ? markNotDelivered(explained) : explained;
    }
  };
}
