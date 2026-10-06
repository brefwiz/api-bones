import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { context, trace, propagation, ROOT_CONTEXT, SpanContext, TraceFlags, type TextMapPropagator, type TextMapSetter, type TextMapGetter, type Context } from "@opentelemetry/api";
import { W3CTraceContextPropagator } from "@opentelemetry/core";
import type { UnaryRequest } from "@connectrpc/connect";
import { addTraceContextInterceptor, captureTraceContext, injectTraceContext } from "./index";

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

  it("injects a captured context after ambient context changes", () => {
    const activeCtx = trace.setSpanContext(context.active(), createMockSpanContext());
    const active = vi.spyOn(context, "active").mockReturnValue(activeCtx);
    const delayedCallbackSource = new EventTarget();
    const captured = captureTraceContext();
    const carrier: Record<string, string> = {};

    active.mockReturnValue(ROOT_CONTEXT);
    delayedCallbackSource.addEventListener("end", () => {
      injectTraceContext(carrier, captured);
    });
    delayedCallbackSource.dispatchEvent(new Event("end"));

    expect(carrier.traceparent).toBe(
      "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
    );
    active.mockRestore();
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


describe("propagation carrier isolation", () => {
  let retained: Record<string, unknown> | undefined;
  let getterCalls = 0;
  const hostile: TextMapPropagator = {
    inject<C>(_ctx: Context, carrier: C, setter: TextMapSetter<C>) {
      const staged = carrier as Record<string, unknown>;
      retained = staged;
      setter.set(carrier, "x-before-error", "kept");
      for (const key of ["Authorization", "X-Org-Id", "X-Org-Path", "X-Subject-Id"]) {
        setter.set(carrier, key, "setter-identity");
        staged[key] = "direct-identity";
      }
      staged["x-direct-context"] = "kept";
      staged["x-non-string"] = { toString() { throw new Error("coercion must not run"); } };
      Object.defineProperty(staged, "x-accessor", { get() { getterCalls++; throw new Error("getter must not run"); }, enumerable: true });
      Object.defineProperty(staged, "__proto__", { value: "poison", enumerable: true });
      Object.defineProperty(staged, "constructor", { value: "poison", enumerable: true });
      staged.prototype = "poison";
      Object.setPrototypeOf(staged, { "x-inherited": "poison" });
      throw new Error("extension failed");
    },
    extract<C>(ctx: Context, _carrier: C, _getter: TextMapGetter<C>) { return ctx; },
    fields: () => [],
  };
  beforeEach(() => {
    vi.restoreAllMocks();
    retained = undefined;
    getterCalls = 0;
    propagation.disable();
    expect(propagation.setGlobalPropagator(hostile)).toBe(true);
  });
  afterEach(() => {
    propagation.disable();
    propagation.setGlobalPropagator(propagator);
  });
  it.each(["object", "headers", "map"])("protects present and absent identity on %s carriers", (kind) => {
    for (const present of [false, true]) {
      const initial: Record<string, string> = present ? { authorization: "caller", "x-org-id": "caller", "x-org-path": "caller", "x-subject-id": "caller" } : {};
      const carrier = kind === "headers" ? new Headers(initial) : kind === "map" ? new Map(Object.entries(initial)) : { ...initial };
      const prototype = Object.getPrototypeOf(carrier);
      injectTraceContext(carrier);
      const read = (key: string) => carrier instanceof Headers || carrier instanceof Map ? carrier.get(key) : (carrier as Record<string, string>)[key];
      for (const key of ["authorization", "x-org-id", "x-org-path", "x-subject-id"]) {
        expect(read(key) ?? undefined).toBe(present ? "caller" : undefined);
        expect(read(key.toUpperCase()) ?? undefined).toBe(kind === "headers" && present ? "caller" : undefined);
      }
      expect(read("x-before-error")).toBe("kept");
      expect(read("x-direct-context")).toBe("kept");
      for (const key of ["x-accessor", "x-non-string", "x-inherited"]) expect(read(key) ?? undefined).toBeUndefined();
      for (const key of ["__proto__", "prototype", "constructor"]) {
        expect(carrier instanceof Headers || carrier instanceof Map ? carrier.has(key) : Object.hasOwn(carrier, key)).toBe(false);
      }
      expect(getterCalls).toBe(0);
      expect(Object.getPrototypeOf(carrier)).toBe(prototype);
      expect(retained).not.toBe(carrier);
      retained!["authorization"] = "late-identity";
      retained!["x-direct-context"] = "late-context";
      expect(read("authorization") ?? undefined).toBe(present ? "caller" : undefined);
      expect(read("x-direct-context")).toBe("kept");
    }
  });
});
