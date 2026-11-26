// SPDX-License-Identifier: MIT
/**
 * The loopback server of the sdk-call lane, as the steps see it: a separate
 * process (`tests/typescript/provider/sdk_call_server.ts`) scripted and read
 * through its control endpoint. It never loads the wrapper under test.
 *
 * A scripted change of the caller reaches the steps as a line on the process's
 * stdout; the steps apply it to their own credential state and ack it, and only
 * then does the server answer the request that triggered it.
 */

import { spawn } from "node:child_process";
import type { ChildProcess } from "node:child_process";
import { join } from "node:path";
import { createInterface } from "node:readline";

const SERVER = join(import.meta.dirname, "../../../typescript/provider/sdk_call_server.ts");

/** What one request carried. */
export interface Seen {
  traceparent: string;
  /** The `authorization` header; empty when the request carried none. */
  authorization: string;
  timeoutMs: number;
  /** The `bones-list-cache` header; empty when the request carried none. */
  listCache: string;
  /** The `if-none-match` header; empty when the request carried none. */
  ifNoneMatch: string;
  /** The page size a list request asked for; zero for any other request or none asked. */
  pageSize: number;
}

/** A change of the caller the server scripts while it handles a request. */
export interface CallerChange {
  subject?: string;
  authorization?: string;
}

/** What a scenario scripts on the server; a `null` clears a scripted change. */
export interface Patch {
  /** The scenario that runs; the server names it on every line it narrates. */
  scenario?: string;
  widgets?: Record<string, string>;
  parts?: Record<string, string[][]>;
  pages?: number[][];
  pageDelayMs?: number;
  ticks?: number;
  tickMs?: number;
  unavailable?: number;
  unauthenticated?: number;
  switchTo?: string | null;
  switchOnPage?: { index: number; subject: string } | null;
  refreshTo?: string | null;
  expireAtPage?: number | null;
  garble?: boolean;
}

export class Provider {
  /** The base URL of the services the wrapper calls. */
  url = "";
  /** Applies a change of the caller the server published; set by the scenario that holds the caller. */
  onCallerChange: ((change: CallerChange) => void) | undefined;
  #control = "";
  #child: ChildProcess | undefined;
  #acks = new Set<Promise<unknown>>();

  /** Starts the server process and returns once it listens. */
  async start(): Promise<void> {
    const child = spawn("node", [SERVER], { stdio: ["pipe", "pipe", "inherit"] });
    this.#child = child;
    await new Promise<void>((resolve, reject) => {
      child.once("error", reject);
      child.once("exit", (code) => reject(new Error(`the sdk-call server exited with ${code} before it was ready`)));
      createInterface({ input: child.stdout! }).on("line", (line) => {
        const message = JSON.parse(line) as { ready?: boolean; url?: string; control?: string; event?: string; id?: number } & CallerChange;
        if (message.ready === true) {
          this.url = message.url!;
          this.#control = message.control!;
          resolve();
        } else if (message.event === "caller") {
          const { subject, authorization } = message;
          this.onCallerChange?.({ subject, authorization });
          const acked = this.#post(`/ack/${message.id}`);
          this.#acks.add(acked);
          void acked.finally(() => this.#acks.delete(acked));
        }
      });
    });
  }

  /** Ends the server process. */
  async stop(): Promise<void> {
    const child = this.#child;
    if (child === undefined) return;
    this.#child = undefined;
    const ended = new Promise<void>((resolve) => child.once("exit", () => resolve()));
    child.stdin!.end();
    child.kill();
    await ended;
  }

  async #post(path: string, body?: unknown): Promise<unknown> {
    const response = await fetch(`${this.#control}${path}`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    if (!response.ok) throw new Error(`the sdk-call server refused ${path}: ${await response.text()}`);
    return response.json();
  }

  async configure(patch: Patch): Promise<void> {
    await this.#post("/configure", patch);
  }

  /** Forgets what the last scenario scripted and recorded, and closes what it left open. */
  async reset(): Promise<void> {
    await this.#post("/reset");
    await Promise.allSettled([...this.#acks]);
  }

  async seen(): Promise<Seen[]> {
    const response = await fetch(`${this.#control}/seen`);
    if (!response.ok) throw new Error(`the sdk-call server refused /seen: ${await response.text()}`);
    return (await response.json()) as Seen[];
  }

  async clearSeen(): Promise<void> {
    await this.#post("/seen/clear");
  }

  /** A second loopback address of the same server; its base URL. */
  async alias(): Promise<string> {
    return ((await this.#post("/alias")) as { url: string }).url;
  }
}
