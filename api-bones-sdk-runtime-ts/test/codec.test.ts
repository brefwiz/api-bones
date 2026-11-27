// SPDX-License-Identifier: MIT
import { describe, expect, it } from "vitest";
import { Int32ValueSchema } from "@bufbuild/protobuf/wkt";
import { BonesError, PageError, decodeBatchChunk, decodePage } from "../src/index.js";
import { PageFailure_Kind, PageSchema } from "../src/generated/bones/sdk/v1/boundary_pb.js";
import { Total_Kind } from "../src/generated/bones/v1/pagination_pb.js";
import { batchChunkBytes, bytesOf, numberBytes, pageBytes } from "./wire.js";

describe("decodePage", () => {
  it("decodes every item by the item schema and carries the page fields", () => {
    const page = decodePage(pageBytes([4, 5], true), Int32ValueSchema);
    expect(page.items.map((item) => item.value)).toEqual([4, 5]);
    expect([page.revision, page.fromCache, page.hasNext, page.total]).toEqual(["r1", false, true, undefined]);
  });

  it("reads the cache flag and an exact or estimated total", () => {
    const exact = decodePage(bytesOf(PageSchema, { items: [], revision: "r2", fromCache: true, total: { value: 12n, kind: Total_Kind.EXACT } }), Int32ValueSchema);
    expect(exact).toMatchObject({ items: [], revision: "r2", fromCache: true, hasNext: false, total: { value: 12n, kind: "exact" } });
    const estimate = decodePage(bytesOf(PageSchema, { total: { value: 3n, kind: Total_Kind.ESTIMATE } }), Int32ValueSchema);
    expect(estimate.total).toEqual({ value: 3n, kind: "estimate" });
    const unspecified = decodePage(bytesOf(PageSchema, { total: { value: 1n } }), Int32ValueSchema);
    expect(unspecified.total).toEqual({ value: 1n, kind: "estimate" });
  });

  it("rejects bytes that are not a page", () => {
    expect(() => decodePage(new Uint8Array([0xff]), Int32ValueSchema)).toThrow();
  });
});

describe("decodeBatchChunk", () => {
  it("answers one outcome per id in request order: an item, a typed failure or a malformed outcome", () => {
    const chunk = decodeBatchChunk(
      batchChunkBytes({
        results: [
          { outcome: { case: "item", value: numberBytes(7) } },
          {
            outcome: {
              case: "error",
              value: { code: "MISSING", page: { kind: PageFailure_Kind.CURSOR_EXPIRED, deliveredPages: 1, deliveredItems: 2n } },
            },
          },
          { outcome: { case: "error", value: { code: "NOT_FOUND", connectCode: "not_found" } } },
          {},
        ],
      }),
      Int32ValueSchema,
    );
    expect(chunk).toHaveLength(4);
    expect("ok" in chunk[0]! ? chunk[0].ok.value : undefined).toBe(7);
    expect("error" in chunk[1]! ? chunk[1].error : undefined).toBeInstanceOf(PageError);
    const second = chunk[2]!;
    expect("error" in second ? [second.error.code, second.error.connectCode] : undefined).toEqual(["NOT_FOUND", "not_found"]);
    const unset = chunk[3]!;
    expect("error" in unset ? unset.error : undefined).toBeInstanceOf(BonesError);
    expect("error" in unset ? [unset.error.code, unset.error.connectCode] : undefined).toEqual(["PRECONDITION_FAILED", "failed_precondition"]);
  });

  it("answers an empty chunk as no outcomes", () => {
    expect(decodeBatchChunk(batchChunkBytes({}), Int32ValueSchema)).toEqual([]);
  });
});
