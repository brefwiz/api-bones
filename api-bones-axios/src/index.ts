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
  [key: string]: unknown;
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

export interface AxiosLikeInstance {
  interceptors: {
    response: AxiosInterceptorManager<EnvelopeAxiosResponse>;
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
