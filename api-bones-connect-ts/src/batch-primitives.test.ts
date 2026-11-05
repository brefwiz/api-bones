// SPDX-License-Identifier: MIT
import { Code, ConnectError } from "@connectrpc/connect";
import { describe, expect, it } from "vitest";
import { authorizeBatchResults, serveBatchRequest } from "./batch-primitives.js";
import { errorInfoOf } from "./error-info.js";

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
});
