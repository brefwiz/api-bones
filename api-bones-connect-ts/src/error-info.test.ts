// SPDX-License-Identifier: MIT
import { Code, ConnectError } from "@connectrpc/connect";
import { describe, expect, it } from "vitest";

import {
  BonesError,
  ERROR_INFO_TYPE,
  decodeErrorInfo,
  encodeErrorInfo,
  errorInfoOf,
  errorInfosOf,
  ErrorArgError,
  MAX_ERROR_ARGS,
  MAX_ERROR_INFOS,
  decodeValidationFailure,
  encodeValidationFailure,
  errorToken,
  messageWithToken,
  VALIDATION_FAILURE_TYPE,
  violationsOf,
  type ErrorArg,
  type ErrorInfo,
} from "./error-info.js";

const FULL: ErrorInfo = { code: "GRANT_MISSING", emitter: "payments" };

function failureWith(info: ErrorInfo, type = ERROR_INFO_TYPE): ConnectError {
  const err = new ConnectError("refused", Code.PermissionDenied);
  err.details = [{ type, value: encodeErrorInfo(info) }];
  return err;
}

describe("ErrorInfo wire codec", () => {
  it("round-trips", () => {
    expect(decodeErrorInfo(encodeErrorInfo(FULL))).toEqual({ ...FULL, args: new Map() });
  });

  it("omits empty fields", () => {
    expect(encodeErrorInfo({ code: "", emitter: "" })).toEqual(new Uint8Array());
    expect(encodeErrorInfo({ code: "X", emitter: "" })).toEqual(
      Uint8Array.from([0x0a, 0x01, 0x58]),
    );
  });

  it("uses multi-byte lengths for long values", () => {
    const info = { code: "A".repeat(300), emitter: "" };
    expect(decodeErrorInfo(encodeErrorInfo(info))).toEqual({ ...info, args: new Map() });
  });

  it("matches the bytes the Rust codec writes", () => {
    const wire = Buffer.from("Cg1HUkFOVF9NSVNTSU5HEghwYXltZW50cw", "base64");
    expect(Uint8Array.from(wire)).toEqual(encodeErrorInfo(FULL));
  });

  it("skips unknown fields", () => {
    const bytes = Uint8Array.from([
      0x48, 0x05, 0x0a, 0x01, 0x41, 0x55, 1, 2, 3, 4, 0x59, 1, 2, 3, 4, 5, 6, 7, 8,
    ]);
    expect(decodeErrorInfo(bytes)).toEqual({ code: "A", emitter: "", args: new Map() });
  });

  it("rejects malformed input", () => {
    expect(decodeErrorInfo(Uint8Array.from([0x0a, 0x05, 0x41]))).toBeUndefined();
    expect(decodeErrorInfo(Uint8Array.from([0x0a]))).toBeUndefined();
    expect(decodeErrorInfo(Uint8Array.from([0x0b]))).toBeUndefined();
    expect(decodeErrorInfo(Uint8Array.from([0x0a, 0x01, 0xff]))).toBeUndefined();
    expect(decodeErrorInfo(new Uint8Array(11).fill(0x80))).toBeUndefined();
    expect(decodeErrorInfo(Uint8Array.from([0x0d, 1, 2]))).toBeUndefined();
  });
});

