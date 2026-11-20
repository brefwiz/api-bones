// SPDX-License-Identifier: MIT
import { describe, expect, it } from "vitest";

import { mapListCall, type ListCall, type Page } from "./page.js";

const page = (items: number[], hasNext: boolean): Page<number> => ({
  items, revision: "r1", fromCache: false, hasNext, total: { value: 5n, kind: "exact" },
});

function source(pagesOut: Page<number>[], fail?: Error): ListCall<number> & { pulled: number } {
  const state = { pulled: 0 };
  async function* gen() {
    for (const p of pagesOut) { state.pulled++; yield p; }
    if (fail) throw fail;
  }
  return {
    get pulled() { return state.pulled; },
    span: { marker: "span" } as never,
    pages: () => gen(),
    items: async function* () { for await (const p of gen()) yield* p.items; },
    firstPage: async () => pagesOut[0]!,
  };
}

describe("mapListCall", () => {
  it("maps items and keeps page metadata", async () => {
    const mapped = mapListCall(source([page([1, 2], true), page([3], false)]), (n) => `v${n}`);
    const seen: Page<string>[] = [];
    for await (const p of mapped.pages()) seen.push(p);
    expect(seen.map((p) => p.items)).toEqual([["v1", "v2"], ["v3"]]);
    expect(seen[0]).toMatchObject({ revision: "r1", hasNext: true, fromCache: false, total: { value: 5n, kind: "exact" } });
    const flat: string[] = [];
    for await (const v of mapped.items()) flat.push(v);
    expect(flat).toEqual(["v1", "v2", "v3"]);
  });

  it("maps the first page", async () => {
    const first = await mapListCall(source([page([7], false)]), (n) => n * 2).firstPage();
    expect(first.items).toEqual([14]);
    expect(first.revision).toBe("r1");
  });

  it("handles an empty page", async () => {
    const mapped = mapListCall(source([page([], false)]), (n) => n);
    expect((await mapped.firstPage()).items).toEqual([]);
    const out: number[] = [];
    for await (const v of mapped.items()) out.push(v);
    expect(out).toEqual([]);
  });

  it("is lazy and does not buffer", async () => {
    const src = source([page([1], true), page([2], false)]);
    const mapped = mapListCall(src, (n) => n);
    expect(src.pulled).toBe(0);
    for await (const _ of mapped.pages()) break;
    expect(src.pulled).toBe(1);
  });

  it("propagates source errors", async () => {
    const boom = new Error("boom");
    const mapped = mapListCall(source([page([1], true)], boom), (n) => n);
    await expect((async () => { for await (const _ of mapped.pages()) { /* drain */ } })()).rejects.toBe(boom);
  });

  it("propagates mapper errors", async () => {
    const mapped = mapListCall(source([page([1], false)]), () => { throw new Error("bad"); });
    await expect(mapped.firstPage()).rejects.toThrow("bad");
  });

  it("passes the span through", () => {
    const src = source([]);
    expect(mapListCall(src, (n) => n).span).toBe(src.span);
  });
});
