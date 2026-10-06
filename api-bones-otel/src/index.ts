// OpenTelemetry W3C trace-context propagation for Connect-ES SDKs.
// Provides interceptors for automatic trace-context injection into outbound requests.

import { context, propagation, type Context } from "@opentelemetry/api";
import type { Interceptor } from "@connectrpc/connect";

// ---------------------------------------------------------------------------
// Trace context injection helpers
// ---------------------------------------------------------------------------

const identityHeaders = new Set(["authorization", "x-org-id", "x-org-path", "x-subject-id"]);

function permittedHeader(key: string): boolean {
  return !identityHeaders.has(key.toLowerCase()) &&
    key !== "__proto__" && key !== "prototype" && key !== "constructor";
}

/**
 * Capture the currently active OpenTelemetry context for later use.
 *
 * Capture synchronously while the request span is active, then pass the
 * returned context to {@link injectTraceContext} from delayed callbacks whose
 * async resource may have been created before the span. This avoids relying on
 * ambient context after crossing EventEmitter, stream, or callback boundaries.
 */
export function captureTraceContext(): Context {
  return context.active();
}

/**
 * Inject the active OpenTelemetry trace context as W3C trace headers
 * into a carrier (plain object, Headers, or Map-like).
 *
 * This is a generic helper compatible with any HTTP client library.
 * Installed propagators may carry baggage and ordinary custom header families.
 * Identity headers (`authorization`, `x-org-id`, `x-org-path`, `x-subject-id`)
 * are reserved case-insensitively, even when absent. Propagators receive a
 * private plain staging object, never the caller carrier. Only own primitive
 * string data properties are copied; accessors and prototype keys are ignored.
 * Custom propagators relying on the caller carrier's identity or type must use
 * the supplied setter instead. Retaining staging cannot mutate the caller later.
 * Permitted writes before an injection error survive. With no propagator,
 * this is a no-op.
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
  const staged: Record<string, string> = Object.create(null);
  try {
    propagation.inject(ctx ?? captureTraceContext(), staged, {
      set: (target, key, value) => {
        if (typeof key === "string" && typeof value === "string" && permittedHeader(key)) {
          Object.defineProperty(target, key, { value, enumerable: true, configurable: true, writable: true });
        }
      },
    });
  } catch {
    // Keep permitted writes already staged; instrumentation stays best effort.
  }
  for (const [key, descriptor] of Object.entries(Object.getOwnPropertyDescriptors(staged))) {
    if (!("value" in descriptor) || typeof descriptor.value !== "string" || !permittedHeader(key)) {
      continue;
    }
    try {
      if (carrier instanceof Headers || carrier instanceof Map) {
        carrier.set(key, descriptor.value);
      } else {
        Object.defineProperty(carrier, key, {
          value: descriptor.value, enumerable: true, configurable: true, writable: true,
        });
      }
    } catch {
      // One refused destination write must not prevent other permitted writes.
    }
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
