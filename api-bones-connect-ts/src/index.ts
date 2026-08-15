// SPDX-License-Identifier: MIT
//
// Runtime-agnostic entry point: the policy contract and retry maths shared by
// both adapters. Importing this pulls in neither `@connectrpc/connect-web` nor
// `@connectrpc/connect-node`, so a consumer that only needs the types or the
// policy helpers never acquires a runtime it cannot use.
//
// Transports live behind explicit subpaths, so the runtime is a deliberate
// choice at the composition root rather than something resolved by accident:
//
//   import { configureConnectTransport }     from "@brefwiz/api-bones-connect/web";
//   import { configureNodeConnectTransport } from "@brefwiz/api-bones-connect/node";

export {
  DEFAULT_BACKOFF,
  computeBackoffDelay,
  resolveBackoff,
  type BackoffConfig,
  type BackoffOptions,
} from "./backoff.js";

export {
  MAX_CONNECT_GET_URL_BYTES,
  MAX_PRIVATE_CACHE_TTL_SECONDS,
  eligibleBrowserReadPolicy,
  indexGeneratedPolicy,
  parseMethodPolicy,
  type GeneratedBrowserCachePolicy,
  type GeneratedMethodPolicy,
  type GeneratedMethodPolicyDocument,
  type SdkTransportProfile,
} from "./policy.js";

export {
  MAX_RETRY_ATTEMPTS,
  RetryThrottle,
  isConnectionWriteFailure,
  isRetryableMethod,
  makeRetryInterceptor,
  rpcIdentity,
  serverPushbackMs,
  type RetryInterceptorOptions,
  type RetryThrottleOptions,
} from "./retry.js";

export { scopedCallOptions } from "./scoped.js";