describe("errorInfoOf", () => {
  it("reads the detail", () => {
    expect(errorInfoOf(failureWith(FULL))).toEqual({ ...FULL, args: new Map() });
  });

  it("accepts a prefixed type url", () => {
    expect(errorInfoOf(failureWith(FULL, `type.googleapis.com/${ERROR_INFO_TYPE}`))).toEqual({ ...FULL, args: new Map() });
  });

  it("treats a malformed detail as absent", () => {
    expect(errorInfoOf(failureWith({ ...FULL, code: "lower" }))).toBeUndefined();
    expect(errorInfoOf(failureWith({ ...FULL, emitter: "Not Valid" }))).toBeUndefined();
    expect(new BonesError(failureWith({ ...FULL, code: "lower" })).code).toBeUndefined();
  });

  it("examines a bounded number of details", () => {
    const bad = encodeErrorInfo({ code: "lower", emitter: "" });
    const err = new ConnectError("refused", Code.Internal);
    err.details = [
      ...Array.from({ length: MAX_ERROR_INFOS }, () => ({ type: ERROR_INFO_TYPE, value: bad })),
      { type: ERROR_INFO_TYPE, value: encodeErrorInfo(FULL) },
    ];
    expect(errorInfoOf(err)).toBeUndefined();
    err.details = Array.from({ length: MAX_ERROR_INFOS + 1 }, () => ({
      type: ERROR_INFO_TYPE,
      value: encodeErrorInfo(FULL),
    }));
    expect(errorInfosOf(err)).toHaveLength(MAX_ERROR_INFOS);
  });

  it("ignores the retired version field", () => {
    const bytes = Uint8Array.from([...encodeErrorInfo({ code: "A", emitter: "" }), 0x1a, 1, 0x31]);
    expect(decodeErrorInfo(bytes)).toEqual({ code: "A", emitter: "", args: new Map() });
  });

  it("ignores other details", () => {
    expect(errorInfoOf(failureWith(FULL, "bones.v1.ValidationFailure"))).toBeUndefined();
    expect(errorInfoOf(new ConnectError("x", Code.Internal))).toBeUndefined();
  });
});

describe("errorToken", () => {
  it("renders the canonical token", () => {
    expect(errorToken(FULL)).toBe("[bones-error code=GRANT_MISSING emitter=payments]");
  });

  it.each([
    { ...FULL, code: "" },
    { ...FULL, emitter: "" },
    { ...FULL, code: "lower" },
    { ...FULL, code: "1BAD" },
    { ...FULL, code: "A".repeat(129) },
    { ...FULL, emitter: "Upper" },
    { ...FULL, emitter: "a b" },
    { ...FULL, emitter: "a".repeat(129) },
    { ...FULL, code: "OK] [bones-error code=FORGED" },
  ])("renders nothing for a value outside the grammar: %o", (info) => {
    expect(errorToken(info)).toBeUndefined();
  });

  it("accepts the limits of the grammar", () => {
    expect(errorToken({ ...FULL, code: "A".repeat(128) })).toBeDefined();
  });
});

describe("messageWithToken", () => {
  it("appends the token when there is one", () => {
    expect(messageWithToken("refused", FULL)).toBe(
      "refused [bones-error code=GRANT_MISSING emitter=payments]",
    );
  });

  it("leaves the message alone otherwise", () => {
    expect(messageWithToken("refused", undefined)).toBe("refused");
    expect(messageWithToken("refused", { ...FULL, emitter: "" })).toBe("refused");
  });
});

describe("BonesError", () => {
  it("exposes the code and emitter and ends with the token", () => {
    const err = new BonesError(failureWith(FULL));
    expect(err.code).toBe("GRANT_MISSING");
    expect(err.emitter).toBe("payments");
    expect(err.message.endsWith("[bones-error code=GRANT_MISSING emitter=payments]")).toBe(true);
    expect(err.cause).toBeInstanceOf(ConnectError);
    expect(err).toBeInstanceOf(Error);
  });

  it("carries nothing for a failure without the detail", () => {
    const plain = new ConnectError("refused", Code.NotFound);
    const err = new BonesError(plain);
    expect(err.code).toBeUndefined();
    expect(err.emitter).toBeUndefined();
    expect(err.message).toBe(plain.message);
  });

  it("keeps an unstamped code readable without a token", () => {
    const err = new BonesError(failureWith({ ...FULL, emitter: "" }));
    expect(err.code).toBe("GRANT_MISSING");
    expect(err.emitter).toBeUndefined();
    expect(err.message).not.toContain("[bones-error");
  });
});

const hex = (bytes: Uint8Array): string => Buffer.from(bytes).toString("hex");
const args = (entries: Array<[string, ErrorArg]>): ReadonlyMap<string, ErrorArg> => new Map(entries);

