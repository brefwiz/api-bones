// SPDX-License-Identifier: MIT
/**
 * The api-bones test service, run as its own process: the
 * `Widgets`, `Numbers` and `Events` services of the test contract on one
 * loopback address, recording what each request carried. It is the TypeScript
 * twin of the Rust lane's server in `tests/sdk-call/rust/tests/sdk_call.rs`.
 *
 * Callers reach the services only over the wire, through the generated
 * wrapper. They script and read the server through a control endpoint on a
 * second address (`POST /reset`, `POST /configure`, `GET /seen`,
 * `POST /seen/clear`, `POST /alias`, `POST /ack/<id>`).
 *
 * The caller's credential state lives in the steps process. A scripted change
 * of the caller (`switchTo`, `switchOnPage`, `refreshTo`) is published on
 * stdout as one JSON line `{"event":"caller","id":N,...}`; the server holds the
 * response until the steps process has applied it and acked it on the control
 * endpoint, so the order is the protocol's, never a timing.
 *
 * The server narrates its decisions on stderr, one line `sdk-call-server
 * {json}` each, in time order with the steps' own output: every scenario reset,
 * every list request (cursor, the revision-cache headers it carried, the answer
 * it got), every stream tick and every client abort, with milliseconds since
 * process start (`t`) and since the request began (`ms`).
 *
 * Startup prints one JSON line `{"ready":true,"url":...,"control":...}`; the
 * process ends when its stdin closes.
 */

import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";
import type { AddressInfo } from "node:net";
import { setTimeout as delay } from "node:timers/promises";
import { create, type DescMessage, type DescMethodServerStreaming, type DescMethodUnary, type DescService, type MessageShape } from "@bufbuild/protobuf";
import { Code, ConnectError } from "@connectrpc/connect";
import { connectNodeAdapter } from "@connectrpc/connect-node";
import { encodeErrorInfo } from "@brefwiz/api-bones-connect";
import { contractRegistry } from "./contract.js";
import type { Patch, Seen } from "./types.js";

/** The scenario the server last heard of; every line it narrates names it. */
let scenario = "";

/** Writes one decision of the server on stderr, which the steps process passes through. */
function note(event: string, fields: Record<string, unknown> = {}): void {
  process.stderr.write(`sdk-call-server ${JSON.stringify({ event, scenario, t: Math.round(performance.now()), ...fields })}\n`);
}


interface Services {
  widgets: DescService;
  numbers: DescService;
  events: DescService;
}

let services: Services | undefined;

function contract(): Services {
  const registry = contractRegistry();
  services ??= {
    widgets: registry.getService("contract.v1.Widgets")!,
    numbers: registry.getService("contract.v1.Numbers")!,
    events: registry.getService("contract.v1.Events")!,
  };
  return services;
}

function method<T>(service: DescService, name: string): T {
  const found = service.methods.find((candidate) => candidate.name === name);
  if (found === undefined) throw new Error(`the contract has no method ${name}`);
  return found as T;
}

/** Spends one of the refusals `owed` still counts; whether one was owed. */
function spend(owed: { count: number }): boolean {
  if (owed.count <= 0) return false;
  owed.count -= 1;
  return true;
}

class SdkCallServer {
  /** The widgets the server holds, by id. */
  readonly widgets = new Map<string, string>();
  /** The pages of parts each widget has, by widget id; the cursor `cN` names page `N`. */
  readonly parts = new Map<string, string[][]>();
  /** Revision of the current scripted content; changes with every reset and every scripted list, so a cached chain never matches a later scenario. */
  private revisionCounter = 0;
  private get revision(): string {
    return `r${this.revisionCounter}`;
  }
  /** The pages the numbers server serves; the cursor `cN` names page `N`. */
  pages: number[][] = [];
  pageDelayMs = 0;
  ticks = 0;
  tickMs = 0;
  readonly seen: Seen[] = [];
  readonly unavailable = { count: 0 };
  readonly unauthenticated = { count: 0 };
  /** The subject the server switches the caller to while it handles a request. */
  switchTo: string | undefined;
  /** The subject the server switches the caller to while it handles the request for the zero-based page. */
  switchOnPage: { index: number; subject: string } | undefined;
  /** The `authorization` value the server gives the caller's source while it handles a request. */
  refreshTo: string | undefined;
  /** The zero-based page whose cursor the server refuses as expired. */
  expireAtPage: number | undefined;
  /** Whether the numbers server answers with bytes no decoder reads. */
  garble = false;
  #server: Server | undefined;
  #alias: Server | undefined;
  #handle: ((request: IncomingMessage, response: ServerResponse) => void) | undefined;

