// Structural types matching the api_bones::response shapes on the wire.
// Defined inline so this package has zero runtime dependencies.

export interface ResponseMeta {
  request_id?: string | null;
  timestamp?: string | null;
  [key: string]: unknown;
}

export interface Link {
  rel: string;
  href: string;
  method?: string | null;
}

export type Links = Record<string, Link>;

export interface ApiResponseEnvelope<T = unknown> {
  data: T;
  meta: ResponseMeta;
  links?: Links | null;
}

// ---------------------------------------------------------------------------
// Structural Axios surface — keeps this package transport-agnostic so it
// works with both axios and any axios-compatible wrapper.
// ---------------------------------------------------------------------------

export interface EnvelopeAxiosRequestConfig {
  _envelopeMeta?: ResponseMeta;
  _envelopeLinks?: Links | null;
}

export interface EnvelopeAxiosResponse {
  data: unknown;
  status: number;
  headers?: unknown;
  config?: EnvelopeAxiosRequestConfig;
}

export interface AxiosInterceptorManager<V> {
  use(
    onFulfilled: (value: V) => V | Promise<V>,
    onRejected?: (error: unknown) => unknown,
  ): number;
  eject(id: number): void;
}

// Widened `V` to `any` so real axios instances (whose interceptor is typed for
// `AxiosResponse`, not `EnvelopeAxiosResponse`) are structurally assignable.
// The interceptor callback internally treats the value as `EnvelopeAxiosResponse`.
export interface AxiosRequestConfig {
  headers?: Record<string, string>;
  [key: string]: unknown;
}

export interface AxiosLikeInstance {
  interceptors: {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    response: AxiosInterceptorManager<any>;
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    request: AxiosInterceptorManager<any>;
  };
}

// ---------------------------------------------------------------------------
// Envelope detection
// ---------------------------------------------------------------------------

function isObject(x: unknown): x is Record<string, unknown> {
  return typeof x === "object" && x !== null;
}

function isApiResponseEnvelope(x: unknown): x is ApiResponseEnvelope {
  return isObject(x) && "data" in x && "meta" in x && isObject(x.meta);
}

// ---------------------------------------------------------------------------
// Interceptor installer
// ---------------------------------------------------------------------------

/**
 * Add a response interceptor to `instance` that unwraps the
 * `api_bones::response::ApiResponse<T>` envelope transparently.
 *
 * **Before**: `response.data` is `{ data: T, meta: ResponseMeta, links?: Links }`
 * **After**:  `response.data` is `T`
 *
 * The envelope metadata is stashed on `response.config._envelopeMeta` /
 * `response.config._envelopeLinks` so it remains accessible via
 * `getEnvelopeMeta(response.config)` and `getEnvelopeLinks(response.config)`.
 *
 * Only responses whose body matches the envelope shape are transformed;
 * plain JSON responses pass through unchanged.
 *
 * Returns the interceptor id so the caller can eject it via
 * `instance.interceptors.response.eject(id)`.
 *
 * @example
 * ```ts
 * import axios from "axios";
 * import { addEnvelopeUnwrapInterceptor } from "@brefwiz/api-bones-axios";
 *
 * const client = axios.create({ baseURL: "/api" });
 * addEnvelopeUnwrapInterceptor(client);
 *
 * // Payload is now User directly — no .data.data:
 * const { data: user } = await client.get<User>("/users/me");
 * ```
 */
export function addEnvelopeUnwrapInterceptor(instance: AxiosLikeInstance): number {
  return instance.interceptors.response.use(
    (response: EnvelopeAxiosResponse) => {
      if (isApiResponseEnvelope(response.data)) {
        const envelope = response.data as ApiResponseEnvelope;
        const cfg = (response.config ?? {}) as EnvelopeAxiosRequestConfig;
        cfg._envelopeMeta = envelope.meta;
        cfg._envelopeLinks = envelope.links ?? null;
        return { ...response, config: cfg, data: envelope.data };
      }
      return response;
    },
    (error: unknown) => Promise.reject(error),
  );
}

/**
 * Add a request interceptor to `instance` that injects the active OpenTelemetry
 * trace context (W3C `traceparent` / `tracestate` headers) on every outbound call.
 *
 * This integrates with the host application's globally-configured OpenTelemetry
 * setup (via `@opentelemetry/api`'s `context` and `propagation` APIs). If no
 * active span or context exists, no trace headers are injected.
 *
 * If `@opentelemetry/api` is not available at runtime (not installed or not
 * imported), this function silently succeeds without injecting headers.
 *
 * Returns the interceptor id so the caller can eject it via
 * `instance.interceptors.request.eject(id)`.
 *
 * @example
 * ```ts
 * import axios from "axios";
 * import { addTraceContextInterceptor } from "@brefwiz/api-bones-axios";
 *
 * const client = axios.create({ baseURL: "/api" });
 * addTraceContextInterceptor(client);
 *
 * // Outbound calls now carry W3C traceparent/tracestate headers
 * // linking them to the active span in the host app.
 * const { data: user } = await client.get<User>("/users/me");
 * ```
 */
export function addTraceContextInterceptor(instance: AxiosLikeInstance): number {
  return instance.interceptors.request.use(
    (config: AxiosRequestConfig) => {
      try {
        // Dynamically require @opentelemetry/api to avoid a hard dependency.
        // eslint-disable-next-line @typescript-eslint/no-require-imports
        const otel = require("@opentelemetry/api");
        const { context, propagation } = otel;

        if (!context || !propagation) {
          return config;
        }

        // Create a carrier (plain object) to receive injected headers
        const carrier: Record<string, string> = {};

        // Get the current active context and inject W3C trace headers
        const activeContext = context.active?.();
        if (activeContext) {
          propagation.inject?.(activeContext, carrier);
        }

        // Merge injected headers into the request config
        if (Object.keys(carrier).length > 0) {
          if (!config.headers) {
            config.headers = {};
          }
          Object.assign(config.headers, carrier);
        }
      } catch {
        // @opentelemetry/api not available; silently skip injection
      }

      return config;
    },
    (error: unknown) => Promise.reject(error),
  );
}

// ---------------------------------------------------------------------------
// Envelope metadata accessors
// ---------------------------------------------------------------------------

/**
 * Read the `ResponseMeta` stashed by the envelope interceptor.
 * Returns `null` when the interceptor was not installed or the response body
 * was not an `ApiResponse` envelope.
 */
export function getEnvelopeMeta(
  config: EnvelopeAxiosRequestConfig,
): ResponseMeta | null {
  return config._envelopeMeta ?? null;
}

/**
 * Read the HATEOAS links stashed by the envelope interceptor.
 * Returns `null` when absent or when the interceptor was not installed.
 */
export function getEnvelopeLinks(
  config: EnvelopeAxiosRequestConfig,
): Links | null {
  return config._envelopeLinks ?? null;
}
