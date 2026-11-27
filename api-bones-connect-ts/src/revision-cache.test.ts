// SPDX-License-Identifier: MIT
import { describe, expect, it } from "vitest";
import { HOLD_CHAIN_BYTES, HOLD_TOTAL_BYTES, PAGE_OVERHEAD, RevisionCache } from "./revision-cache.js";
import type { Page } from "./page.js";

const page = (revision: string, value: number): Page<number> => ({ items: [value], revision, fromCache: false, hasNext: false });

describe("holder-bound revision chains", () => {
  it("replays only a completed chain with one revision", () => {
    const cache = new RevisionCache(() => "alice/acme");
    const key = cache.key("/Numbers/List", "query");
    const first = page("r1", 1), last = page("r1", 2);
    cache.begin(key);
    cache.keep(key, first, false);
    expect(cache.replay(key)).toBeUndefined();
    cache.keep(key, last, true);
    expect(cache.replay(key)).toEqual([first, last]);
  });

  it("does not replay a chain fetched across a write", () => {
    const cache = new RevisionCache(() => "alice/acme");
    const key = cache.key("/Numbers/List", "query");
    const first = page("r1", 1), last = page("r2", 2);
    cache.begin(key);
    cache.keep(key, first, false);
    cache.keep(key, last, true);
    expect(cache.replay(key)).toBeUndefined();
  });

  it("clears every old identity even when identity later returns", () => {
    let identity = "alice/acme";
    const cache = new RevisionCache(() => identity);
    const key = cache.key("/Numbers/List", "query");
    const held = page("r1", 1);
    cache.begin(key);
    cache.keep(key, held, true);
    identity = "bob/globex";
    expect(cache.replay(cache.key("/Numbers/List", "query"))).toBeUndefined();
    identity = "alice/acme";
    expect(cache.key("/Numbers/List", "query")).not.toEqual(key);
    expect(cache.replay(key)).toBeUndefined();
  });

  it("makes pending cache keys obsolete on session end", () => {
    const cache = new RevisionCache(() => "alice/acme");
    const key = cache.key("/Numbers/List", "query");
    const held = page("r1", 1);
    cache.begin(key);
    cache.keep(key, held, true);
    cache.clear();
    expect(cache.key("/Numbers/List", "query")).not.toEqual(key);
    expect(cache.replay(key)).toBeUndefined();
  });

  it("separates methods and canonical query hashes", () => {
    const cache = new RevisionCache(() => "alice/acme");
    const key = cache.key("/Numbers/List", "red:20");
    const held = page("r1", 1);
    cache.begin(key);
    cache.keep(key, held, true);
    expect(cache.replay(cache.key("/Numbers/List", "blue:20"))).toBeUndefined();
    expect(cache.replay(cache.key("/Numbers/List", "red:50"))).toBeUndefined();
    expect(cache.replay(cache.key("/Other/List", "red:20"))).toBeUndefined();
  });
});

/** A string item whose encoded size as a repeated field is exactly `bytes`. */
const itemOf = (bytes: number): string => {
  for (let length = Math.max(bytes - 5, 0); ; length++) {
    const encoded = length + 1 + (length < 0x80 ? 1 : length < 0x4000 ? 2 : length < 0x200000 ? 3 : 4);
    if (encoded === bytes) return "x".repeat(length);
    if (encoded > bytes) throw new Error(`no item encodes to ${bytes} bytes`);
  }
};
const sized = (revision: string, bytes: number): Page<string> => ({ items: [itemOf(bytes)], revision, fromCache: false, hasNext: false });
const empty = (revision: string): Page<string> => ({ items: [], revision, fromCache: false, hasNext: false });
const held = (cache: RevisionCache, key: string) => cache.replay(key) !== undefined;
const put = (cache: RevisionCache, method: string, ...pages: Page<unknown>[]) => {
  const key = cache.key(method, "query");
  cache.begin(key);
  pages.forEach((page, index) => cache.keep(key, page, index === pages.length - 1));
  return key;
};

describe("held chains stay within a byte budget", () => {
  it("holds a chain at the budget and drops one byte over", () => {
    const cache = new RevisionCache(() => "alice/acme");
    expect(held(cache, put(cache, "/S/List", sized("r1", HOLD_CHAIN_BYTES - PAGE_OVERHEAD - 2)))).toBe(true);
    expect(held(cache, put(cache, "/S/Edge", sized("r1", HOLD_CHAIN_BYTES - PAGE_OVERHEAD - 1)))).toBe(false);
    expect(held(cache, put(cache, "/S/Other", sized("r1", HOLD_CHAIN_BYTES / 2 + 1), sized("r1", HOLD_CHAIN_BYTES / 2 + 1)))).toBe(false);
  });

  it("stops holding pages of an overflowing walk and evicts the older revision", () => {
    const cache = new RevisionCache(() => "alice/acme");
    const key = put(cache, "/S/List", sized("r1", 2));
    cache.begin(key);
    cache.keep(key, sized("r2", HOLD_CHAIN_BYTES - PAGE_OVERHEAD - 2), false);
    cache.keep(key, sized("r2", 2), false);
    cache.keep(key, sized("r2", 2), true);
    expect(held(cache, key)).toBe(false);
    expect(cache.isEmpty()).toBe(true);
  });

  it("evicts the least recently used chain past the total", () => {
    const cache = new RevisionCache(() => "alice/acme");
    const quarter = HOLD_TOTAL_BYTES / 4 - PAGE_OVERHEAD - 2;
    const [a, b, c, d] = ["A", "B", "C", "D"].map((method) => put(cache, `/S/${method}`, sized("r1", quarter)));
    // Reading A refreshes it, so the next eviction takes B.
    expect(held(cache, a)).toBe(true);
    const e = put(cache, "/S/E", sized("r1", 2));
    expect(held(cache, b)).toBe(false);
    for (const kept of [a, c, d, e]) expect(held(cache, kept)).toBe(true);
  });

  it("charges every item-less page", () => {
    const cache = new RevisionCache(() => "alice/acme");
    const fits = Math.floor(HOLD_CHAIN_BYTES / (PAGE_OVERHEAD + 2));
    expect(held(cache, put(cache, "/S/Fits", ...Array.from({ length: fits }, () => empty("r1"))))).toBe(true);
    expect(held(cache, put(cache, "/S/Over", ...Array.from({ length: fits + 1 }, () => empty("r1"))))).toBe(false);
  });

  it("charges a revision by its length and evicts item-less chains", () => {
    const cache = new RevisionCache(() => "alice/acme");
    const revision = "r".repeat(1 << 16);
    const stored = (2 * HOLD_TOTAL_BYTES) / (1 << 16);
    const keys = Array.from({ length: stored }, (_, n) => put(cache, `/S/${n}`, empty(revision)));
    const count = keys.filter((key) => held(cache, key)).length;
    expect(count).toBeGreaterThan(0);
    expect(count).toBeLessThan(stored);
  });

  it("does not hold a chain with a page without a revision", () => {
    const cache = new RevisionCache(() => "alice/acme");
    const key = put(cache, "/S/List", sized("r1", 2));
    cache.begin(key);
    cache.keep(key, sized("r1", 2), false);
    cache.keep(key, sized("", 2), true);
    expect(held(cache, key)).toBe(false);
    expect(cache.isEmpty()).toBe(true);
  });
});