  get url(): string {
    const address = this.#server?.address() as AddressInfo | null | undefined;
    if (!address) throw new Error("the lane's server is not started");
    return `http://127.0.0.1:${address.port}`;
  }

  /** Forgets everything one scenario scripted or recorded, and closes what it left open. */
  async reset(): Promise<void> {
    note("reset", { seen: this.seen.length });
    this.widgets.clear();
    this.parts.clear();
    this.pages = [];
    this.revisionCounter += 1;
    this.pageDelayMs = 0;
    this.ticks = 0;
    this.tickMs = 0;
    this.seen.length = 0;
    this.unavailable.count = 0;
    this.unauthenticated.count = 0;
    this.switchTo = undefined;
    this.switchOnPage = undefined;
    this.refreshTo = undefined;
    this.expireAtPage = undefined;
    this.garble = false;
    releaseHeld();
    this.#server?.closeAllConnections();
    const alias = this.#alias;
    this.#alias = undefined;
    if (alias !== undefined) {
      alias.closeAllConnections();
      await new Promise<void>((resolve) => alias.close(() => resolve()));
    }
  }

  /** Applies what a scenario scripts; a `null` clears a scripted change. */
  configure(patch: Patch): void {
    scenario = patch.scenario ?? scenario;
    for (const [id, name] of Object.entries(patch.widgets ?? {})) this.widgets.set(id, name);
    for (const [id, pages] of Object.entries(patch.parts ?? {})) this.parts.set(id, pages);
    if (patch.parts !== undefined || patch.pages !== undefined) this.revisionCounter += 1;
    if (patch.pages !== undefined) this.pages = patch.pages;
    if (patch.pageDelayMs !== undefined) this.pageDelayMs = patch.pageDelayMs;
    if (patch.ticks !== undefined) this.ticks = patch.ticks;
    if (patch.tickMs !== undefined) this.tickMs = patch.tickMs;
    if (patch.unavailable !== undefined) this.unavailable.count = patch.unavailable;
    if (patch.unauthenticated !== undefined) this.unauthenticated.count = patch.unauthenticated;
    if (patch.switchTo !== undefined) this.switchTo = patch.switchTo ?? undefined;
    if (patch.switchOnPage !== undefined) this.switchOnPage = patch.switchOnPage ?? undefined;
    if (patch.refreshTo !== undefined) this.refreshTo = patch.refreshTo ?? undefined;
    if (patch.expireAtPage !== undefined) this.expireAtPage = patch.expireAtPage ?? undefined;
    if (patch.garble !== undefined) this.garble = patch.garble;
  }

