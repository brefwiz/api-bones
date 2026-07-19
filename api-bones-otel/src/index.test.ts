import { describe, it, expect, beforeEach } from "vitest";
import { context, trace, propagation, SpanContext, TraceFlags } from "@opentelemetry/api";
import { W3CTraceContextPropagator } from "@opentelemetry/core";
import type { UnaryRequest } from "@connectrpc/connect";
import { addTraceContextInterceptor, injectTraceContext } from "./index";

// Register a W3C propagator for testing
const propagator = new W3CTraceContextPropagator();
propagation.setGlobalPropagator(propagator);

// Mock Connect-ES types for testing
function createMockRequest(headers?: Record<string, string>): UnaryRequest {
  return {
    header: new Headers(headers ?? {}),
    message: {},
    method: { kind: "unary" },
  } as UnaryRequest;
}

// Create a mock span context for testing
function createMockSpanContext(): SpanContext {
  return {
    traceId: "4bf92f3577b34da6a3ce929d0e0e4736",
    spanId: "00f067aa0ba902b7",
    traceFlags: TraceFlags.SAMPLED,
    traceState: undefined,
  };
}

describe("injectTraceContext", () => {
  beforeEach(() => {
    propagation.setGlobalPropagator(propagator);
  });

  it("injects trace context into plain object carrier with explicit context", () => {
    const spanCtx = createMockSpanContext();
    const ctx = trace.setSpanContext(context.active(), spanCtx);

    const carrier: Record<string, string> = {};
    injectTraceContext(carrier, ctx);

    // W3C trace context propagator sets traceparent header
    expect(carrier.traceparent).toBeDefined();
    expect(carrier.traceparent).toMatch(/^00-[a-f0-9]{32}-[a-f0-9]{16}-[01][0-9a-f]$/);
  });

  it("injects trace context into Headers carrier with explicit context", () => {
    const spanCtx = createMockSpanContext();
    const ctx = trace.setSpanContext(context.active(), spanCtx);

    const headers = new Headers();
    injectTraceContext(headers, ctx);

    // W3C trace context propagator sets traceparent header
    expect(headers.has("traceparent")).toBe(true);
    const traceparent = headers.get("traceparent");
    expect(traceparent).toMatch(/^00-[a-f0-9]{32}-[a-f0-9]{16}-[01][0-9a-f]$/);
  });

  it("injects trace context into Map carrier with explicit context", () => {
    const spanCtx = createMockSpanContext();
    const ctx = trace.setSpanContext(context.active(), spanCtx);

    const carrier = new Map<string, string>();
    injectTraceContext(carrier, ctx);

    // W3C trace context propagator sets traceparent header
    expect(carrier.has("traceparent")).toBe(true);
    const traceparent = carrier.get("traceparent");
    expect(traceparent).toMatch(/^00-[a-f0-9]{32}-[a-f0-9]{16}-[01][0-9a-f]$/);
  });

  it("is a no-op with no active span", () => {
    const carrier: Record<string, string> = {};

    // Ensure no active span
    injectTraceContext(carrier);

    // With no active span, most propagators don't inject anything
    expect(carrier.traceparent).toBeUndefined();
  });

  it("accepts explicit context parameter", () => {
    const spanCtx = createMockSpanContext();
    const explicitCtx = trace.setSpanContext(context.active(), spanCtx);

    const carrier: Record<string, string> = {};
    injectTraceContext(carrier, explicitCtx);

    expect(carrier.traceparent).toBeDefined();
    expect(carrier.traceparent).toMatch(/^00-[a-f0-9]{32}-[a-f0-9]{16}-[01][0-9a-f]$/);
  });

  it("handles errors gracefully", () => {
    // Pass an object without a proper set method — should not throw
    const badCarrier = Object.create(null);
    expect(() => {
      injectTraceContext(badCarrier);
    }).not.toThrow();
  });
});

describe("addTraceContextInterceptor", () => {
  it("calls next handler and preserves request", async () => {
    const interceptor = addTraceContextInterceptor();
    const req = createMockRequest({ "x-request-id": "123" });
    let nextCalled = false;

    const mockNext = async (req: UnaryRequest) => {
      nextCalled = true;
      return { message: { success: true } };
    };

    const result = await interceptor(mockNext)(req);

    expect(nextCalled).toBe(true);
    expect(result.message).toEqual({ success: true });
    // Original headers should be preserved
    expect(req.header.get("x-request-id")).toBe("123");
  });

  it("attempts trace context injection without breaking requests", async () => {
    // This test verifies that even if trace context injection has issues,
    // the request still goes through successfully (fail-safe behavior)
    const interceptor = addTraceContextInterceptor();
    const req = createMockRequest();
    let nextCalled = false;

    const mockNext = async (req: UnaryRequest) => {
      nextCalled = true;
      return { message: { processed: true } };
    };

    const result = await interceptor(mockNext)(req);

    // Verify the request went through successfully
    expect(nextCalled).toBe(true);
    expect(result.message).toEqual({ processed: true });
  });

  it("passes request through to next middleware unchanged", async () => {
    const interceptor = addTraceContextInterceptor();
    const mockNext = async (req: UnaryRequest) => {
      return { message: { result: "success" } };
    };

    const req = createMockRequest({ "x-custom": "value" });
    const response = await interceptor(mockNext)(req);

    expect(response.message).toEqual({ result: "success" });
    expect(req.header.get("x-custom")).toBe("value");
  });

  it("silently handles injection errors", async () => {
    const interceptor = addTraceContextInterceptor();
    let callCount = 0;
    const mockNext = async (req: UnaryRequest) => {
      callCount++;
      return { message: {} };
    };

    const req = createMockRequest();
    // Even with propagation disabled or errors, the request should still go through
    await interceptor(mockNext)(req);

    expect(callCount).toBe(1);
  });

  it("works with no active span", async () => {
    const interceptor = addTraceContextInterceptor();
    const mockNext = async (req: UnaryRequest) => {
      return { message: {} };
    };

    const req = createMockRequest();
    const response = await interceptor(mockNext)(req);

    // Request should pass through successfully even without an active span
    expect(response.message).toBeDefined();
  });
});
