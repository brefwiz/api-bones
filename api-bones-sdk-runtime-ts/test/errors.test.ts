// SPDX-License-Identifier: MIT
import { describe, expect, it } from "vitest";
import { BonesError, PageError, isCursorRejected, parseBonesError } from "../src/index.js";

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

describe("parseBonesError", () => {
  const wire = { code: "GRANT_MISSING", emitter: "example", platform: "FORBIDDEN", connectCode: "permission_denied", message: "denied" };

  it("revives each tagged argument to its plain value", () => {
    const error = parseBonesError(
      JSON.stringify({
        ...wire,
        args: {
          s: { kind: "string", value: "x" },
          n: { kind: "int64", value: "9007199254740993" },
          d: { kind: "double", value: 1.5 },
          nan: { kind: "double", value: "NaN" },
          inf: { kind: "double", value: "Infinity" },
          ninf: { kind: "double", value: "-Infinity" },
          t: { kind: "timestamp", value: "2026-01-02T03:04:05.123456789Z" },
        },
      }),
    );
    expect(error).toBeInstanceOf(BonesError);
    expect(error).not.toBeInstanceOf(PageError);
    expect(error.message).toBe("denied");
    expect([error.code, error.emitter, error.platform, error.connectCode]).toEqual(["GRANT_MISSING", "example", "FORBIDDEN", "permission_denied"]);
    expect(error.args["s"]).toBe("x");
    expect(error.args["n"]).toBe(9007199254740993n);
    expect(error.args["d"]).toBe(1.5);
    expect(error.args["nan"]).toBeNaN();
    expect(error.args["inf"]).toBe(Infinity);
    expect(error.args["ninf"]).toBe(-Infinity);
    expect((error.args["t"] as Date).toISOString()).toBe("2026-01-02T03:04:05.123Z");
  });

  it("drops an argument of an unknown kind and keeps the error", () => {
    const error = parseBonesError(JSON.stringify({ ...wire, args: { m: { kind: "money", value: "1" }, s: { kind: "string", value: "x" } } }));
    expect(error.args).toEqual({ s: "x" });
    expect(error.code).toBe("GRANT_MISSING");
  });

  it("keeps an argument named __proto__ as an own property and the prototype intact", () => {
    const json = `{"code":"X","emitter":"example","platform":"INTERNAL_SERVER_ERROR","connectCode":"internal","message":"m","args":{"__proto__":{"kind":"timestamp","value":"2026-01-02T03:04:05Z"}}}`;
    const error = parseBonesError(json);
    expect(Object.getPrototypeOf(error.args)).toBe(Object.prototype);
    expect(Object.prototype.hasOwnProperty.call(error.args, "__proto__")).toBe(true);
    expect((Object.getOwnPropertyDescriptor(error.args, "__proto__")?.value as Date).toISOString()).toBe("2026-01-02T03:04:05.000Z");
  });

  it("drops an unreadable int64 and an unreadable timestamp and keeps the error", () => {
    const error = parseBonesError(
      JSON.stringify({
        ...wire,
        args: { n: { kind: "int64", value: "1.5" }, t: { kind: "timestamp", value: "not-a-date" }, s: { kind: "string", value: "x" } },
      }),
    );
    expect(error.args).toEqual({ s: "x" });
    expect(error.code).toBe("GRANT_MISSING");
  });

  it("revives a list failure as a page error", () => {
    const error = parseBonesError(
      JSON.stringify({ ...wire, args: {}, page: { kind: "cursor_expired", deliveredPages: 2, deliveredItems: 100 } }),
    );
    expect(error).toBeInstanceOf(PageError);
    expect(isCursorRejected(error)).toBe(true);
  });
});
