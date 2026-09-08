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

// The Connect error vocabulary this package's predicates classify. Re-exported
// so a consumer reasoning about retry eligibility takes the code and the error
// type from the same place as the predicates that read them.
export { Code, ConnectError } from "@connectrpc/connect";
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
  connectionFailureAsUnavailable,
  isConnectionWriteFailure,
  isReplayableTransportFailure,
  isRetryableMethod,
  isUnpromptedRetryable,
  makeConnectionFailureNormalizer,
  makeRetryInterceptor,
  rpcIdentity,
  serverPushbackMs,
  type RetryInterceptorOptions,
  type RetryThrottleOptions,
} from "./retry.js";

export { scopedCallOptions } from "./scoped.js";

// Workload API identity. Exported from the root because the error is something
// a composition root catches, and the trust-domain derivation is something a
// test asserts — neither requires pulling in the Node transport.
export {
  WorkloadIdentityError,
  workloadClientTlsIdentity,
  clientTlsIdentityFor,
  trustDomainOf,
  WATCHER_ATTEMPTS,
} from "./workload-identity.js";
export type { WorkloadIdentityErrorKind } from "./workload-identity.js";
