// SPDX-License-Identifier: MIT
import { injectTraceContext } from "@brefwiz/api-bones-otel";
import { propagation, ROOT_CONTEXT, trace, SpanKind, SpanStatusCode } from "@opentelemetry/api";
import type { Attributes, Span, Tracer } from "@opentelemetry/api";

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

/** What the Rust core reports when it opens a span. Times are epoch milliseconds. */
export interface SpanStart {
  readonly name: string;
  readonly startTime: number;
  readonly kind: "client" | "internal";
  /** The parent's W3C `traceparent`; absent for a new root. */
  readonly parent?: string;
  readonly tracestate?: string;
  readonly attributes?: Attributes;
}

/** What the Rust core reports when it ends a span it opened. */
export interface SpanEnd {
  /** The value `start` returned. */
  readonly traceparent: string;
  readonly endTime: number;
  readonly status: "ok" | "error" | "unset";
  /** `<connectCode> <code>`, set when `status` is `error`. */
  readonly error?: string;
  readonly attributes?: Attributes;
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
   * Opens a span from a `SpanStart` JSON document and returns its
   * `traceparent`. A span that is not recording is ended at once and gives
   * `undefined`, so the core mints its own ids.
   */
  start(json: string): string | undefined {
    const begin = JSON.parse(json) as SpanStart;
    const parent =
      begin.parent === undefined
        ? ROOT_CONTEXT
        : propagation.extract(ROOT_CONTEXT, { traceparent: begin.parent, tracestate: begin.tracestate });
    const tracer = this.#tracer ?? trace.getTracer("@brefwiz/api-bones-sdk-runtime");
    const span = tracer.startSpan(
      begin.name,
      {
        startTime: begin.startTime,
        kind: begin.kind === "client" ? SpanKind.CLIENT : SpanKind.INTERNAL,
        attributes: begin.attributes,
      },
      parent,
    );
    if (!span.isRecording()) {
      span.end();
      return undefined;
    }
    const { traceId, spanId, traceFlags } = span.spanContext();
    const traceparent = `00-${traceId}-${spanId}-${traceFlags.toString(16).padStart(2, "0")}`;
    this.#open.set(traceparent, span);
    return traceparent;
  }

  /** Ends the span `start` returned, from a `SpanEnd` JSON document. An unknown `traceparent` is ignored. */
  end(json: string): void {
    const finish = JSON.parse(json) as SpanEnd;
    const span = this.#open.get(finish.traceparent);
    if (span === undefined) return;
    this.#open.delete(finish.traceparent);
    if (finish.attributes !== undefined) span.setAttributes(finish.attributes);
    if (finish.status === "error") span.setStatus({ code: SpanStatusCode.ERROR, message: finish.error });
    else if (finish.status === "ok") span.setStatus({ code: SpanStatusCode.OK });
    span.end(finish.endTime);
  }

  /** The open span for `traceparent`, if the core has not ended it. */
  lookup(traceparent: string): Span | undefined {
    return this.#open.get(traceparent);
  }
}