  async #admit(requestHeader: Headers, pageSize = 0): Promise<void> {
    this.seen.push({
      traceparent: requestHeader.get("traceparent") ?? "",
      authorization: requestHeader.get("authorization") ?? "",
      timeoutMs: Number(requestHeader.get("connect-timeout-ms") ?? 0) || 0,
      listCache: requestHeader.get("bones-list-cache") ?? "",
      ifNoneMatch: requestHeader.get("if-none-match") ?? "",
      pageSize,
    });
    if (this.refreshTo !== undefined) {
      const authorization = this.refreshTo;
      this.refreshTo = undefined;
      await changeCaller({ authorization });
    }
    if (spend(this.unauthenticated)) throw new ConnectError("the caller is not signed in", Code.Unauthenticated);
    if (spend(this.unavailable)) throw new ConnectError("the server is down", Code.Unavailable);
  }

  async #switchIfScripted(): Promise<void> {
    if (this.switchTo === undefined) return;
    const subject = this.switchTo;
    this.switchTo = undefined;
    await changeCaller({ subject });
  }

  async #page(cursor: string): Promise<{ items: number[]; page: { nextCursor: string; revision: string } }> {
    const index = /^c(\d+)$/.exec(cursor)?.[1] === undefined ? 0 : Number(cursor.slice(1));
    if (this.switchOnPage?.index === index) {
      const { subject } = this.switchOnPage;
      this.switchOnPage = undefined;
      await changeCaller({ subject });
    }
    if (this.expireAtPage === index) {
      const expired = new ConnectError("the cursor expired", Code.FailedPrecondition);
      expired.details.push({ type: "bones.v1.ErrorInfo", value: encodeErrorInfo({ code: "CURSOR_EXPIRED", emitter: "numbers" }) });
      throw expired;
    }
    if (this.pageDelayMs > 0) await delay(this.pageDelayMs);
    return {
      items: this.pages[index] ?? [],
      page: { nextCursor: index + 1 < this.pages.length ? `c${index + 1}` : "", revision: this.revision },
    };
  }

  async start(): Promise<void> {
    if (this.#server !== undefined) return;
    const { widgets, numbers, events } = contract();
    const handler = connectNodeAdapter({
      routes: (router) => {
        const getWidget = method<DescMethodUnary>(widgets, "GetWidget");
        router.rpc(getWidget, async (request, context) => {
          const asked = request as unknown as { id: string };
          await this.#admit(context.requestHeader);
          await this.#switchIfScripted();
          const name = this.widgets.get(asked.id);
          if (name === undefined) throw new ConnectError("no such widget", Code.NotFound);
          return create(getWidget.output, { id: asked.id, name } as never);
        });
        const batch = method<DescMethodUnary>(widgets, "BatchGetWidgets");
        router.rpc(batch, async (request, context) => {
          const asked = request as unknown as { ids: string[] };
          await this.#admit(context.requestHeader);
          await this.#switchIfScripted();
          const results = asked.ids.map((id) => {
            const name = this.widgets.get(id);
            return name === undefined
              ? { id, outcome: { case: "error", value: { code: "not_found", message: "no such widget" } } }
              : { id, outcome: { case: "item", value: { id, name } } };
          });
          return create(batch.output, { results } as never);
        });
        const listParts = method<DescMethodUnary>(widgets, "ListWidgetParts");
        router.rpc(listParts, async (request, context) => {
          const asked = request as unknown as { widgetId: string; page?: { cursor: string } };
          await this.#admit(context.requestHeader);
          if (!this.widgets.has(asked.widgetId)) throw new ConnectError("no such widget", Code.NotFound);
          const cursor = asked.page?.cursor ?? "";
          const index = /^c(\d+)$/.exec(cursor)?.[1] === undefined ? 0 : Number(cursor.slice(1));
          const pages = this.parts.get(asked.widgetId) ?? [];
          return create(listParts.output, {
            items: pages[index] ?? [],
            page: { nextCursor: index + 1 < pages.length ? `c${index + 1}` : "", revision: this.revision },
          } as never);
        });
        const list = method<DescMethodUnary>(numbers, "ListNoSideEffects");
        router.rpc(list, async (request, context) => {
          const page = (request as unknown as { page?: { cursor: string; pageSize: number } }).page;
          const cursor = page?.cursor ?? "";
          const began = performance.now();
          const asked = { cursor, listCache: context.requestHeader.get("bones-list-cache"), ifNoneMatch: context.requestHeader.get("if-none-match") };
          context.signal.addEventListener("abort", () => note("list-aborted", { ...asked, ms: Math.round(performance.now() - began) }), { once: true });
          await this.#admit(context.requestHeader, page?.pageSize ?? 0);
          if (cursor === "" && asked.listCache === "1" && asked.ifNoneMatch === `"${this.revision}"`) {
            context.responseHeader.set("bones-not-modified", "1");
            note("list", { ...asked, answer: "not-modified", ms: Math.round(performance.now() - began) });
            return create(list.output);
          }
          const answer = await this.#page(cursor);
          note("list", { ...asked, answer: "page", items: answer.items.length, next: answer.page.nextCursor, revision: answer.page.revision, ms: Math.round(performance.now() - began) });
          return create(list.output, answer as never);
        });
        const watch = method<DescMethodServerStreaming>(events, "Watch");
        router.rpc(watch, (request, context) =>
          this.#ticks((request as unknown as { count: number }).count, context.requestHeader, watch, context.signal),
        );
      },
    });
    this.#handle = (request, response) => {
      if (this.garble && request.url?.startsWith("/contract.v1.Numbers/")) {
        response.writeHead(200, { "content-type": request.headers["content-type"] ?? "application/proto" });
        response.end(Buffer.from([0xff, 0xff, 0xff]));
        return;
      }
      handler(request, response);
    };
    this.#server = createServer(this.#handle);
    await new Promise<void>((resolve) => this.#server!.listen(0, "127.0.0.1", resolve));
  }

  /** Opens a second loopback listener onto the same server; its base URL. */
  async startAlias(): Promise<string> {
    if (this.#server === undefined || this.#handle === undefined) throw new Error("the lane's server is not started");
    if (this.#alias !== undefined) throw new Error("the lane's server already has another address");
    const alias = createServer(this.#handle);
    this.#alias = alias;
    await new Promise<void>((resolve) => alias.listen(0, "127.0.0.1", resolve));
    return `http://127.0.0.1:${(alias.address() as AddressInfo).port}`;
  }

  async *#ticks(count: number, requestHeader: Headers, watch: DescMethodServerStreaming, signal: AbortSignal): AsyncGenerator<MessageShape<DescMessage>> {
    const began = performance.now();
    const elapsed = (): number => Math.round(performance.now() - began);
    signal.addEventListener("abort", () => note("watch-aborted", { ms: elapsed() }), { once: true });
    let sent = 0;
    try {
      await this.#admit(requestHeader);
      for (let next = 0; next < count; next += 1) {
        await delay(this.tickMs);
        if (signal.aborted) return;
        note("tick", { sequence: next, ms: elapsed() });
        sent = next + 1;
        yield create(watch.output, { sequence: next } as never);
      }
    } finally {
      note("watch-end", { count, sent, aborted: signal.aborted, tickMs: this.tickMs, ms: elapsed() });
    }
  }
}

// --- the control endpoint and the caller-change channel ---

/** How long a response waits for the steps process to ack a change of the caller. */
const ACK_TIMEOUT_MS = 30_000;

const held = new Map<number, () => void>();
let nextEvent = 0;

/** Publishes a change of the caller to the steps process and resolves once it acked it. */
function changeCaller(change: { subject?: string; authorization?: string }): Promise<void> {
  const id = nextEvent;
  nextEvent += 1;
  return new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => {
      held.delete(id);
      reject(new Error(`the steps process did not ack the caller change ${id}`));
    }, ACK_TIMEOUT_MS);
    held.set(id, () => {
      clearTimeout(timer);
      resolve();
    });
    process.stdout.write(`${JSON.stringify({ event: "caller", id, ...change })}\n`);
  });
}