describe("typed arguments", () => {
  it("writes the same golden bytes as the Rust codec", () => {
    const info: ErrorInfo = {
      code: "A",
      emitter: "",
      args: args([["n", { kind: "int64", value: 5n }]]),
    };
    expect(hex(encodeErrorInfo(info))).toBe("0a0141" + "2207" + "0a016e" + "1202" + "1005");
    expect(Buffer.from("CgFBIgcKAW4SAhAF", "base64").toString("hex")).toBe(hex(encodeErrorInfo(info)));
  });

  it("round-trips every argument type", () => {
    const info: ErrorInfo = {
      code: "GRANT_MISSING",
      emitter: "payments",
      args: args([
        ["amount", { kind: "double", value: 12.5 }],
        ["count", { kind: "int64", value: -7n }],
        ["name", { kind: "string", value: "Zoë" }],
        ["when", { kind: "timestamp", seconds: 1_700_000_000n, nanos: 123_456_789 }],
      ]),
    };
    expect(decodeErrorInfo(encodeErrorInfo(info))).toEqual(info);
  });

  it("keeps an int64 above 2^53 exact", () => {
    const value = 9_007_199_254_740_993n;
    const decoded = decodeErrorInfo(encodeErrorInfo({ ...FULL, args: args([["n", { kind: "int64", value }]]) }));
    expect(decoded?.args?.get("n")).toEqual({ kind: "int64", value });
    const extremes = [(1n << 63n) - 1n, -(1n << 63n)];
    for (const extreme of extremes) {
      const back = decodeErrorInfo(encodeErrorInfo({ ...FULL, args: args([["n", { kind: "int64", value: extreme }]]) }));
      expect(back?.args?.get("n")).toEqual({ kind: "int64", value: extreme });
    }
  });

  it("writes arguments in name order", () => {
    const a = encodeErrorInfo({ ...FULL, args: args([["b", { kind: "int64", value: 1n }], ["a", { kind: "int64", value: 2n }]]) });
    const b = encodeErrorInfo({ ...FULL, args: args([["a", { kind: "int64", value: 2n }], ["b", { kind: "int64", value: 1n }]]) });
    expect(hex(a)).toBe(hex(b));
  });

  it("never alters the token", () => {
    const withArgs = { ...FULL, args: args([["n", { kind: "int64", value: 5n }]]) };
    expect(errorToken(withArgs)).toBe(errorToken(FULL));
    expect(messageWithToken("refused", withArgs)).toBe(messageWithToken("refused", FULL));
  });

  it("exposes the arguments on BonesError", () => {
    const info = { ...FULL, args: args([["n", { kind: "int64", value: 5n }]]) };
    expect(new BonesError(failureWith(info)).args).toEqual(info.args);
    expect(new BonesError(new ConnectError("x", Code.NotFound)).args.size).toBe(0);
  });

  it("carries arguments on a validation failure", () => {
    const failure = {
      field: "/name",
      message: "too long",
      rule: "max_length",
      args: args([["max", { kind: "int64" as const, value: 10n }]]),
    };
    expect(decodeValidationFailure(encodeValidationFailure(failure))).toEqual(failure);
  });

  it("refuses arguments out of bounds on construction", () => {
    const refuse = (entries: Array<[string, ErrorArg]>, reason: string): void => {
      expect(() => encodeErrorInfo({ ...FULL, args: args(entries) })).toThrow(ErrorArgError);
      try {
        encodeErrorInfo({ ...FULL, args: args(entries) });
      } catch (e) {
        expect((e as ErrorArgError).reason).toBe(reason);
      }
    };
    const many = Array.from({ length: MAX_ERROR_ARGS + 1 }, (_, i): [string, ErrorArg] => [`k${i}`, { kind: "int64", value: 1n }]);
    refuse(many, "too-many");
    refuse([["Bad", { kind: "int64", value: 1n }]], "invalid-key");
    refuse([["", { kind: "int64", value: 1n }]], "invalid-key");
    refuse([["a".repeat(65), { kind: "int64", value: 1n }]], "invalid-key");
    refuse([["s", { kind: "string", value: "x".repeat(1025) }]], "string-too-long");
    refuse([["s", { kind: "string", value: "é".repeat(513) }]], "string-too-long");
    refuse([["d", { kind: "double", value: Number.NaN }]], "non-finite-double");
    refuse([["d", { kind: "double", value: Number.POSITIVE_INFINITY }]], "non-finite-double");
    refuse([["t", { kind: "timestamp", seconds: 253_402_300_800n, nanos: 0 }]], "invalid-timestamp");
    refuse([["t", { kind: "timestamp", seconds: 0n, nanos: 1_000_000_000 }]], "invalid-timestamp");
  });

  it("accepts arguments at the bounds", () => {
    const edge = Array.from({ length: MAX_ERROR_ARGS }, (_, i): [string, ErrorArg] => [`k${i}`, { kind: "string", value: "x".repeat(1024) }]);
    const info = { ...FULL, args: args(edge) };
    expect(decodeErrorInfo(encodeErrorInfo(info))?.args?.size).toBe(MAX_ERROR_ARGS);
    expect(() => encodeErrorInfo({ ...FULL, args: args([["a".repeat(64), { kind: "int64", value: 1n }]]) })).not.toThrow();
  });

  it("decodes out-of-bounds wire arguments to nothing", () => {
    // key "Bad" (uppercase) with an int64 value of 1
    const bad = Uint8Array.from([0x22, 0x07, 0x0a, 0x03, 0x42, 0x61, 0x64, 0x12, 0x02, 0x10, 0x01]);
    expect(decodeErrorInfo(bad)).toBeUndefined();
    // a double that is NaN
    const nan = Uint8Array.from([0x22, 0x0e, 0x0a, 0x01, 0x64, 0x12, 0x09, 0x19, 0, 0, 0, 0, 0, 0, 0xf8, 0x7f]);
    expect(decodeErrorInfo(nan)).toBeUndefined();
  });

  it("drops an entry with no value and skips malformed entries as a whole", () => {
    const valueless = Uint8Array.from([0x22, 0x03, 0x0a, 0x01, 0x6e]);
    expect(decodeErrorInfo(valueless)?.args?.size).toBe(0);
    expect(decodeErrorInfo(Uint8Array.from([0x22, 0x05, 0x0a]))).toBeUndefined();
  });

  describe("violationsOf", () => {
    const violation = (rule: string, limit?: bigint) => ({
      field: "/guests",
      message: "too many",
      rule,
      args: args(limit === undefined ? [] : [["limit", { kind: "int64" as const, value: limit }]]),
    });
    const refusal = (...details: Array<{ type: string; value: Uint8Array }>): ConnectError => {
      const err = new ConnectError("refused", Code.InvalidArgument);
      err.details = details;
      return err;
    };
    const detail = (failure: ReturnType<typeof violation>) => ({
      type: VALIDATION_FAILURE_TYPE,
      value: encodeValidationFailure(failure),
    });

    it("is empty without violation details", () => {
      expect(violationsOf(new ConnectError("x", Code.InvalidArgument))).toEqual([]);
      expect(violationsOf(failureWith(FULL))).toEqual([]);
    });

    it("reads every violation in detail order, with its arguments", () => {
      const first = violation("max", 4n);
      const second = violation("required");
      const err = refusal(detail(first), { type: ERROR_INFO_TYPE, value: encodeErrorInfo(FULL) }, detail(second));
      expect(violationsOf(err)).toEqual([first, second]);
    });

    it("never truncates", () => {
      const err = refusal(...Array.from({ length: MAX_ERROR_INFOS * 4 }, () => detail(violation("max", 1n))));
      expect(violationsOf(err)).toHaveLength(MAX_ERROR_INFOS * 4);
    });

    it("treats an undecodable detail as absent", () => {
      // key "Bad" (uppercase) breaks the argument bounds
      const bad = Uint8Array.from([0x22, 0x07, 0x0a, 0x03, 0x42, 0x61, 0x64, 0x12, 0x02, 0x10, 0x01]);
      const ok = violation("max", 4n);
      expect(violationsOf(refusal({ type: VALIDATION_FAILURE_TYPE, value: bad }, detail(ok)))).toEqual([ok]);
    });

    it("accepts the type URL form", () => {
      const ok = violation("max", 4n);
      const err = refusal({ type: `type.googleapis.com/${VALIDATION_FAILURE_TYPE}`, value: encodeValidationFailure(ok) });
      expect(violationsOf(err)).toEqual([ok]);
    });
  });
});
