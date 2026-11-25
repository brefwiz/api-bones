// SPDX-License-Identifier: MIT
import { describe, expect, it } from "vitest";
import { BasicTracerProvider } from "@opentelemetry/sdk-trace-base";
import { JsSpanSink, PageError, parseBonesError, type BonesError, iterableOf, iterateHandle, listCallOf, type ListCall, type Page } from "../src/index.js";
import { HandleDouble, Rejection, pageJson, wireError } from "./handle-double.js";

const decodePage = (json: string): Page<number> => JSON.parse(json) as Page<number>;
const decodeNumber = (json: string): number => JSON.parse(json) as number;

function listOver(pages: Array<string | Rejection>): { call: ListCall<number>; handles: HandleDouble[] } {
  const handles: HandleDouble[] = [];
  const call = listCallOf<number>({
    openPages: () => {
      const handle = new HandleDouble(pages);
      handles.push(handle);
      return handle;
    },
    decodePage,
    sink: new JsSpanSink(),
  });
  return { call, handles };
}

describe("handle adapters cancel the handle exactly once", () => {
  it("on break out of pages()", async () => {
    const { call, handles } = listOver([pageJson([1, 2], true), pageJson([3], false)]);
    for await (const page of call.pages()) {
      expect(page.items).toEqual([1, 2]);
      break;
    }
    expect(handles).toHaveLength(1);
    expect(handles[0]?.cancels).toBe(1);
  });

  it("on break out of items()", async () => {
    const { call, handles } = listOver([pageJson([1, 2], true), pageJson([3], false)]);
    const seen: number[] = [];
    for await (const item of call.items()) {
      seen.push(item);
      if (item === 2) break;
    }
    expect(seen).toEqual([1, 2]);
    expect(handles[0]?.cancels).toBe(1);
  });

  it("on return() before the first next()", async () => {
    const { call, handles } = listOver([pageJson([1], false)]);
    const walk = call.pages()[Symbol.asyncIterator]();
    await walk.return?.();
    await walk.return?.();
    expect(handles[0]?.cancels).toBe(1);
  });

  it("on return() of items() before the first next()", async () => {
    const { call, handles } = listOver([pageJson([1], false)]);
    const walk = call.items()[Symbol.asyncIterator]();
    await walk.return?.();
    expect(handles[0]?.cancels).toBe(1);
  });

  it("when the sequence ends and the consumer returns afterwards", async () => {
    const { call, handles } = listOver([pageJson([1], false)]);
    const walk = call.pages()[Symbol.asyncIterator]();
    expect((await walk.next()).done).toBe(false);
    expect((await walk.next()).done).toBe(true);
    await walk.return?.();
    expect(handles[0]?.cancels).toBe(1);
  });

  it("when the handle rejects, handing the mapped error to the consumer", async () => {
    const handle = new HandleDouble([new Rejection(wireError("example", "GRANT_MISSING", "FORBIDDEN", "permission_denied", "denied"))]);
    const walk = iterateHandle(handle, decodeNumber, (rejection) => parseBonesError(rejection as string));
    const failure: unknown = await walk.next().catch((error: unknown) => error);
    expect(failure).toMatchObject({ code: "GRANT_MISSING", connectCode: "permission_denied" });
    await walk.return?.();
    expect(handle.cancels).toBe(1);
  });

  it("when two next() calls overlap, the second is refused as busy", async () => {
    const handle = new HandleDouble([]);
    const walk = iterateHandle(handle, decodeNumber, (rejection) => parseBonesError(rejection as string));
    const [first, second] = await Promise.allSettled([walk.next(), walk.next()]);
    expect(first).toEqual({ status: "fulfilled", value: { done: true, value: undefined } });
    expect(second.status).toBe("rejected");
    const refusal = second.status === "rejected" ? (second.reason as BonesError) : undefined;
    expect([refusal?.emitter, refusal?.code, refusal?.platform, refusal?.connectCode]).toEqual([
      "bones",
      "PRECONDITION_FAILED",
      "PRECONDITION_FAILED",
      "failed_precondition",
    ]);
    expect(refusal?.message).toBe("handle busy: next() called while a previous next() is pending");
    expect(handle.cancels).toBe(1);
  });

  it("when a document does not decode", async () => {
    const handle = new HandleDouble(["not json"]);
    const walk = iterateHandle(handle, decodeNumber);
    await expect(walk.next()).rejects.toBeInstanceOf(SyntaxError);
    expect(handle.cancels).toBe(1);
  });

  it("on firstPage()", async () => {
    const { call, handles } = listOver([pageJson([1, 2], true), pageJson([3], false)]);
    const page = await call.firstPage();
    expect(page.items).toEqual([1, 2]);
    expect(handles[0]?.cancels).toBe(1);
  });

  it("on break out of a stream", async () => {
    const handles: HandleDouble[] = [];
    const stream = iterableOf(() => {
      const handle = new HandleDouble(["1", "2", "3"]);
      handles.push(handle);
      return handle;
    }, decodeNumber);
    for await (const value of stream) {
      if (value === 2) break;
    }
    expect(handles[0]?.cancels).toBe(1);
  });

  it("when a stream ends, handing over its values in order", async () => {
    const handle = new HandleDouble(["1", "2", "3"]);
    const seen: number[] = [];
    for await (const value of iterableOf(() => handle, decodeNumber)) seen.push(value);
    expect(seen).toEqual([1, 2, 3]);
    expect(handle.cancels).toBe(1);
  });

  it("when a stream's handle rejects, throwing the typed error after the values delivered", async () => {
    const wire = JSON.stringify({
      code: "CURSOR_REJECTED",
      emitter: "example",
      platform: "PRECONDITION_FAILED",
      connectCode: "failed_precondition",
      message: "the list position was refused",
      args: {},
      page: { kind: "cursor_expired", deliveredPages: 2, deliveredItems: 4 },
    });
    const handle = new HandleDouble(["1", "2", new Rejection(wire)]);
    const seen: number[] = [];
    const walk = async (): Promise<void> => {
      for await (const value of iterableOf(() => handle, decodeNumber, (rejection) => parseBonesError(rejection as string))) seen.push(value);
    };
    const failure: unknown = await walk().catch((error: unknown) => error);
    expect(seen).toEqual([1, 2]);
    expect(failure).toBeInstanceOf(PageError);
    expect(failure).toMatchObject({ kind: "cursor_expired", deliveredPages: 2, deliveredItems: 4 });
    expect(handle.cancels).toBe(1);
  });
});

