// SPDX-License-Identifier: MIT
//
// Canonical browser Connect transport.
//
// Moved verbatim in behaviour from core-ui's src/api/connect-client.ts so the
// two runtime adapters can share one policy vocabulary. Everything browser-only
// stays here and only here: the CSRF meta-tag read, `credentials: "include"`,
// and the GET transport. A Node service must never acquire those by importing a
// transport — that is how a CLI ends up depending on `document`.

import {
  create,
  type DescMessage,
  type DescMethodUnary,
  type MessageInitShape,
  type MessageShape,
} from "@bufbuild/protobuf";
import { base64Encode } from "@bufbuild/protobuf/wire";
import { Code, ConnectError, type Interceptor, type Transport } from "@connectrpc/connect";
import { createClientMethodSerializers, createMethodUrl } from "@connectrpc/connect/protocol";
import { createConnectTransport, createGrpcWebTransport } from "@connectrpc/connect-web";

import type { BackoffOptions } from "./backoff";
import { computeBackoffDelay, resolveBackoff } from "./backoff";
import {
  eligibleBrowserReadPolicy,
  indexGeneratedPolicy,
  MAX_CONNECT_GET_URL_BYTES,
  type SdkTransportProfile,
} from "./policy";

export interface ConnectTransportOptions {
  baseUrl: string;
  /** Composition-root profile. Browser GET is opt-in through `webapp`. */
  profile: SdkTransportProfile;
  /** Generated, complete policy document emitted beside the SDK. */
  policy: unknown;
  /**
   * BFF mode (default): omit this. The transport sends credentials (the opaque
   * session cookie) on every request and never touches a bearer — no token ever
   * lives in JS. Provide a `getToken` only for the legacy direct-bearer path;
   * when present, `Authorization: Bearer <token>` is added.
   */
  getToken?: () => string | null | undefined;
  /**
   * Called when the server returns Unauthenticated. Typically triggers a logout
   * or redirect to the login page.
   */
  onUnauthorized?: () => void;
  /**
   * Use binary protobuf encoding instead of JSON. Default: false (JSON, more
   * debuggable in browser devtools).
   */
  useBinaryFormat?: boolean;
  /** Fall back to gRPC-Web transport instead of Connect protocol. */
  useGrpcWeb?: boolean;
  /** Retry options for transient unary failures. */
  retry?: BackoffOptions;
  /** Product interceptors, including tracing, composed ahead of core interceptors. */
  interceptors?: readonly Interceptor[];
  /** Fetch override for browser adapters and focused transport tests. */
  fetch?: typeof globalThis.fetch;
}

const RETRYABLE_CODES = new Set([Code.Unavailable, Code.Aborted, Code.ResourceExhausted]);
const MAX_RETRIES = 3;

function makeAuthInterceptor(getToken: () => string | null | undefined): Interceptor {
  return (next) => (req) => {
    const token = getToken();
    if (token) {
      req.header.set("Authorization", `Bearer ${token}`);
    }
    return next(req);
  };
}

/** Read the CSRF synchronizer token rendered into the page `<meta>`. */
function getCsrfTokenFromMeta(): string | null {
  if (typeof document === "undefined") return null;
  return document.querySelector('meta[name="csrf-token"]')?.getAttribute("content") || null;
}

/**
 * Attach `X-CSRF-Token` to Connect requests. It protects POST fallbacks and is
 * carried as a header, never as cache-key or URL material.
 */
function makeCsrfInterceptor(): Interceptor {
  const token = getCsrfTokenFromMeta();
  return (next) => (req) => {
    if (token) {
      req.header.set("X-CSRF-Token", token);
    }
    return next(req);
  };
}

function makeUnauthInterceptor(onUnauthorized: () => void): Interceptor {
  return (next) => async (req) => {
    try {
      return await next(req);
    } catch (err) {
      if (err instanceof ConnectError && err.code === Code.Unauthenticated) {
        onUnauthorized();
      }
      throw err;
    }
  };
}

