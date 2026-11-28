// SPDX-License-Identifier: MIT
import { Code, ConnectError } from "@connectrpc/connect";
import { describe, expect, it } from "vitest";
import { create } from "@bufbuild/protobuf";
import { BATCH_MAX_ITEMS, BATCH_MAX_REQUEST_BYTES, authorizeBatchResults, itemErrorToConnect, planItems, serveBatchRequest } from "./batch-primitives.js";
import { errorInfoOf } from "./error-info.js";
import { ErrorInfoSchema } from "./generated/bones/v1/errors_pb.js";

describe("provider batch boundaries", () => {
  it("rejects 201 ids before any provider work and accepts 200", async () => {
    let calls = 0;
    const serve = async () => { calls++; return authorizeBatchResults([{ ok: true, value: "widget" }], () => { calls++; }); };
    const ids = Array.from({ length: 201 }, (_, index) => String(index));
    const error = await serveBatchRequest(ids, serve).catch((error: unknown) => error);
    expect(error).toBeInstanceOf(ConnectError);
    expect((error as ConnectError).code).toBe(Code.InvalidArgument);
    expect(errorInfoOf(error as ConnectError)?.code).toBe("BATCH_TOO_LARGE");
    expect(calls).toBe(0);
    await expect(serveBatchRequest(ids.slice(0, 200), serve)).resolves.toEqual([{ ok: true, value: "widget" }]);
    expect(calls).toBe(2);
  });

  it.each([Code.NotFound, Code.PermissionDenied, Code.InvalidArgument, Code.FailedPrecondition])("keeps expected refusal %s per item", async code => {
    const error = new ConnectError("refused", code);
    await expect(authorizeBatchResults([{ ok: true, value: "allowed" }, { ok: true, value: "denied" }], value => { if (value === "denied") throw error; })).resolves.toEqual([{ ok: true, value: "allowed" }, { ok: false, error }]);
  });

  it.each([new ConnectError("private authorization fault", Code.Internal), new Error("private authorization fault"), new ConnectError("service unavailable", Code.Unavailable)])("fails whole authorization for %s", async error => {
    await expect(authorizeBatchResults([{ ok: true, value: "widget" }], () => { throw error; })).rejects.toBe(error);
  });

  it("fails whole call for an existing internal item error", async () => {
    const error = new ConnectError("private lookup fault", Code.Internal);
    await expect(authorizeBatchResults([{ ok: false, error }], () => {})).rejects.toBe(error);
  });

  it("carries the typed arguments of a decoded ErrorInfo message into the item error", () => {
    const info = create(ErrorInfoSchema, {
      code: "SEAT_TAKEN",
      emitter: "bookings",
      args: {
        seat: { value: { case: "string", value: "12A" } },
        row: { value: { case: "int64", value: 4n } },
        price: { value: { case: "double", value: 12.5 } },
        at: { value: { case: "timestamp", value: { seconds: 86_400n, nanos: 5 } } },
        unset: {},
      },
    });
    const error = itemErrorToConnect({ code: "failed_precondition", message: "taken", info });
    expect(error.code).toBe(Code.FailedPrecondition);
    expect(errorInfoOf(error)).toEqual({
      code: "SEAT_TAKEN",
      emitter: "bookings",
      args: new Map([
        ["at", { kind: "timestamp", seconds: 86_400n, nanos: 5 }],
        ["price", { kind: "double", value: 12.5 }],
        ["row", { kind: "int64", value: 4n }],
        ["seat", { kind: "string", value: "12A" }],
      ]),
    });
  });
});

describe("message item planning", () => {
  it("allows 768 KiB per request", () => {
    expect(BATCH_MAX_REQUEST_BYTES).toBe(786_432);
  });

  it("plans no chunk for no items", () => {
    expect(planItems(0, [])).toEqual({ chunks: [], rejected: new Map() });
  });

  it("splits 450 items into chunks of 200, 200 and 50 by position", () => {
    const plan = planItems(0, Array.from({ length: 450 }, () => 10));
    expect(plan.chunks.map(chunk => chunk.length)).toEqual([BATCH_MAX_ITEMS, BATCH_MAX_ITEMS, 50]);
    expect(plan.chunks.flat()).toEqual(Array.from({ length: 450 }, (_, index) => index));
    expect(plan.rejected.size).toBe(0);
  });

  it("closes a chunk at the byte limit before the count limit", () => {
    const half = BATCH_MAX_REQUEST_BYTES / 2;
    expect(planItems(0, [half, half, 1, 1]).chunks).toEqual([[0, 1], [2, 3]]);
  });

  it("counts the template bytes of every chunk", () => {
    expect(planItems(100, [BATCH_MAX_REQUEST_BYTES - 100, 1]).chunks).toEqual([[0], [1]]);
  });

  it("keeps a repeated item at every position", () => {
    expect(planItems(0, [5, 5, 5]).chunks).toEqual([[0, 1, 2]]);
  });

  it("rejects an item that alone exceeds the limit at its own position and keeps the others", () => {
    const plan = planItems(0, [5, BATCH_MAX_REQUEST_BYTES + 1, 5]);
    expect(plan.chunks).toEqual([[0, 2]]);
    expect([...plan.rejected.keys()]).toEqual([1]);
    const error = plan.rejected.get(1)!;
    expect(error.code).toBe(Code.InvalidArgument);
    expect(errorInfoOf(error)?.code).toBe("BATCH_ITEM_TOO_LARGE");
  });

  it("accepts an item that exactly fills the limit", () => {
    expect(planItems(0, [BATCH_MAX_REQUEST_BYTES]).chunks).toEqual([[0]]);
  });
});
