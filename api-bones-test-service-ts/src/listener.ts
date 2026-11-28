// SPDX-License-Identifier: MIT
import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";
import * as http2 from "node:http2";
import * as net from "node:net";

/** A request handler both HTTP/1.1 and HTTP/2 serve: the two request types share the parts handlers read. */
export type Handle = (request: IncomingMessage | http2.Http2ServerRequest, response: ServerResponse | http2.Http2ServerResponse) => void;

/** The preface every HTTP/2 prior-knowledge connection opens with; no HTTP/1.1 method is spelled this way. */
const H2_PREFACE = "PRI ";

/**
 * One loopback listener that answers HTTP/1.1 (a browser page) and cleartext
 * HTTP/2 with prior knowledge (the wasm SDK under Node), told apart by the
 * first bytes of each connection.
 *
 * An HTTP/1.1 connection is handed to the HTTP/1.1 server with the bytes read
 * so far put back. Node's HTTP/2 session reads the socket's native handle and
 * never sees bytes already read into JavaScript, so an HTTP/2 connection is
 * relayed, preface included, to a loopback HTTP/2 server of its own.
 */
export class Listener {
  readonly #sockets = new Set<net.Socket>();
  readonly #h1: Server;
  readonly #h2: http2.Http2Server;
  readonly #server: net.Server;
  #relay: net.AddressInfo | undefined;

  constructor(handle: Handle) {
    this.#h1 = createServer(handle as (request: IncomingMessage, response: ServerResponse) => void);
    this.#h2 = http2.createServer(handle as (request: http2.Http2ServerRequest, response: http2.Http2ServerResponse) => void);
    this.#server = net.createServer((socket) => {
      this.#sockets.add(socket);
      socket.once("close", () => this.#sockets.delete(socket));
      socket.on("error", () => undefined);
      socket.once("data", (first: Buffer) => {
        if (first.toString("latin1", 0, H2_PREFACE.length) === H2_PREFACE && this.#relay !== undefined) {
          this.#relayTo(socket, first, this.#relay);
          return;
        }
        socket.pause();
        socket.unshift(first);
        this.#h1.emit("connection", socket);
        socket.resume();
      });
    });
  }

  listen(port: number, host: string, ready: () => void): void {
    this.#h2.listen(0, host, () => {
      this.#relay = this.#h2.address() as net.AddressInfo;
      this.#server.listen(port, host, ready);
    });
  }

  #relayTo(socket: net.Socket, first: Buffer, to: net.AddressInfo): void {
    const upstream = net.connect(to.port, to.address);
    this.#sockets.add(upstream);
    upstream.once("close", () => {
      this.#sockets.delete(upstream);
      socket.destroy();
    });
    socket.once("close", () => upstream.destroy());
    upstream.on("error", () => undefined);
    upstream.write(first);
    socket.pipe(upstream);
    upstream.pipe(socket);
  }

  address(): net.AddressInfo | string | null {
    return this.#server.address();
  }

  closeAllConnections(): void {
    for (const socket of this.#sockets) socket.destroy();
  }

  close(done: () => void): void {
    this.#h2.close();
    this.#server.close(() => done());
  }
}
