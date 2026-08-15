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

import http from "node:http";
import https from "node:https";
import type { ClientRequest, IncomingMessage } from "node:http";
import type { Socket } from "node:net";

import { Code, ConnectError, type Interceptor } from "@connectrpc/connect";

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

/** Live identity, resolved per connection so a rotating SVID needs no rebuild. */
export type NodeTlsIdentitySource = NodeTlsIdentity | (() => NodeTlsIdentity);

export function resolveTlsIdentity(
  source: NodeTlsIdentitySource | undefined,
): NodeTlsIdentity {
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
}

const RECORD = Symbol.for("brefwiz.apiBonesConnect.socketRecord");

type TrackedSocket = Socket & { [RECORD]?: SocketRecord };

type AgentInternals = http.Agent & {
  addRequest(request: ClientRequest, options: unknown): void;
  createConnection(options: object, callback: unknown): Socket;
};

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

function trackSocket(socket: TrackedSocket, authority: string): SocketRecord {
  const existing = socket[RECORD];
  if (existing) return existing;

  socketSeq += 1;
  const record: SocketRecord = {
    id: socketSeq,
    authority,
    firstSeenAt: Date.now(),
    requestCount: 0,
    lastResponseAt: null,
    connectionHeader: null,
    keepAliveHeader: null,
    finAt: null,
  };
  socket[RECORD] = record;
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
 * A pooling agent that records why its connections fail.
 *
 * Keep-alive is on deliberately: pooling is what makes a Node service fast, and
 * turning it off to dodge a stale-socket race trades a rare, now-diagnosable
 * failure for a TLS handshake on every call. `keepAliveMsecs` is set below the
 * shortest idle timeout any reasonable server runs, so the client is the side
 * that closes first and the race has no window to open in.
 */
export function createDiagnosticAgent(
  secure: boolean,
  recorder: ConnectionFactsRecorder,
  tls?: NodeTlsIdentitySource,
): http.Agent {
  // Node's own default is 1000ms; naming it here is the point — this value is
  // load-bearing, not incidental, and it must stay strictly below the server's
  // idle timeout (hyper, Go and nginx all default to 5s or more).
  const options = { keepAlive: true, keepAliveMsecs: 1_000 };
  const agent: http.Agent = secure ? new https.Agent(options) : new http.Agent(options);
  const internals = agent as AgentInternals;
  const addRequest = internals.addRequest.bind(internals);

  if (tls !== undefined && secure) {
    // Resolved per connection rather than folded into the agent's constructor
    // options, so an identity that rotates is picked up without rebuilding the
    // agent — rebuilding is what a caller doing this by hand must do, and it
    // discards the socket pool every time. Node derives its pool key from the
    // resolved material, so a rotation opens a fresh bucket by itself and
    // sockets already established finish under the identity they started with.
    const createConnection = internals.createConnection.bind(internals);
    internals.createConnection = (connectionOptions: object, callback: unknown) =>
      createConnection({ ...connectionOptions, ...resolveTlsIdentity(tls) }, callback);
  }

  internals.addRequest = (request: ClientRequest, requestOptions: unknown): void => {
    let record: SocketRecord | null = null;
    let socket: TrackedSocket | null = null;
    let assignedAt = 0;
    let reused = false;
    let priorRequests = 0;
    let idleBeforeAssignMs: number | null = null;

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
        });
      });
    });

    addRequest(request, requestOptions);
  };

  return agent;
}

/**
 * Rewrites a connection-write failure to say why the connection was gone.
 *
 * Only that failure shape is touched. Every other error — including a
 * connection-write failure the retry interceptor already replayed away — passes
 * through untouched, so this never changes what a caller catches, only what the
 * message tells them.
 */
export function makeConnectionDiagnosticsInterceptor(
  recorder: ConnectionFactsRecorder,
): Interceptor {
  return (next) => async (req) => {
    const startedAt = Date.now();
    try {
      return await next(req);
    } catch (err) {
      if (!(err instanceof ConnectError) || !isConnectionWriteFailure(err)) throw err;
      // The agent records on setImmediate; yield once so the facts for this
      // very failure are in the queue before we look.
      await new Promise<void>((resolve) => setImmediate(resolve));
      const facts = recorder.take(startedAt);
      if (facts === null) throw err;
      throw new ConnectError(
        `${err.rawMessage} (${formatConnectionFacts(facts)})`,
        Code.Internal,
        err.metadata,
        undefined,
        err.cause,
      );
    }
  };
}