describe("a list opens a fresh handle for every walk", () => {
  it("walks pages, then items, then the first page from separate handles", async () => {
    const { call, handles } = listOver([pageJson([1, 2], true), pageJson([3], false)]);
    const pages: Array<readonly number[]> = [];
    for await (const page of call.pages()) pages.push(page.items);
    const items: number[] = [];
    for await (const item of call.items()) items.push(item);
    await call.firstPage();
    expect(pages).toEqual([[1, 2], [3]]);
    expect(items).toEqual([1, 2, 3]);
    expect(handles).toHaveLength(3);
    expect(handles.map((handle) => handle.cancels)).toEqual([1, 1, 1]);
  });

  it("reads past an empty page and resolves the walk's iteration span through the sink", async () => {
    const sink = new JsSpanSink(new BasicTracerProvider().getTracer("list-call-test"));
    const traceparent = sink.start(JSON.stringify({ name: "iteration", startTime: Date.now(), kind: "internal" }));
    const call = listCallOf<number>({
      openPages: () => new HandleDouble([pageJson([], true), pageJson([4], false)], traceparent),
      decodePage,
      sink,
    });
    expect(call.span).toBeUndefined();
    const items: number[] = [];
    for await (const item of call.items()) items.push(item);
    expect(items).toEqual([4]);
    expect(traceparent).toBeDefined();
    expect(call.span).toBe(sink.lookup(traceparent ?? ""));
    expect(call.span).toBeDefined();
  });

  it("leaves the list call without an iteration span when its handle reports none", async () => {
    const { call } = listOver([pageJson([1], false)]);
    await call.firstPage();
    expect(call.span).toBeUndefined();
  });

  it("refuses a first page that never arrives", async () => {
    const { call, handles } = listOver([]);
    await expect(call.firstPage()).rejects.toBeInstanceOf(TypeError);
    expect(handles[0]?.cancels).toBe(1);
  });
});
