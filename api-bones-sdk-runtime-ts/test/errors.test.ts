// SPDX-License-Identifier: MIT
import { describe, expect, it } from "vitest";
import { BonesError, PageError, isCursorRejected } from "../src/index.js";

const init = { code: "CURSOR_EXPIRED", emitter: "example", platform: "BAD_REQUEST", connectCode: "invalid_argument", message: "cursor expired" };

describe("errors", () => {
  it("a page error is a bones error carrying what was delivered", () => {
    const error = new PageError(init, { kind: "cursor_expired", deliveredPages: 2, deliveredItems: 40 });
    expect(error).toBeInstanceOf(BonesError);
    expect(error.kind).toBe("cursor_expired");
    expect([error.deliveredPages, error.deliveredItems]).toEqual([2, 40]);
    expect(error.code).toBe("CURSOR_EXPIRED");
    expect(error.platform).toBe("BAD_REQUEST");
    expect(error.args).toEqual({});
  });

  it.each(["cursor_invalid", "cursor_expired", "cursor_query_mismatch"] as const)("%s is a rejected cursor", (kind) => {
    expect(isCursorRejected(new PageError(init, { kind, deliveredPages: 0, deliveredItems: 0 }))).toBe(true);
  });

  it("a failed call and a non-page value are not a rejected cursor", () => {
    expect(isCursorRejected(new PageError(init, { kind: "rpc", deliveredPages: 0, deliveredItems: 0 }))).toBe(false);
    expect(isCursorRejected(new BonesError(init))).toBe(false);
    expect(isCursorRejected("cursor_invalid")).toBe(false);
    expect(isCursorRejected(undefined)).toBe(false);
  });
});

describe("BonesError", () => {
  it("carries the wait a refusal named, and null when it named none", () => {
    expect(new BonesError(init).retryAfterMs).toBeNull();
    expect(new BonesError({ ...init, retryAfterMs: 1500 }).retryAfterMs).toBe(1500);
  });
});
