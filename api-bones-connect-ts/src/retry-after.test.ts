// SPDX-License-Identifier: MIT
import { Code, ConnectError } from "@connectrpc/connect";
import { describe, expect, it } from "vitest";

import { encodeErrorInfo, ERROR_INFO_TYPE } from "./error-info.js";
import {
  RETRY_INFO_TYPE,
  decodeRetryInfo,
  encodeRetryInfo,
  resourceExhaustedRetryAfter,
  retryAfter,
  retryAfterSeconds,
  withRetryAfter,
} from "./retry-after.js";

function refusalWithHeader(value: string): ConnectError {
  const err = new ConnectError("slow down", Code.ResourceExhausted);
  err.metadata.set("retry-after", value);
  return err;
}

function refusalWithDetails(...details: Array<{ type: string; value: Uint8Array }>): ConnectError {
  const err = new ConnectError("slow down", Code.ResourceExhausted);
  err.details = details;
  return err;
}

describe("retryAfterSeconds", () => {
  it("is ceil with a floor of one", () => {
    expect(retryAfterSeconds(0)).toBe(1);
    expect(retryAfterSeconds(1)).toBe(1);
    expect(retryAfterSeconds(999)).toBe(1);
    expect(retryAfterSeconds(1000)).toBe(1);
    expect(retryAfterSeconds(1001)).toBe(2);
    expect(retryAfterSeconds(1200)).toBe(2);
    expect(retryAfterSeconds(7000)).toBe(7);
  });

  it("reads a wait that is not a positive number as one second", () => {
    expect(retryAfterSeconds(-5)).toBe(1);
    expect(retryAfterSeconds(Number.NaN)).toBe(1);
  });
});

describe("RetryInfo wire codec", () => {
  it("matches the bytes the platform limiter and the Rust codec write", () => {
    expect(encodeRetryInfo(7000)).toEqual(Uint8Array.from([0x0a, 0x02, 0x08, 0x07]));
    expect(encodeRetryInfo(300_000)).toEqual(Uint8Array.from([0x0a, 0x03, 0x08, 0xac, 0x02]));
    expect(encodeRetryInfo(1200)).toEqual(Uint8Array.from([0x0a, 0x02, 0x08, 0x02]));
    expect(encodeRetryInfo(0)).toEqual(Uint8Array.from([0x0a, 0x02, 0x08, 0x01]));
    expect(Buffer.from(encodeRetryInfo(7000)).toString("base64")).toBe("CgIIBw==");
  });

  it("round-trips whole seconds", () => {
    for (const seconds of [1, 2, 7, 127, 128, 300, 86_400, 2 ** 32 - 1]) {
      expect(decodeRetryInfo(encodeRetryInfo(seconds * 1000))).toBe(seconds * 1000);
    }
  });

  it("reads seconds and nanos", () => {
    // retry_delay { seconds: 2, nanos: 500_000_000 }
    const bytes = Uint8Array.from([0x0a, 0x08, 0x08, 0x02, 0x10, 0x80, 0xca, 0xb5, 0xee, 0x01]);
    expect(decodeRetryInfo(bytes)).toBe(2500);
  });

  it("reads a duration with the zero seconds omitted", () => {
    expect(decodeRetryInfo(Uint8Array.from([0x0a, 0x00]))).toBe(0);
  });

  it("skips unknown fields", () => {
    expect(decodeRetryInfo(Uint8Array.from([0x28, 0x01, 0x0a, 0x02, 0x08, 0x03, 0x28, 0x01]))).toBe(3000);
  });

  it("refuses what names no usable wait", () => {
    expect(decodeRetryInfo(new Uint8Array())).toBeUndefined();
    expect(decodeRetryInfo(Uint8Array.from([0x28, 0x01]))).toBeUndefined();
    expect(decodeRetryInfo(Uint8Array.from([0x0a, 0x05, 0x08]))).toBeUndefined();
    expect(decodeRetryInfo(Uint8Array.from([0x0a]))).toBeUndefined();
    expect(decodeRetryInfo(Uint8Array.from([0x0a, 0x01, 0x08]))).toBeUndefined();
    // seconds: -1, sign-extended to ten bytes.
    const negative = [0x0a, 0x0b, 0x08, ...Array(9).fill(0xff), 0x01];
    expect(decodeRetryInfo(Uint8Array.from(negative))).toBeUndefined();
    // nanos: 1_000_000_000, out of range.
    expect(decodeRetryInfo(Uint8Array.from([0x0a, 0x08, 0x08, 0x01, 0x10, 0x80, 0x94, 0xeb, 0xdc, 0x03]))).toBeUndefined();
  });
});