function releaseHeld(): void {
  for (const release of held.values()) release();
  held.clear();
}

async function body(request: IncomingMessage): Promise<string> {
  const chunks: Buffer[] = [];
  for await (const chunk of request) chunks.push(chunk as Buffer);
  return Buffer.concat(chunks).toString("utf8");
}

function controlServer(server: SdkCallServer): Server {
  return createServer((request, response) => {
    void (async () => {
      const route = `${request.method} ${request.url}`;
      const reply = (status: number, value: unknown): void => {
        response.writeHead(status, { "content-type": "application/json" });
        response.end(JSON.stringify(value));
      };
      try {
        const ack = /^POST \/ack\/(\d+)$/.exec(route);
        if (ack !== null) {
          const id = Number(ack[1]);
          held.get(id)?.();
          held.delete(id);
          reply(200, {});
        } else if (route === "POST /reset") {
          await server.reset();
          reply(200, {});
        } else if (route === "POST /configure") {
          server.configure(JSON.parse(await body(request)) as Patch);
          reply(200, {});
        } else if (route === "GET /seen") {
          reply(200, server.seen);
        } else if (route === "POST /seen/clear") {
          server.seen.length = 0;
          reply(200, {});
        } else if (route === "POST /alias") {
          reply(200, { url: await server.startAlias() });
        } else {
          reply(404, { error: `no control route ${route}` });
        }
      } catch (error) {
        reply(500, { error: String(error) });
      }
    })();
  });
}

const server = new SdkCallServer();
await server.start();
const control = controlServer(server);
await new Promise<void>((resolve) => control.listen(0, "127.0.0.1", resolve));
process.stdout.write(
  `${JSON.stringify({ ready: true, url: server.url, control: `http://127.0.0.1:${(control.address() as AddressInfo).port}` })}\n`,
);
process.stdin.resume();
process.stdin.on("close", () => process.exit(0));
