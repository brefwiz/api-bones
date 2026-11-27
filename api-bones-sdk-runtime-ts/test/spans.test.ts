// SPDX-License-Identifier: MIT
import { ROOT_CONTEXT, SpanKind, SpanStatusCode, context, createTraceState, propagation, trace } from "@opentelemetry/api";
import { AsyncLocalStorageContextManager } from "@opentelemetry/context-async-hooks";
import { W3CTraceContextPropagator } from "@opentelemetry/core";
import { AlwaysOffSampler, BasicTracerProvider, InMemorySpanExporter, SimpleSpanProcessor } from "@opentelemetry/sdk-trace-base";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { timestampFromMs } from "@bufbuild/protobuf/wkt";
import { JsSpanSink, callCtx } from "../src/index.js";
import { SpanEnd_Status, SpanStart_Kind } from "../src/generated/bones/sdk/v1/boundary_pb.js";
import { spanEndBytes, spanStartBytes, startedOf } from "./wire.js";

const traceId = "4bf92f3577b34da6a3ce929d0e0e4736";
const spanId = "00f067aa0ba902b7";
const exporter = new InMemorySpanExporter();
const provider = new BasicTracerProvider({ spanProcessors: [new SimpleSpanProcessor(exporter)] });
const manager = new AsyncLocalStorageContextManager().enable();

beforeAll(() => {
  context.setGlobalContextManager(manager);
  propagation.setGlobalPropagator(new W3CTraceContextPropagator());
});

afterAll(async () => {
  context.disable();
  propagation.disable();
  await provider.shutdown();
});

const hr = (time: [number, number]): number => time[0] * 1000 + time[1] / 1e6;

describe("callCtx", () => {
  it("reads the trace fields of the active context through the propagator", () => {
    const active = trace.setSpanContext(ROOT_CONTEXT, { traceId, spanId, traceFlags: 1, isRemote: true });
    const ctx = context.with(active, () => callCtx());
    expect(ctx).toEqual({ traceparent: `00-${traceId}-${spanId}-01` });
  });

  it("writes the trace state of the active context beside the traceparent", () => {
    const active = trace.setSpanContext(ROOT_CONTEXT, {
      traceId,
      spanId,
      traceFlags: 1,
      isRemote: true,
      traceState: createTraceState("vendor=value"),
    });
    const ctx = context.with(active, () => callCtx());
    expect(ctx).toEqual({ traceparent: `00-${traceId}-${spanId}-01`, tracestate: "vendor=value" });
  });

  it("carries the caller's abort signal and omits what is absent", () => {
    const abort = new AbortController();
    expect(callCtx(abort.signal)).toEqual({ signal: abort.signal });
  });
});