describe("withRetryAfter", () => {
  it("sets the header, no-store and the detail", () => {
    const err = withRetryAfter(new ConnectError("busy", Code.Unavailable), 1200);
    expect(err.metadata.get("retry-after")).toBe("2");
    expect(err.metadata.get("cache-control")).toBe("no-store");
    expect(err.details).toHaveLength(1);
    expect(err.details[0]).toEqual({ type: RETRY_INFO_TYPE, value: encodeRetryInfo(1200) });
    expect(retryAfter(err)).toBe(2000);
  });

  it("keeps the code, the message, other headers and other details", () => {
    const base = new ConnectError("busy", Code.Unavailable);
    base.metadata.set("x-trace", "abc");
    base.details = [{ type: ERROR_INFO_TYPE, value: encodeErrorInfo({ code: "X", emitter: "" }) }];
    const err = withRetryAfter(base, 3000);
    expect(err.code).toBe(Code.Unavailable);
    expect(err.rawMessage).toBe("busy");
    expect(err.metadata.get("x-trace")).toBe("abc");
    expect(err.details.map((detail) => (detail as { type: string }).type)).toEqual([
      ERROR_INFO_TYPE,
      RETRY_INFO_TYPE,
    ]);
    expect(base.metadata.get("retry-after")).toBeNull();
  });

  it("replaces an earlier wait", () => {
    const err = withRetryAfter(withRetryAfter(new ConnectError("x", Code.Unavailable), 9000), 4000);
    expect(err.metadata.get("retry-after")).toBe("4");
    expect(err.details).toHaveLength(1);
    expect(retryAfter(err)).toBe(4000);
  });

  it("chooses only the code in resourceExhaustedRetryAfter", () => {
    const err = resourceExhaustedRetryAfter("slow down", 5000);
    expect(err.code).toBe(Code.ResourceExhausted);
    expect(err.rawMessage).toBe("slow down");
    expect(retryAfter(err)).toBe(5000);
    expect(err.metadata.get("retry-after")).toBe("5");
  });
});

describe("retryAfter", () => {
  it("prefers the detail to the header", () => {
    const err = refusalWithHeader("9");
    err.details = [{ type: RETRY_INFO_TYPE, value: encodeRetryInfo(3000) }];
    expect(retryAfter(err)).toBe(3000);
  });

  it("falls back to the header", () => {
    expect(retryAfter(refusalWithHeader("9"))).toBe(9000);
    expect(retryAfter(refusalWithHeader(" 9 "))).toBe(9000);
    expect(retryAfter(refusalWithHeader("0"))).toBe(0);
  });

  it("is undefined for a value that is not whole seconds", () => {
    for (const value of [
      "Wed, 21 Oct 2015 07:28:00 GMT",
      "-1",
      "-0",
      "+5",
      "1.5",
      "1e3",
      "",
      "  ",
      "soon",
      "18446744073709551616",
    ]) {
      expect(retryAfter(refusalWithHeader(value)), JSON.stringify(value)).toBeUndefined();
    }
  });

  it("is undefined without a wait", () => {
    expect(retryAfter(new ConnectError("x", Code.ResourceExhausted))).toBeUndefined();
  });

  it("falls through an undecodable detail to the header", () => {
    const err = refusalWithDetails({ type: RETRY_INFO_TYPE, value: Uint8Array.from([0x0a, 0x05, 0x08]) });
    err.metadata.set("retry-after", "8");
    expect(retryAfter(err)).toBe(8000);
  });

  it("skips a bad detail for a good one", () => {
    const err = refusalWithDetails(
      { type: RETRY_INFO_TYPE, value: Uint8Array.from([0x0a, 0x05, 0x08]) },
      { type: RETRY_INFO_TYPE, value: encodeRetryInfo(2000) },
    );
    expect(retryAfter(err)).toBe(2000);
  });

  it("accepts a prefixed type url", () => {
    const err = refusalWithDetails({
      type: `type.googleapis.com/${RETRY_INFO_TYPE}`,
      value: encodeRetryInfo(7000),
    });
    expect(retryAfter(err)).toBe(7000);
  });

  it("ignores other details", () => {
    expect(retryAfter(refusalWithDetails({ type: ERROR_INFO_TYPE, value: encodeRetryInfo(7000) }))).toBeUndefined();
  });

  it("examines a bounded number of details", () => {
    const bad = { type: RETRY_INFO_TYPE, value: Uint8Array.from([0x0a, 0x05, 0x08]) };
    const err = refusalWithDetails(bad, bad, bad, bad, { type: RETRY_INFO_TYPE, value: encodeRetryInfo(2000) });
    expect(retryAfter(err)).toBeUndefined();
  });
});
