// SPDX-License-Identifier: MIT
import type { DescMethodUnary } from "@bufbuild/protobuf";
import { EmptySchema, StringValueSchema } from "@bufbuild/protobuf/wkt";
import type { Transport } from "@connectrpc/connect";
import { once } from "node:events";
import * as http2 from "node:http2";
import { connect, createServer, type AddressInfo, type Socket } from "node:net";
import { afterEach, describe, expect, it } from "vitest";

import { configureNodeConnectTransport } from "./node.js";
import { ConnectionFactsRecorder } from "./node-diagnostics.js";
import { HTTP2_KEEPALIVE, ObservedHttp2SessionManager } from "./node-http2.js";
import { MAX_RETRY_ATTEMPTS, type RetryEvent } from "./retry.js";

const { NGHTTP2_NO_ERROR, NGHTTP2_REFUSED_STREAM, NGHTTP2_INTERNAL_ERROR } = http2.constants;

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

const unspecifiedPolicy = {
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

interface Peer {
  port: number;
  calls: () => number;
  sockets: Socket[];
  sessions: () => number;
}

const open: http2.Http2Server[] = [];
const sockets: Socket[] = [];
const relays: ReturnType<typeof createServer>[] = [];

afterEach(() => {
  for (const socket of sockets.splice(0)) socket.destroy();
  for (const server of open.splice(0)) server.close();
  for (const relay of relays.splice(0)) relay.close();
});

/** A cleartext HTTP/2 server whose `onStream` decides every call's fate. */
async function serve(
  onStream: (stream: http2.ServerHttp2Stream, call: number) => void,
): Promise<Peer> {
  const server = http2.createServer();
  let calls = 0;
  let sessions = 0;
  server.on("connection", (socket) => sockets.push(socket));
  server.on("session", () => {
    sessions += 1;
  });
  server.on("stream", (stream) => {
    calls += 1;
    stream.on("error", () => {});
    stream.resume();
    onStream(stream, calls);
  });
  open.push(server);
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  return {
    port: (server.address() as AddressInfo).port,
    calls: () => calls,
    sockets,
    sessions: () => sessions,
  };
}

function succeed(stream: http2.ServerHttp2Stream): void {
  stream.respond({ ":status": 200, "content-type": "application/proto" });
  stream.end();
}

async function serviceTransport(baseUrl: string, events: RetryEvent[]): Promise<Transport> {
  return configureNodeConnectTransport({
    baseUrl,
    profile: "service",
    httpVersion: "2",
    policy: unspecifiedPolicy,
    retry: { initialDelayMs: 0, maxDelayMs: 0 },
    onRetry: (event) => events.push(event),
  });
}

async function callOnce(baseUrl: string, events: RetryEvent[]): Promise<unknown> {
  const transport = await serviceTransport(baseUrl, events);
  return send(transport);
}

function send(transport: Transport): Promise<unknown> {
  return transport.unary(getMethod, undefined, undefined, undefined, { value: "x" });
}

async function closedPort(): Promise<number> {
  return new Promise<number>((resolve, reject) => {
    const probe = createServer();
    probe.on("error", reject);
    probe.listen(0, "127.0.0.1", () => {
      const { port } = probe.address() as AddressInfo;
      probe.close(() => resolve(port));
    });
  });
}

describe("HTTP/2 requests that never left the client", () => {
  it("replays a refused connection on an UNSPECIFIED method", async () => {
    const port = await closedPort();
    const events: RetryEvent[] = [];
    await expect(callOnce(`http://127.0.0.1:${port}`, events)).rejects.toThrow();
    expect(events).toHaveLength(MAX_RETRY_ATTEMPTS);
  }, 10_000);

  it("replays a REFUSED_STREAM on an UNSPECIFIED method", async () => {
    const peer = await serve((stream, call) => {
      if (call <= 2) stream.close(NGHTTP2_REFUSED_STREAM);
      else succeed(stream);
    });
    const events: RetryEvent[] = [];
    await callOnce(`http://127.0.0.1:${peer.port}`, events);
    expect(events).toHaveLength(2);
    expect(peer.calls()).toBe(3);
  }, 10_000);

  it("replays a stream above the GOAWAY last stream id on an UNSPECIFIED method", async () => {
    const peer = await serve((stream, call) => {
      // The second stream is numbered 3; announcing 1 as the last processed
      // stream tells the client stream 3 was never acted on.
      if (call === 2) stream.session?.goaway(NGHTTP2_NO_ERROR, 1);
      else succeed(stream);
    });
    const events: RetryEvent[] = [];
    const transport = await serviceTransport(`http://127.0.0.1:${peer.port}`, events);
    await send(transport);
    await send(transport);
    expect(events).toHaveLength(1);
    expect(peer.calls()).toBe(3);
  }, 10_000);

  it("does not replay a stream the peer already processed", async () => {
    const peer = await serve((stream) => {
      stream.close(NGHTTP2_INTERNAL_ERROR);
    });
    const events: RetryEvent[] = [];
    await expect(callOnce(`http://127.0.0.1:${peer.port}`, events)).rejects.toThrow();
    expect(events).toHaveLength(0);
    expect(peer.calls()).toBe(1);
  }, 10_000);
});

describe("HTTP/2 keepalive", () => {
  it("pings idle sessions every 30s and gives an unanswered ping 15s", () => {
    expect(HTTP2_KEEPALIVE).toEqual({
      pingIntervalMs: 30_000,
      pingTimeoutMs: 15_000,
      pingIdleConnection: true,
    });
  });

  it("evicts an idle session that stops answering pings before the next call", async () => {
    const peer = await serve((stream) => succeed(stream));
    const relay = await blackholeRelay(peer.port);
    const manager = new ObservedHttp2SessionManager(
      `http://127.0.0.1:${relay.port}`,
      new ConnectionFactsRecorder(),
      undefined,
      { pingIntervalMs: 40, pingTimeoutMs: 40, pingIdleConnection: true },
    );
    const exchange = async (): Promise<void> => {
      const stream = await manager.request(
        "POST",
        "/pkg.v1.Svc/Get",
        { "content-type": "application/proto" },
        { endStream: true },
      );
      stream.resume();
      await once(stream, "close");
    };

    await exchange();
    expect(peer.sessions()).toBe(1);

    // Every frame on the live connection is now swallowed, so no ping is answered.
    relay.silenceExisting();
    await new Promise((resolve) => setTimeout(resolve, 400));
    expect(["error", "closed"]).toContain(manager.state());

    await exchange();
    expect(peer.sessions()).toBe(2);
  }, 10_000);
});

/** A TCP relay in front of the peer that can silence the connections already open. */
async function blackholeRelay(
  targetPort: number,
): Promise<{ port: number; silenceExisting: () => void }> {
  const silenced = new Set<Socket>();
  const relay = createServer((client) => {
    const upstream = connect(targetPort, "127.0.0.1");
    sockets.push(client, upstream);
    client.on("data", (data) => {
      if (!silenced.has(client)) upstream.write(data);
    });
    upstream.on("data", (data) => {
      if (!silenced.has(client)) client.write(data);
    });
    client.on("error", () => upstream.destroy());
    upstream.on("error", () => client.destroy());
    client.on("close", () => upstream.destroy());
    upstream.on("close", () => client.destroy());
    live.add(client);
  });
  const live = new Set<Socket>();
  relays.push(relay);
  relay.listen(0, "127.0.0.1");
  await once(relay, "listening");
  return {
    port: (relay.address() as AddressInfo).port,
    silenceExisting: () => {
      for (const client of live) silenced.add(client);
    },
  };
}