function makeRetryInterceptor(retryOpts?: BackoffOptions): Interceptor {
  const backoff = resolveBackoff(retryOpts);
  return (next) => async (req) => {
    // Only retry unary requests — streams must handle reconnect separately.
    if (req.stream) return next(req);

    let attempt = 0;
    for (;;) {
      try {
        return await next(req);
      } catch (err) {
        if (err instanceof ConnectError && RETRYABLE_CODES.has(err.code) && attempt < MAX_RETRIES) {
          const delay = computeBackoffDelay(attempt, backoff);
          await new Promise<void>((resolve) => setTimeout(resolve, delay));
          attempt++;
          continue;
        }
        throw err;
      }
    }
  };
}

function rpcIdentity(method: DescMethodUnary): string {
  return `/${method.parent.typeName}/${method.name}`;
}

function encodedConnectGetUrlBytes<I extends DescMessage, O extends DescMessage>(
  baseUrl: string,
  method: DescMethodUnary<I, O>,
  input: MessageInitShape<I>,
  useBinaryFormat: boolean,
): number {
  const { serialize } = createClientMethodSerializers(method, useBinaryFormat);
  const message = create(method.input, input);
  const bytes = serialize(message as MessageShape<I>);
  const encodedMessage = useBinaryFormat
    ? base64Encode(bytes, "url")
    : encodeURIComponent(new TextDecoder().decode(bytes));
  const query = useBinaryFormat
    ? `?connect=v1&encoding=proto&base64=1&message=${encodedMessage}`
    : `?connect=v1&encoding=json&message=${encodedMessage}`;
  return new TextEncoder().encode(`${createMethodUrl(baseUrl, method)}${query}`).byteLength;
}

function withCredentials(fetchImpl: typeof globalThis.fetch): typeof globalThis.fetch {
  return (input, init) =>
    fetchImpl(input, {
      ...init,
      credentials: "include",
      cache: init?.method === "GET" ? "no-cache" : init?.cache,
    });
}

/**
 * Build a Connect transport preconfigured for all brefwiz services.
 *
 * @example
 * ```ts
 * import { configureConnectTransport } from "@brefwiz/api-bones-connect/web";
 * import { createClient } from "@connectrpc/connect";
 * import { RulesService } from "./generated/rules_connect";
 * import policy from "./generated/connect-method-policy.json";
 *
 * const transport = configureConnectTransport({
 *   baseUrl: import.meta.env.VITE_API_URL,
 *   profile: "webapp",
 *   policy,
 * });
 *
 * const rulesClient = createClient(RulesService, transport);
 * ```
 */
export function configureConnectTransport(opts: ConnectTransportOptions): Transport {
  const { baseUrl, profile, policy, getToken, onUnauthorized, useBinaryFormat, useGrpcWeb, retry } =
    opts;
  const policyByRpc = indexGeneratedPolicy(policy);

  const interceptors: Interceptor[] = [
    ...(opts.interceptors ?? []),
    ...(getToken ? [makeAuthInterceptor(getToken)] : []),
    makeCsrfInterceptor(),
    ...(onUnauthorized ? [makeUnauthInterceptor(onUnauthorized)] : []),
    makeRetryInterceptor(retry),
  ];

  const transportOpts = {
    baseUrl,
    useBinaryFormat: useBinaryFormat ?? false,
    interceptors,
    fetch: withCredentials(opts.fetch ?? globalThis.fetch),
  };

  const postTransport = useGrpcWeb
    ? createGrpcWebTransport(transportOpts)
    : createConnectTransport(transportOpts);
  if (profile !== "webapp" || useGrpcWeb) return postTransport;

  const getTransport = createConnectTransport({
    ...transportOpts,
    useHttpGet: true,
  });

  return {
    async unary(method, signal, timeoutMs, header, input, contextValues) {
      const methodPolicy = eligibleBrowserReadPolicy(policyByRpc.get(rpcIdentity(method)));
      if (!methodPolicy) {
        return postTransport.unary(method, signal, timeoutMs, header, input, contextValues);
      }
      const urlBytes = encodedConnectGetUrlBytes(baseUrl, method, input, useBinaryFormat ?? false);
      const maxBytes = Math.min(MAX_CONNECT_GET_URL_BYTES, methodPolicy.maxEncodedUrlBytes);
      const selected = urlBytes <= maxBytes ? getTransport : postTransport;
      return selected.unary(method, signal, timeoutMs, header, input, contextValues);
    },
    stream(method, signal, timeoutMs, header, input, contextValues) {
      return postTransport.stream(method, signal, timeoutMs, header, input, contextValues);
    },
  };
}
