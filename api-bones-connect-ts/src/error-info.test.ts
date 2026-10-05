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
  MAX_ERROR_INFOS,
  errorToken,
  messageWithToken,
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
    expect(decodeErrorInfo(encodeErrorInfo(FULL))).toEqual(FULL);
  });

  it("omits empty fields", () => {
    expect(encodeErrorInfo({ code: "", emitter: "" })).toEqual(new Uint8Array());
    expect(encodeErrorInfo({ code: "X", emitter: "" })).toEqual(
      Uint8Array.from([0x0a, 0x01, 0x58]),
    );
  });

  it("uses multi-byte lengths for long values", () => {
    const info = { code: "A".repeat(300), emitter: "" };
    expect(decodeErrorInfo(encodeErrorInfo(info))).toEqual(info);
  });

  it("matches the bytes the Rust codec writes", () => {
    const wire = Buffer.from("Cg1HUkFOVF9NSVNTSU5HEghwYXltZW50cxoFMS40LjI", "base64");
    expect(Uint8Array.from(wire)).toEqual(encodeErrorInfo(FULL));
  });

  it("skips unknown fields", () => {
    const bytes = Uint8Array.from([
      0x48, 0x05, 0x0a, 0x01, 0x41, 0x55, 1, 2, 3, 4, 0x59, 1, 2, 3, 4, 5, 6, 7, 8,
    ]);
    expect(decodeErrorInfo(bytes)).toEqual({ code: "A", emitter: "" });
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
    expect(errorInfoOf(failureWith(FULL))).toEqual(FULL);
  });

  it("accepts a prefixed type url", () => {
    expect(errorInfoOf(failureWith(FULL, `type.googleapis.com/${ERROR_INFO_TYPE}`))).toEqual(FULL);
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
    expect(decodeErrorInfo(bytes)).toEqual({ code: "A", emitter: "" });
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
