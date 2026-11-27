// SPDX-License-Identifier: MIT
import { describe, expect, it } from "vitest";
import { BasicTracerProvider } from "@opentelemetry/sdk-trace-base";
import { Int32ValueSchema, timestampNow } from "@bufbuild/protobuf/wkt";
import { PageFailure_Kind, SpanStart_Kind } from "../src/generated/bones/sdk/v1/boundary_pb.js";
import { JsSpanSink, PageError, decodePage as decodeCodecPage, iterableOf, iterateHandle, listCallOf, mapCallError, type BonesError, type Handle, type ListCall, type Page } from "../src/index.js";
import { HandleDouble, Rejection } from "./handle-double.js";
import { callErrorBytes, numberBytes, numberOf, pageBytes, refusal, spanStartBytes, startedOf } from "./wire.js";

const decodePage = (bytes: Uint8Array): Page<number> => {
  const page = decodeCodecPage(bytes, Int32ValueSchema);
  return { ...page, items: page.items.map((item) => item.value) };
};

function listOver(pages: Array<Uint8Array | Rejection>): { call: ListCall<number>; handles: HandleDouble[] } {
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

describe("the pages a list reads ahead", () => {
  function opened(chosen?: 0 | 1): { call: ListCall<number>; asked: Array<0 | 1 | undefined> } {
    const asked: Array<0 | 1 | undefined> = [];
    const call = listCallOf<number>({
      openPages: (prefetch) => {
        asked.push(prefetch);
        return new HandleDouble([pageBytes([1], false)]);
      },
      decodePage,
      sink: new JsSpanSink(),
    });
    return { call: chosen === undefined ? call : call.withPrefetch(chosen), asked };
  }

  it("leaves the choice to the core when the caller made none", async () => {
    const { call, asked } = opened();
    for await (const _page of call.pages()) break;
    expect(asked).toEqual([undefined]);
  });

  it.each([0, 1] as const)("withPrefetch(%s) names it for pages() and items()", async (chosen) => {
    const { call, asked } = opened(chosen);
    for await (const _page of call.pages()) break;
    for await (const _item of call.items()) break;
    expect(asked).toEqual([chosen, chosen]);
  });

  it("firstPage() asks the core for its first-page read and names no prefetch", async () => {
    const seen: Array<[0 | 1 | undefined, boolean | undefined]> = [];
    const call = listCallOf<number>({
      openPages: (prefetch, firstOnly) => {
        seen.push([prefetch, firstOnly]);
        return new HandleDouble([pageBytes([1], true)]);
      },
      decodePage,
      sink: new JsSpanSink(),
    }).withPrefetch(1);
    await call.firstPage();
    expect(seen).toEqual([[undefined, true]]);
  });

  it("withPrefetch returns another list and leaves the first one as it was", async () => {
    const { call, asked } = opened();
    const quiet = call.withPrefetch(0);
    expect(quiet).not.toBe(call);
    for await (const _page of call.pages()) break;
    for await (const _page of quiet.pages()) break;
    expect(asked).toEqual([undefined, 0]);
  });
});

describe("handle adapters cancel the handle exactly once", () => {
  it("on break out of pages()", async () => {
    const { call, handles } = listOver([pageBytes([1, 2], true), pageBytes([3], false)]);
    for await (const page of call.pages()) {
      expect(page.items).toEqual([1, 2]);
      break;
    }
    expect(handles).toHaveLength(1);
    expect(handles[0]?.cancels).toBe(1);
  });

  it("on break out of items()", async () => {
    const { call, handles } = listOver([pageBytes([1, 2], true), pageBytes([3], false)]);
    const seen: number[] = [];
    for await (const item of call.items()) {
      seen.push(item);
      if (item === 2) break;
    }
    expect(seen).toEqual([1, 2]);
    expect(handles[0]?.cancels).toBe(1);
  });

  it("on return() before the first next()", async () => {
    const { call, handles } = listOver([pageBytes([1], false)]);
    const walk = call.pages()[Symbol.asyncIterator]();
    await walk.return?.();
    await walk.return?.();
    expect(handles[0]?.cancels).toBe(1);
  });

  it("on return() of items() before the first next()", async () => {
    const { call, handles } = listOver([pageBytes([1], false)]);
    const walk = call.items()[Symbol.asyncIterator]();
    await walk.return?.();
    expect(handles[0]?.cancels).toBe(1);
  });

  it("drops the buffered items of items() once it is returned", async () => {
    const { call, handles } = listOver([pageBytes([1, 2], true), pageBytes([3, 4], true), pageBytes([5], false)]);
    const walk = call.items()[Symbol.asyncIterator]();
    expect((await walk.next()).value).toBe(1);
    await walk.return?.();
    expect(await walk.next()).toEqual({ done: true, value: undefined });
    expect(handles[0]?.cancels).toBe(1);
  });

  it("when the sequence ends and the consumer returns afterwards", async () => {
    const { call, handles } = listOver([pageBytes([1], false)]);
    const walk = call.pages()[Symbol.asyncIterator]();
    expect((await walk.next()).done).toBe(false);
    expect((await walk.next()).done).toBe(true);
    await walk.return?.();
    expect(handles[0]?.cancels).toBe(1);
  });

  it("when the handle rejects, handing the mapped error to the consumer", async () => {
    const handle = new HandleDouble([new Rejection(refusal("example", "GRANT_MISSING", "FORBIDDEN", "permission_denied", "denied"))]);
    const walk = iterateHandle(handle, numberOf, mapCallError);
    const failure: unknown = await walk.next().catch((error: unknown) => error);
    expect(failure).toMatchObject({ code: "GRANT_MISSING", connectCode: "permission_denied" });
    await walk.return?.();
    expect(handle.cancels).toBe(1);
  });

  it("when two next() calls overlap, the second is refused as busy", async () => {
    const handle = new HandleDouble([]);
    const walk = iterateHandle(handle, numberOf, mapCallError);
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

  it("keeps the handle open when next() is refused as HANDLE_BUSY", async () => {
    const handle = new HandleDouble([new Rejection(refusal("bones", "HANDLE_BUSY", "PRECONDITION_FAILED", "failed_precondition", "busy"))]);
    const walk = iterateHandle(handle, numberOf, mapCallError);
    await expect(walk.next()).rejects.toMatchObject({ code: "HANDLE_BUSY" });
    expect(handle.cancels).toBe(0);
    await walk.return?.();
    expect(handle.cancels).toBe(1);
  });

  it("when a message does not decode", async () => {
    const handle = new HandleDouble([new Uint8Array([0xff])]);
    const walk = iterateHandle(handle, numberOf);
    await expect(walk.next()).rejects.toBeInstanceOf(Error);
    expect(handle.cancels).toBe(1);
  });

  it("on firstPage()", async () => {
    const { call, handles } = listOver([pageBytes([1, 2], true), pageBytes([3], false)]);
    const page = await call.firstPage();
    expect(page.items).toEqual([1, 2]);
    expect(handles[0]?.cancels).toBe(1);
  });

  it("on break out of a stream", async () => {
    const handles: HandleDouble[] = [];
    const stream = iterableOf(() => {
      const handle = new HandleDouble([1, 2, 3].map(numberBytes));
      handles.push(handle);
      return handle;
    }, numberOf);
    for await (const value of stream) {
      if (value === 2) break;
    }
    expect(handles[0]?.cancels).toBe(1);
  });

  it("when a stream ends, handing over its values in order", async () => {
    const handle = new HandleDouble([1, 2, 3].map(numberBytes));
    const seen: number[] = [];
    for await (const value of iterableOf(() => handle, numberOf)) seen.push(value);
    expect(seen).toEqual([1, 2, 3]);
    expect(handle.cancels).toBe(1);
  });

  it("when a stream's handle rejects, throwing the typed error after the values delivered", async () => {
    const wire = callErrorBytes({
      code: "CURSOR_REJECTED",
      emitter: "example",
      platform: "PRECONDITION_FAILED",
      connectCode: "failed_precondition",
      message: "the list position was refused",
      page: { kind: PageFailure_Kind.CURSOR_EXPIRED, deliveredPages: 2, deliveredItems: 4n },
    });
    const handle = new HandleDouble([numberBytes(1), numberBytes(2), new Rejection(wire)]);
    const seen: number[] = [];
    const walk = async (): Promise<void> => {
      for await (const value of iterableOf(() => handle, numberOf, mapCallError)) seen.push(value);
    };
    const failure: unknown = await walk().catch((error: unknown) => error);
    expect(seen).toEqual([1, 2]);
    expect(failure).toBeInstanceOf(PageError);
    expect(failure).toMatchObject({ kind: "cursor_expired", deliveredPages: 2, deliveredItems: 4 });
    expect(handle.cancels).toBe(1);
  });
});

describe("a handle that delivers more than bytes", () => {
  it("hands the whole delivery to the decoder", async () => {
    const hold = {};
    const queue: Array<readonly [Uint8Array, object]> = [[numberBytes(7), hold]];
    const handle: Handle<readonly [Uint8Array, object]> = {
      next: async () => queue.shift(),
      cancel: () => undefined,
      span: () => undefined,
    };
    const seen: object[] = [];
    for await (const value of iterateHandle(handle, ([bytes, held]) => {
      seen.push(held);
      return numberOf(bytes);
    })) {
      expect(value).toBe(7);
    }
    expect(seen).toEqual([hold]);
    expect(seen[0]).toBe(hold);
  });
});

describe("a list opens a fresh handle for every walk", () => {
  it("walks pages, then items, then the first page from separate handles", async () => {
    const { call, handles } = listOver([pageBytes([1, 2], true), pageBytes([3], false)]);
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
    const { traceparent } = startedOf(sink.start(spanStartBytes({ name: "iteration", startTime: timestampNow(), kind: SpanStart_Kind.INTERNAL })));
    const call = listCallOf<number>({
      openPages: () => new HandleDouble([pageBytes([], true), pageBytes([4], false)], traceparent),
      decodePage,
      sink,
    });
    expect(call.span).toBeUndefined();
    const items: number[] = [];
    for await (const item of call.items()) items.push(item);
    expect(items).toEqual([4]);
    expect(traceparent).not.toBe("");
    expect(call.span).toBe(sink.lookup(traceparent));
    expect(call.span).toBeDefined();
  });

  it("leaves the list call without an iteration span when its handle reports none", async () => {
    const { call } = listOver([pageBytes([1], false)]);
    await call.firstPage();
    expect(call.span).toBeUndefined();
  });

  it("refuses a first page that never arrives", async () => {
    const { call, handles } = listOver([]);
    await expect(call.firstPage()).rejects.toBeInstanceOf(TypeError);
    expect(handles[0]?.cancels).toBe(1);
  });
});
