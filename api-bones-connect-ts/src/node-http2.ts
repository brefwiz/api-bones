// SPDX-License-Identifier: MIT
//
// HTTP/2 session management for the Node transport.
//
// Two jobs, both about a session that outlives the calls riding on it. It keeps
// an idle session honest with PING frames so a peer that silently went away is
// evicted before a call reuses it, and it records, per call, what the session
// lifecycle proves about whether the request was ever acted on.

import * as http2 from "node:http2";

import { Http2SessionManager, type Http2SessionOptions } from "@connectrpc/connect-node";

import {
  ConnectionFactsRecorder,
  nextSocketId,
  type ConnectionFacts,
  type FactsSlot,
} from "./node-diagnostics.js";

/**
 * Keepalive for HTTP/2 sessions: ping every 30s including while idle, and
 * treat a session that does not answer within 15s as dead.
 *
 * A session reused after that silence is verified first and replaced when the
 * ping goes unanswered, so a call is not written onto a connection the peer
 * dropped without a FIN.
 */
export const HTTP2_KEEPALIVE: Required<
  Pick<Http2SessionOptions, "pingIntervalMs" | "pingTimeoutMs" | "pingIdleConnection">
> = {
  pingIntervalMs: 30_000,
  pingTimeoutMs: 15_000,
  pingIdleConnection: true,
};

type SessionOptions = http2.ClientSessionOptions | http2.SecureClientSessionOptions;

interface SessionRecord {
  id: number;
  openedAt: number;
  streams: number;
  /** Highest stream id the peer said it processed, once it sent a GOAWAY. */
  lastStreamId: number | null;
}

/**
 * A session manager that keeps sessions alive and reports, for the call being
 * made, whether the request provably never reached the peer's application.
 *
 * The evidence lands in the {@link FactsSlot} of the call that is executing,
 * so concurrent calls sharing one session never receive each other's facts.
 * Two situations count as never delivered:
 *
 * - no stream was opened: the connection could not be made, or the session
 *   refused a new stream, so no request frame was written;
 * - the peer refused the stream (`REFUSED_STREAM`) or sent a GOAWAY naming a
 *   last processed stream below this one, and no response had begun. HTTP/2
 *   guarantees such a stream was not processed.
 */
export class ObservedHttp2SessionManager {
  readonly authority: string;
  readonly #inner: Http2SessionManager;
  readonly #recorder: ConnectionFactsRecorder;
  readonly #host: string;
  readonly #secure: boolean;
  readonly #sessions = new WeakMap<http2.Http2Session, SessionRecord>();

  constructor(
    url: string,
    recorder: ConnectionFactsRecorder,
    sessionOptions?: SessionOptions,
    ping: Http2SessionOptions = HTTP2_KEEPALIVE,
  ) {
    const parsed = new URL(url);
    this.authority = parsed.origin;
    this.#host = parsed.hostname.replace(/^\[|\]$/g, "");
    this.#secure = parsed.protocol === "https:";
    this.#recorder = recorder;
    this.#inner = new Http2SessionManager(url, ping, sessionOptions);
  }

  /** The wrapped manager's connection state, for observing eviction. */
  state(): ReturnType<Http2SessionManager["state"]> {
    return this.#inner.state();
  }

  async request(
    method: string,
    path: string,
    headers: http2.OutgoingHttpHeaders,
    options: Omit<http2.ClientSessionRequestOptions, "signal">,
  ): Promise<http2.ClientHttp2Stream> {
    const slot = this.#recorder.currentSlot();
    let stream: http2.ClientHttp2Stream;
    try {
      stream = await this.#inner.request(method, path, headers, options);
    } catch (err) {
      if (slot !== undefined) {
        const state = this.#inner.state();
        slot.facts = this.#facts(null, {
          connected: state !== "error" && state !== "closed",
          unprocessed: true,
        });
      }
      throw err;
    }
    if (slot !== undefined) this.#watch(stream, slot);
    return stream;
  }

  notifyResponseByteRead(stream: http2.ClientHttp2Stream): void {
    this.#inner.notifyResponseByteRead(stream);
  }

  #record(session: http2.Http2Session): SessionRecord {
    let record = this.#sessions.get(session);
    if (record === undefined) {
      record = { id: nextSocketId(), openedAt: Date.now(), streams: 0, lastStreamId: null };
      const seen = record;
      this.#sessions.set(session, seen);
      session.on("goaway", (_code: number, lastStreamId: number) => {
        seen.lastStreamId = lastStreamId;
      });
    }
    return record;
  }

  #watch(stream: http2.ClientHttp2Stream, slot: FactsSlot): void {
    const session = stream.session;
    if (session === undefined) return;
    const record = this.#record(session);
    record.streams += 1;
    const prior = record.streams - 1;
    let responded = false;
    stream.once("response", () => {
      responded = true;
    });
    stream.once("close", () => {
      const refused = stream.rstCode === http2.constants.NGHTTP2_REFUSED_STREAM;
      const beyondGoaway =
        record.lastStreamId !== null &&
        stream.id !== undefined &&
        stream.id > record.lastStreamId;
      slot.facts = this.#facts(record, {
        connected: true,
        unprocessed: !responded && (refused || beyondGoaway),
        priorRequests: prior,
      });
    });
  }

  #facts(
    record: SessionRecord | null,
    at: { connected: boolean; unprocessed: boolean; priorRequests?: number },
  ): ConnectionFacts {
    const age = record === null ? 0 : Date.now() - record.openedAt;
    return {
      socketId: record?.id ?? nextSocketId(),
      authority: this.#host,
      reused: (at.priorRequests ?? 0) > 0,
      priorRequests: at.priorRequests ?? 0,
      ageAtAssignMs: age,
      ageAtFailureMs: age,
      idleBeforeAssignMs: null,
      finBeforeFailureMs: null,
      serverConnectionHeader: null,
      serverKeepAliveHeader: null,
      bytesWritten: 0,
      bytesRead: 0,
      connected: at.connected,
      unprocessed: at.unprocessed,
      http2: true,
      tlsEstablished: this.#secure ? at.connected : null,
      alpnProtocol: this.#secure && at.connected ? "h2" : null,
      tlsAuthorized: null,
      tlsAuthorizationError: null,
    };
  }
}
