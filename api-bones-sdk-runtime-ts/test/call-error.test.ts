// SPDX-License-Identifier: MIT
import { describe, expect, it } from "vitest";
import { timestampFromMs } from "@bufbuild/protobuf/wkt";
import { BonesError, PageError, isCursorRejected, mapCallError } from "../src/index.js";
import { PageFailure_Kind } from "../src/generated/bones/sdk/v1/boundary_pb.js";
import { callErrorBytes } from "./wire.js";

const wire = { code: "GRANT_MISSING", emitter: "example", platform: "FORBIDDEN", connectCode: "permission_denied", message: "denied" };

describe("mapCallError", () => {
  it("revives each typed argument to its plain value", () => {
    const error = mapCallError(
      callErrorBytes({
        ...wire,
        args: {
          s: { value: { case: "string", value: "x" } },
          n: { value: { case: "int64", value: 9007199254740993n } },
          d: { value: { case: "double", value: 1.5 } },
          nan: { value: { case: "double", value: NaN } },
          inf: { value: { case: "double", value: Infinity } },
          ninf: { value: { case: "double", value: -Infinity } },
          t: { value: { case: "timestamp", value: timestampFromMs(Date.UTC(2026, 0, 2, 3, 4, 5, 123)) } },
        },
      }),
    ) as BonesError;
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

  it("reads the wait a refusal named in whole milliseconds, and null when it named none", () => {
    const named = mapCallError(callErrorBytes({ ...wire, retryAfter: { seconds: 7n, nanos: 500_900_000 } })) as BonesError;
    expect(named.retryAfterMs).toBe(7500);
    expect((mapCallError(callErrorBytes(wire)) as BonesError).retryAfterMs).toBeNull();
  });

  it("drops an argument that names no member of its oneof and keeps the error", () => {
    const error = mapCallError(callErrorBytes({ ...wire, args: { m: {}, s: { value: { case: "string", value: "x" } } } })) as BonesError;
    expect(error.args).toEqual({ s: "x" });
    expect(error.code).toBe("GRANT_MISSING");
  });

  it("keeps an argument named __proto__ as an own property and the prototype intact", () => {
    const error = mapCallError(
      callErrorBytes({ ...wire, args: { ["__proto__"]: { value: { case: "timestamp", value: timestampFromMs(Date.UTC(2026, 0, 2, 3, 4, 5)) } } } }),
    ) as BonesError;
    expect(Object.getPrototypeOf(error.args)).toBe(Object.prototype);
    expect(Object.prototype.hasOwnProperty.call(error.args, "__proto__")).toBe(true);
    expect((Object.getOwnPropertyDescriptor(error.args, "__proto__")?.value as Date).toISOString()).toBe("2026-01-02T03:04:05.000Z");
  });

  it.each([
    [PageFailure_Kind.CURSOR_INVALID, "cursor_invalid"],
    [PageFailure_Kind.CURSOR_EXPIRED, "cursor_expired"],
    [PageFailure_Kind.CURSOR_QUERY_MISMATCH, "cursor_query_mismatch"],
    [PageFailure_Kind.RPC, "rpc"],
    [PageFailure_Kind.UNSPECIFIED, "rpc"],
  ] as const)("revives a list failure of kind %s as a page error of kind %s", (kind, expected) => {
    const error = mapCallError(callErrorBytes({ ...wire, page: { kind, deliveredPages: 2, deliveredItems: 100n } }));
    expect(error).toBeInstanceOf(PageError);
    expect(error).toMatchObject({ kind: expected, deliveredPages: 2, deliveredItems: 100 });
    expect(isCursorRejected(error)).toBe(expected !== "rpc");
  });

  it("hands anything but bytes back unchanged", () => {
    const failure = new Error("the host failed");
    expect(mapCallError(failure)).toBe(failure);
    expect(mapCallError("text")).toBe("text");
    expect(mapCallError(undefined)).toBeUndefined();
  });

  it("does not mistake an abort reason for a call error", () => {
    const abort = new AbortController();
    abort.abort();
    expect(mapCallError(abort.signal.reason)).toBe(abort.signal.reason);
  });
});
