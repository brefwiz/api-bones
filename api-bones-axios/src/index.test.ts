import { describe, it, expect, afterEach } from "vitest";
import {
  addEnvelopeUnwrapInterceptor,
  addTraceContextInterceptor,
  getEnvelopeMeta,
  getEnvelopeLinks,
  type AxiosLikeInstance,
  type EnvelopeAxiosResponse,
  type AxiosRequestConfig,
} from "./index";

function makeInstance() {
  let handler: ((r: EnvelopeAxiosResponse) => EnvelopeAxiosResponse) | null = null;
  const instance: AxiosLikeInstance = {
    interceptors: {
      response: {
        use(onFulfilled) {
          handler = onFulfilled as typeof handler;
          return 0;
        },
        eject() {},
      },
    },
  };
  return {
    instance,
    intercept(response: EnvelopeAxiosResponse): EnvelopeAxiosResponse {
      return handler!(response);
    },
  };
}

describe("addEnvelopeUnwrapInterceptor", () => {
  it("unwraps data and stashes meta + links", () => {
    const { instance, intercept } = makeInstance();
    addEnvelopeUnwrapInterceptor(instance);

    const meta = { request_id: "req-1", timestamp: "2024-01-01T00:00:00Z" };
    const links = { self: { rel: "self", href: "/items/1" } };
    const payload = { id: "1", name: "item" };

    const result = intercept({
      data: { data: payload, meta, links },
      status: 200,
      config: {},
    });

    expect(result.data).toEqual(payload);
    expect(getEnvelopeMeta(result.config!)).toEqual(meta);
    expect(getEnvelopeLinks(result.config!)).toEqual(links);
  });

  it("passes non-envelope responses through unchanged", () => {
    const { instance, intercept } = makeInstance();
    addEnvelopeUnwrapInterceptor(instance);

    const plain = { id: "1" };
    const result = intercept({ data: plain, status: 200 });
    expect(result.data).toEqual(plain);
  });

  it("returns null meta/links when response was not an envelope", () => {
    const { instance, intercept } = makeInstance();
    addEnvelopeUnwrapInterceptor(instance);

    const result = intercept({ data: { id: "1" }, status: 200, config: {} });
    expect(getEnvelopeMeta(result.config!)).toBeNull();
    expect(getEnvelopeLinks(result.config!)).toBeNull();
  });
});

// Helper to create a mock axios-like instance for request interceptors
function makeRequestInstance() {
  let handler: ((r: AxiosRequestConfig) => AxiosRequestConfig) | null = null;
  const instance: AxiosLikeInstance = {
    interceptors: {
      request: {
        use(onFulfilled) {
          handler = onFulfilled as typeof handler;
          return 0;
        },
        eject() {},
      },
      response: {
        use() {
          return 0;
        },
        eject() {},
      },
    },
  };
  return {
    instance,
    intercept(config: AxiosRequestConfig): AxiosRequestConfig {
      return handler!(config);
    },
  };
}

describe("addTraceContextInterceptor", () => {
  afterEach(() => {
    // Clean up any OpenTelemetry context after each test
    try {
      // eslint-disable-next-line @typescript-eslint/no-require-imports
      const otel = require("@opentelemetry/api");
      if (otel?.context?.with) {
        // Reset context if possible (for testing purposes)
      }
    } catch {
      // OpenTelemetry not available
    }
  });

  it("does not inject traceparent header when no active span/context exists", () => {
    const { instance, intercept } = makeRequestInstance();
    addTraceContextInterceptor(instance);

    const config: AxiosRequestConfig = { headers: {} };
    const result = intercept(config);

    // When no active span exists, no trace headers should be injected
    expect(result.headers?.traceparent).toBeUndefined();
    expect(result.headers?.tracestate).toBeUndefined();
  });

  it("successfully injects headers from active context when OpenTelemetry is available", () => {
    // eslint-disable-next-line @typescript-eslint/no-require-imports
    const otel = require("@opentelemetry/api");

    // Mock the propagation.inject to verify it gets called with correct args
    let injectedCarrier: Record<string, string> | null = null;
    let capturedContext: unknown = null;

    const originalInject = otel.propagation.inject;
    otel.propagation.inject = (ctx: unknown, carrier: Record<string, string>) => {
      capturedContext = ctx;
      injectedCarrier = carrier;
      // Simulate injecting a traceparent header
      carrier["traceparent"] = "00-8788271d61ce046176830d210e0df032-dfb2246ad6f27b1b-01";
    };

    try {
      const { instance, intercept } = makeRequestInstance();
      addTraceContextInterceptor(instance);

      const config: AxiosRequestConfig = { headers: {} };
      const result = intercept(config);

      // Verify the injected carrier was used
      expect(injectedCarrier).not.toBeNull();
      expect(result?.headers?.traceparent).toBe(
        "00-8788271d61ce046176830d210e0df032-dfb2246ad6f27b1b-01"
      );
    } finally {
      // Restore the original inject function
      otel.propagation.inject = originalInject;
    }
  });

  it("preserves existing headers when injecting trace context", () => {
    const { instance, intercept } = makeRequestInstance();
    addTraceContextInterceptor(instance);

    const config: AxiosRequestConfig = {
      headers: {
        "Authorization": "Bearer token123",
        "Content-Type": "application/json",
      },
    };
    const result = intercept(config);

    // Existing headers should be preserved
    expect(result.headers?.["Authorization"]).toBe("Bearer token123");
    expect(result.headers?.["Content-Type"]).toBe("application/json");
  });

  it("handles config with no headers gracefully", () => {
    const { instance, intercept } = makeRequestInstance();
    addTraceContextInterceptor(instance);

    const config: AxiosRequestConfig = {};
    const result = intercept(config);

    // Should not throw and config should be returned
    expect(result).toBeDefined();
  });

  it("returns the interceptor id for ejection", () => {
    const { instance } = makeRequestInstance();
    const id = addTraceContextInterceptor(instance);

    // Should return a numeric id that can be used for ejection
    expect(typeof id).toBe("number");
  });
});
