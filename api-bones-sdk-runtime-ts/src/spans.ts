// SPDX-License-Identifier: MIT
import { injectTraceContext } from "@brefwiz/api-bones-otel";
import { propagation, ROOT_CONTEXT, trace, SpanKind, SpanStatusCode } from "@opentelemetry/api";
import type { Attributes, Span, Tracer } from "@opentelemetry/api";
import { create, fromBinary, toBinary } from "@bufbuild/protobuf";
import { timestampMs } from "@bufbuild/protobuf/wkt";
import {
  SpanEnd_Status,
  SpanEndSchema,
  SpanStart_Kind,
  SpanStartSchema,
  SpanStartedSchema,
  type AttributeValue,
} from "./generated/bones/sdk/v1/boundary_pb.js";

/** The per-call inputs the host hands the Rust core. */
export interface CallCtx {
  signal?: AbortSignal;
  traceparent?: string;
  tracestate?: string;
}

/**
 * The call context for the active JavaScript trace: the W3C trace fields the
 * registered propagator writes for `context.active()`, plus the caller's abort
 * signal. Only the host can see the active context.
 */
export function callCtx(signal?: AbortSignal): CallCtx {
  const carrier: Record<string, string> = {};
  injectTraceContext(carrier);
  const ctx: CallCtx = {};
  if (signal !== undefined) ctx.signal = signal;
  if (carrier["traceparent"] !== undefined) ctx.traceparent = carrier["traceparent"];
  if (carrier["tracestate"] !== undefined) ctx.tracestate = carrier["tracestate"];
  return ctx;
}

/** The OpenTelemetry attributes of a span's attribute map; a value that names no member of its oneof is dropped. */
function attributesOf(map: { readonly [name: string]: AttributeValue }): Attributes {
  const attributes: Attributes = {};
  for (const [name, attribute] of Object.entries(map)) {
    const { value } = attribute;
    if (value.case === undefined) continue;
    attributes[name] = value.case === "int64" ? Number(value.value) : value.value;
  }
  return attributes;
}

/** The live JavaScript OpenTelemetry spans of the Rust core's spans. */
export class JsSpanSink {
  readonly #tracer: Tracer | undefined;
  readonly #open = new Map<string, Span>();

  /**
   * Without a `tracer` each span is started on the tracer the global provider
   * gives at that moment, so a provider registered after this sink was built
   * (the wrapper builds its sink at module load) still receives the spans.
   */
  constructor(tracer?: Tracer) {
    this.#tracer = tracer;
  }

  /**
   * Opens a span from the bytes of a `bones.sdk.v1.SpanStart` and answers the
   * bytes of a `SpanStarted`: the span's `traceparent`. A span that is not
   * recording is ended at once and answers `recording: false` with no
   * `traceparent`, so the core mints its own ids.
   */
  start(bytes: Uint8Array): Uint8Array {
    const begin = fromBinary(SpanStartSchema, bytes);
    const parent =
      begin.parent === undefined
        ? ROOT_CONTEXT
        : propagation.extract(ROOT_CONTEXT, { traceparent: begin.parent, tracestate: begin.tracestate });
    const tracer = this.#tracer ?? trace.getTracer("@brefwiz/api-bones-sdk-runtime");
    const span = tracer.startSpan(
      begin.name,
      {
        ...(begin.startTime === undefined ? {} : { startTime: timestampMs(begin.startTime) }),
        kind: begin.kind === SpanStart_Kind.CLIENT ? SpanKind.CLIENT : SpanKind.INTERNAL,
        attributes: attributesOf(begin.attributes),
      },
      parent,
    );
    if (!span.isRecording()) {
      span.end();
      return toBinary(SpanStartedSchema, create(SpanStartedSchema, { traceparent: "", recording: false }));
    }
    const { traceId, spanId, traceFlags } = span.spanContext();
    const traceparent = `00-${traceId}-${spanId}-${traceFlags.toString(16).padStart(2, "0")}`;
    this.#open.set(traceparent, span);
    return toBinary(SpanStartedSchema, create(SpanStartedSchema, { traceparent, recording: true }));
  }

  /** Ends the span `start` returned, from the bytes of a `bones.sdk.v1.SpanEnd`. An unknown `traceparent` is ignored. */
  end(bytes: Uint8Array): void {
    const finish = fromBinary(SpanEndSchema, bytes);
    const span = this.#open.get(finish.traceparent);
    if (span === undefined) return;
    this.#open.delete(finish.traceparent);
    span.setAttributes(attributesOf(finish.attributes));
    if (finish.status === SpanEnd_Status.ERROR) span.setStatus({ code: SpanStatusCode.ERROR, message: finish.error });
    else if (finish.status === SpanEnd_Status.OK) span.setStatus({ code: SpanStatusCode.OK });
    span.end(finish.endTime === undefined ? undefined : timestampMs(finish.endTime));
  }

  /** The open span for `traceparent`, if the core has not ended it. */
  lookup(traceparent: string): Span | undefined {
    return this.#open.get(traceparent);
  }
}
