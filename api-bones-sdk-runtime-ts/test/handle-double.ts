// SPDX-License-Identifier: MIT
import type { Handle } from "../src/index.js";

/** A queued rejection: the handle rejects with this wire JSON string. */
export class Rejection {
  constructor(readonly wire: string) {}
}

/** The wire JSON of a failed call, schema `api-bones.error/v1`. */
export function wireError(emitter: string, code: string, platform: string, connectCode: string, message: string): string {
  return JSON.stringify({ code, emitter, platform, connectCode, message, args: {} });
}

/** A Rust handle at the TypeScript unit level: yields the given documents, then ends. */
export class HandleDouble implements Handle {
  cancels = 0;
  #queue: Array<string | Rejection>;
  #cancelled = false;
  #pending = false;

  constructor(
    documents: ReadonlyArray<string | Rejection>,
    private readonly traceparent?: string,
  ) {
    this.#queue = [...documents];
  }

  async next(): Promise<string | undefined> {
    if (this.#pending) {
      throw wireError("bones", "PRECONDITION_FAILED", "PRECONDITION_FAILED", "failed_precondition", "handle busy: next() called while a previous next() is pending");
    }
    this.#pending = true;
    try {
      await Promise.resolve();
      if (this.#cancelled) return undefined;
      const head = this.#queue.shift();
      if (head instanceof Rejection) {
        this.#queue = [];
        throw head.wire;
      }
      return head;
    } finally {
      this.#pending = false;
    }
  }

  cancel(): void {
    this.cancels += 1;
    this.#cancelled = true;
  }

  span(): string | undefined {
    return this.traceparent;
  }
}

/** A page document in the shape the wrapper's decoder reads. */
export function pageJson(items: readonly number[], hasNext: boolean): string {
  return JSON.stringify({ items, revision: "r1", fromCache: false, hasNext });
}