describe("JsSpanSink", () => {
  it("opens a live span under its parent, answers its traceparent and ends it at the core's time", () => {
    exporter.reset();
    const sink = new JsSpanSink(provider.getTracer("test"));
    const end = Date.now();
    const start = end - 250;
    const started = startedOf(
      sink.start(
        spanStartBytes({
          name: "svc.Method page",
          startTime: timestampFromMs(start),
          kind: SpanStart_Kind.CLIENT,
          parent: `00-${traceId}-${spanId}-01`,
          attributes: { "page.index": { value: { case: "int64", value: 2n } }, "page.name": { value: { case: "string", value: "first" } } },
        }),
      ),
    );
    const { traceparent } = started;
    expect(started.recording).toBe(true);
    expect(traceparent).toMatch(new RegExp(`^00-${traceId}-[0-9a-f]{16}-01$`));
    expect(sink.lookup(traceparent)).toBeDefined();
    sink.end(
      spanEndBytes({
        traceparent,
        endTime: timestampFromMs(end),
        status: SpanEnd_Status.OK,
        attributes: {
          "page.items": { value: { case: "int64", value: 3n } },
          "page.ratio": { value: { case: "double", value: 0.5 } },
          "page.cached": { value: { case: "bool", value: true } },
          "page.unset": {},
        },
      }),
    );
    expect(sink.lookup(traceparent)).toBeUndefined();
    const [span] = exporter.getFinishedSpans();
    expect(span?.name).toBe("svc.Method page");
    expect(span?.kind).toBe(SpanKind.CLIENT);
    expect(span?.parentSpanContext?.spanId).toBe(spanId);
    expect(traceparent.split("-")[2]).toBe(span?.spanContext().spanId);
    expect(traceparent.split("-")[2]).not.toBe(spanId);
    expect(span?.attributes["page.index"]).toBe(2);
    expect(span?.attributes["page.name"]).toBe("first");
    expect(span?.attributes["page.items"]).toBe(3);
    expect(span?.attributes["page.ratio"]).toBe(0.5);
    expect(span?.attributes["page.cached"]).toBe(true);
    expect(span?.attributes).not.toHaveProperty("page.unset");
    expect(span?.status.code).toBe(SpanStatusCode.OK);
    expect(Math.round(hr(span?.startTime ?? [0, 0]))).toBe(Math.round(start));
    expect(Math.round(hr(span?.endTime ?? [0, 0]))).toBe(Math.round(end));
  });

  it("starts a span under the trace state its start carries", () => {
    exporter.reset();
    const sink = new JsSpanSink(provider.getTracer("test"));
    const { traceparent } = startedOf(
      sink.start(
        spanStartBytes({
          name: "stateful",
          startTime: timestampFromMs(Date.now()),
          kind: SpanStart_Kind.INTERNAL,
          parent: `00-${traceId}-${spanId}-01`,
          tracestate: "vendor=value",
        }),
      ),
    );
    sink.end(spanEndBytes({ traceparent, endTime: timestampFromMs(Date.now()), status: SpanEnd_Status.UNSET }));
    const [span] = exporter.getFinishedSpans();
    expect(span?.spanContext().traceState?.serialize()).toBe("vendor=value");
  });

  it("starts a span without a parent as a new root, never under the active context", () => {
    exporter.reset();
    const sink = new JsSpanSink(provider.getTracer("test"));
    const active = trace.setSpanContext(ROOT_CONTEXT, { traceId, spanId, traceFlags: 1, isRemote: true });
    const { traceparent } = context.with(active, () =>
      startedOf(sink.start(spanStartBytes({ name: "root", startTime: timestampFromMs(Date.now()), kind: SpanStart_Kind.INTERNAL }))),
    );
    sink.end(spanEndBytes({ traceparent, endTime: timestampFromMs(Date.now()), status: SpanEnd_Status.UNSET }));
    const [span] = exporter.getFinishedSpans();
    expect(span?.parentSpanContext).toBeUndefined();
    expect(span?.spanContext().traceId).not.toBe(traceId);
  });

  it("opens a span without a start time at the current time", () => {
    exporter.reset();
    const sink = new JsSpanSink(provider.getTracer("test"));
    const { traceparent } = startedOf(sink.start(spanStartBytes({ name: "timeless", kind: SpanStart_Kind.UNSPECIFIED })));
    sink.end(spanEndBytes({ traceparent, status: SpanEnd_Status.UNSPECIFIED }));
    const [span] = exporter.getFinishedSpans();
    expect(span?.name).toBe("timeless");
    expect(span?.kind).toBe(SpanKind.INTERNAL);
    expect(span?.status.code).toBe(SpanStatusCode.UNSET);
  });

  it("marks a span that ended in failure", () => {
    exporter.reset();
    const sink = new JsSpanSink(provider.getTracer("test"));
    const now = Date.now();
    const { traceparent } = startedOf(sink.start(spanStartBytes({ name: "p", startTime: timestampFromMs(now - 5), kind: SpanStart_Kind.INTERNAL })));
    sink.end(spanEndBytes({ traceparent, endTime: timestampFromMs(now), status: SpanEnd_Status.ERROR, error: "canceled INTERNAL_SERVER_ERROR" }));
    const [span] = exporter.getFinishedSpans();
    expect(span?.status.code).toBe(SpanStatusCode.ERROR);
    expect(span?.status.message).toBe("canceled INTERNAL_SERVER_ERROR");
  });

  it("ignores an end for an unknown traceparent", () => {
    exporter.reset();
    const sink = new JsSpanSink(provider.getTracer("test"));
    sink.end(spanEndBytes({ traceparent: `00-${traceId}-${spanId}-01`, endTime: timestampFromMs(1), status: SpanEnd_Status.OK }));
    expect(exporter.getFinishedSpans()).toHaveLength(0);
  });

  it("ends a span that is not recording at once and answers that it is not recording", () => {
    const sink = new JsSpanSink(new BasicTracerProvider({ sampler: new AlwaysOffSampler() }).getTracer("off"));
    expect(startedOf(sink.start(spanStartBytes({ name: "p", startTime: timestampFromMs(1), kind: SpanStart_Kind.INTERNAL })))).toEqual({
      traceparent: "",
      recording: false,
    });
  });

  it("records a span under a tracer provider registered after the sink was built", () => {
    exporter.reset();
    trace.disable();
    const sink = new JsSpanSink();
    trace.disable();
    trace.setGlobalTracerProvider(provider);
    const { traceparent, recording } = startedOf(sink.start(spanStartBytes({ name: "late", startTime: timestampFromMs(Date.now()), kind: SpanStart_Kind.INTERNAL })));
    trace.disable();
    expect(recording).toBe(true);
    sink.end(spanEndBytes({ traceparent, endTime: timestampFromMs(Date.now()), status: SpanEnd_Status.UNSET }));
    expect(exporter.getFinishedSpans().map((span) => span.name)).toEqual(["late"]);
  });
});
