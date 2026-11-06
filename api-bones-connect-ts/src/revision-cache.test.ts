// SPDX-License-Identifier: MIT
import { describe, expect, it } from "vitest";
import { RevisionCache } from "./revision-cache.js";
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
