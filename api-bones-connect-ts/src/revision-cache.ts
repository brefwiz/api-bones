// SPDX-License-Identifier: MIT
import { toBinary, type DescMessage, type Message } from "@bufbuild/protobuf";
import type { Page } from "./page.js";

/** The most one (identity, method, request) chain holds: the encoded protobuf length of its items plus {@link PAGE_OVERHEAD} and the revision's length for every page. */
export const HOLD_CHAIN_BYTES = 1 << 20;
/** What one held page costs besides its items, so a chain of item-less pages still counts. */
export const PAGE_OVERHEAD = 128;
/** The most the whole cache holds; the least recently used chain goes first. */
export const HOLD_TOTAL_BYTES = 4 << 20;

interface Chain {
  revision: string;
  pages: Page<unknown>[];
  bytes: number;
  used: number;
  complete: boolean;
  unheld: boolean;
}

const encoder = new TextEncoder();

function varintLength(value: number): number {
  let length = 1;
  for (let rest = value; rest >= 0x80; rest = Math.floor(rest / 0x80)) length++;
  return length;
}

/** Encoded size of one list item: a message by its schema, a string or bytes item as a repeated field entry. */
function itemBytes(item: unknown, itemSchema: DescMessage | undefined): number {
  if (itemSchema) return toBinary(itemSchema, item as Message).byteLength;
  const length = typeof item === "string" ? encoder.encode(item).byteLength : item instanceof Uint8Array ? item.byteLength : 8;
  return typeof item === "string" || item instanceof Uint8Array ? 1 + varintLength(length) + length : length;
}

/** Per-client, holder-bound cache. Identity changes discard every old chain. A chain holds its pages for as long as it is cached, within {@link HOLD_CHAIN_BYTES} per chain and {@link HOLD_TOTAL_BYTES} overall. */
export class RevisionCache {
  private identity?: string;
  private generation = 0;
  private clock = 0;
  private readonly chains = new Map<string, Chain>();

  constructor(private readonly identityKey: () => string) {}

  clear(): void {
    this.chains.clear();
    this.generation++;
  }

  isEmpty(): boolean {
    return this.chains.size === 0;
  }

  key(method: string, requestHash: string): string {
    const identity = this.identityKey();
    if (this.identity !== identity) {
      this.clear();
      this.identity = identity;
    }
    return JSON.stringify([this.generation, identity, method, requestHash]);
  }

  replay<T>(key: string): Page<T>[] | undefined {
    const chain = this.chains.get(key);
    if (!chain?.complete) return undefined;
    // Entries retained without a next-cursor signal cannot be replayed faithfully.
    if (chain.pages.some((page) => page.revision !== chain.revision || typeof page.hasNext !== "boolean")) {
      this.chains.delete(key);
      return undefined;
    }
    chain.used = ++this.clock;
    return chain.pages as Page<T>[];
  }

  begin(key: string): void {
    this.chains.set(key, { revision: "", pages: [], bytes: 0, used: 0, complete: false, unheld: false });
  }

  /** Adds `page`, charged its items' encoded size, {@link PAGE_OVERHEAD} and its revision's length. A chain over its budget is dropped and later pages of the walk are not held. */
  keep<T>(key: string, page: Page<T>, complete: boolean, itemSchema?: DescMessage): void {
    const chain = this.chains.get(key);
    if (!chain || chain.unheld) return;
    if (chain.pages.length === 0) chain.revision = page.revision;
    if (!page.revision || page.revision !== chain.revision) {
      this.chains.delete(key);
      return;
    }
    chain.bytes += page.items.reduce((sum, item) => sum + itemBytes(item, itemSchema), 0) + PAGE_OVERHEAD + page.revision.length;
    if (chain.bytes > HOLD_CHAIN_BYTES) {
      chain.unheld = true;
      this.chains.delete(key);
      return;
    }
    chain.pages.push(page as Page<unknown>);
    chain.complete = complete;
    if (!complete) return;
    chain.used = ++this.clock;
    while (this.heldBytes() > HOLD_TOTAL_BYTES && this.evictOldest()) { /* evict until within budget */ }
  }

  private heldBytes(): number {
    let total = 0;
    for (const chain of this.chains.values()) if (chain.complete) total += chain.bytes;
    return total;
  }

  /** Drops the least recently used complete chain; whether one went. */
  private evictOldest(): boolean {
    let oldest: [string, Chain] | undefined;
    for (const entry of this.chains) if (entry[1].complete && (!oldest || entry[1].used < oldest[1].used)) oldest = entry;
    if (!oldest) return false;
    this.chains.delete(oldest[0]);
    return true;
  }
}
