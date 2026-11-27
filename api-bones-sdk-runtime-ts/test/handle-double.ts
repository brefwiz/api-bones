// SPDX-License-Identifier: MIT
import type { Handle } from "../src/index.js";
import { refusal } from "./wire.js";

/** A queued rejection: the handle rejects with these `CallError` bytes. */
export class Rejection {
  constructor(readonly wire: Uint8Array) {}
}

/** A Rust handle at the TypeScript unit level: yields the given messages, then ends. */
export class HandleDouble implements Handle {
  cancels = 0;
  #queue: Array<Uint8Array | Rejection>;
  #cancelled = false;
  #pending = false;

  constructor(
    messages: ReadonlyArray<Uint8Array | Rejection>,
    private readonly traceparent?: string,
  ) {
    this.#queue = [...messages];
  }

  async next(): Promise<Uint8Array | undefined> {
    if (this.#pending) {
      throw refusal("bones", "PRECONDITION_FAILED", "PRECONDITION_FAILED", "failed_precondition", "handle busy: next() called while a previous next() is pending");
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
