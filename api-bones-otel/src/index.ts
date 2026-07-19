// OpenTelemetry W3C trace-context propagation for Connect-ES SDKs.
// Provides interceptors for automatic trace-context injection into outbound requests.

import { context, propagation, type Context } from "@opentelemetry/api";
import type { Interceptor } from "@connectrpc/connect";

// ---------------------------------------------------------------------------
// Trace context injection helpers
// ---------------------------------------------------------------------------

/**
 * Inject the active OpenTelemetry trace context as W3C trace headers
 * into a carrier (plain object, Headers, or Map-like).
 *
 * This is a generic helper compatible with any HTTP client library.
 * With no active span or propagator configured, this is a no-op.
 *
 * @param carrier - A plain object, Headers instance, or Map-like object with `.set(key, value)`
 * @param ctx - Optional explicit context. If omitted, uses `context.active()`
 *
 * @example
 * ```ts
 * import { injectTraceContext } from "@brefwiz/api-bones-otel";
 * import axios from "axios";
 *
 * const headers: Record<string, string> = {};
 * injectTraceContext(headers);
 * const response = await axios.get("/api/users", { headers });
 * ```
 */
export function injectTraceContext(
  carrier: Record<string, string> | Headers | Map<string, string>,
  ctx?: Context,
): void {
  try {
    const targetContext = ctx ?? context.active();
    propagation.inject(targetContext, carrier, {
      set: (c: Record<string, string> | Headers | Map<string, string>, key: string, value: string) => {
        if (c instanceof Headers) {
          c.set(key, value);
        } else if (c instanceof Map) {
          c.set(key, value);
        } else {
          c[key as keyof typeof c] = value as never;
        }
      },
    });
  } catch {
    // Silently ignore errors — injecting trace context should never break a request
  }
}

// ---------------------------------------------------------------------------
// Connect-ES Interceptor
// ---------------------------------------------------------------------------

/**
 * Create a Connect-ES `Interceptor` that injects the active OpenTelemetry
 * trace context as W3C trace headers into each outbound request.
 *
 * Pass this to `createConnectTransport()` or transport middleware.
 *
 * With no active span or propagator configured, this is a no-op and the
 * request proceeds unchanged. Trace injection failures are silently ignored
 * to prevent network requests from failing due to instrumentation issues.
 *
 * @returns A Connect-ES `Interceptor` ready for use in `createConnectTransport`
 *
 * @example
 * ```ts
 * import { createConnectTransport } from "@connectrpc/connect-web";
 * import { addTraceContextInterceptor } from "@brefwiz/api-bones-otel";
 *
 * const transport = createConnectTransport({
 *   baseUrl: "https://api.example.com",
 *   interceptors: [addTraceContextInterceptor()],
 * });
 * ```
 */
export function addTraceContextInterceptor(): Interceptor {
  return (next) => {
    return async (req) => {
      try {
        // Both UnaryRequest and StreamRequest have .header property
        injectTraceContext((req as { header: Headers }).header);
      } catch {
        // Silently ignore injection errors
      }
      return next(req);
    };
  };
}
