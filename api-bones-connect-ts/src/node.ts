// SPDX-License-Identifier: MIT
//
// Canonical Node/service Connect transport.
//
// This is the half that did not exist. The browser adapter has always been able
// to name its profile; a CLI, a mesh service, or a test harness had no
// canonical helper at all, so every one of them constructed a transport by hand
// — which is what `connect-read-profile` flags, and what nobody could fix
// because there was nothing to migrate to.
//
// A service transport is POST-only BY CONSTRUCTION, not by policy. The profile
// enum's only behavioural effect is browser GET eligibility, and a Node process
// issues no browser requests: there is no cache to populate, no URL length
// limit to respect, and no cross-origin credential story. So this adapter
// accepts the same policy document as the browser one and uses it for nothing
// but validation — see `configureNodeConnectTransport` for why it takes it at
// all rather than pretending the two profiles are unrelated.

import type { Interceptor, Transport } from "@connectrpc/connect";
import { Code, ConnectError } from "@connectrpc/connect";
import {
  compressionBrotli,
  compressionGzip,
  createConnectTransport,
} from "@connectrpc/connect-node";

import type { BackoffOptions } from "./backoff.js";
import {
  ConnectionFactsRecorder,
  createDiagnosticAgent,
  makeConnectionDiagnosticsInterceptor,
  resolveTlsIdentity,
  type NodeTlsIdentity,
} from "./node-diagnostics.js";
import { indexGeneratedPolicy, type SdkTransportProfile } from "./policy.js";
import { makeRetryInterceptor } from "./retry.js";

export interface NodeConnectTransportOptions {
  baseUrl: string;
  /**
   * Accepted for symmetry with the browser adapter and validated, but a Node
   * transport is always the service profile in behaviour. Passing "webapp"
   * here is a composition-root mistake — a Node process cannot make a browser
   * request — so it is rejected rather than silently downgraded.
   */
  profile: SdkTransportProfile;
  /**
   * The generated policy document. Not used to select a verb here (a service
   * transport has no GET path), but parsed so a malformed or drifted artifact
   * fails at composition time in services too, rather than only in webapps.
   * That keeps one artifact honest across both runtimes instead of leaving
   * services free to ship a policy nothing ever reads.
   */
  policy?: unknown;
  /** Bearer token provider for service-to-service calls. */
  getToken?: () => string | null | undefined;
  /** Called when the server returns Unauthenticated. */
  onUnauthorized?: () => void;
  /** Binary protobuf instead of JSON. Default: false. */
  useBinaryFormat?: boolean;
  /** Retry options for transient unary failures. */
  retry?: BackoffOptions;
  /** Product interceptors, composed ahead of the core ones. */
  interceptors?: readonly Interceptor[];
  /** HTTP version. Default "2" — services talk h2 to the mesh. */
  httpVersion?: "1.1" | "2";
  /**
   * Client TLS identity for an mTLS peer.
   *
   * Not a knob — a mesh workload's identity is runtime state, and without a way
   * to carry it the canonical helper is unusable by exactly the services that
   * most need what it composes. That is the gap that left every mTLS caller
   * hand-rolling connect-node.
   *
   * Pass a function when the identity rotates (a SPIFFE SVID does). It is
   * called per connection, not per transport, so a rotating identity needs no
   * transport rebuild — which is what a caller doing this by hand has to do,
   * and doing so discards the socket pool on every call. Node keys its pool by
   * the resolved TLS material, so a rotation opens a fresh pool bucket on its
   * own and connections already established finish on the identity they
   * started with.
   */
  tls?: NodeTlsIdentity | (() => NodeTlsIdentity);
}

function makeAuthInterceptor(getToken: () => string | null | undefined): Interceptor {
  return (next) => (req) => {
    const token = getToken();
    if (token) {
      req.header.set("Authorization", `Bearer ${token}`);
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

/**
 * Build the canonical Connect transport for a Node service, CLI, or harness.
 *
 * Deliberately carries no browser machinery: no CSRF meta-tag read, no
 * `credentials: "include"`, no GET transport. Those are browser-only concerns,
 * and importing them into a service is how a CLI ends up depending on `document`.
 *
 * @example
 * ```ts
 * import { configureNodeConnectTransport } from "@brefwiz/api-bones-connect/node";
 * import policy from "./generated/connect-method-policy.json";
 *
 * const transport = configureNodeConnectTransport({
 *   baseUrl: process.env.SERVICE_URL!,
 *   profile: "service",
 *   policy,
 * });
 * ```
 */
export function configureNodeConnectTransport(opts: NodeConnectTransportOptions): Transport {
  const { baseUrl, profile, policy, getToken, onUnauthorized, useBinaryFormat, retry } = opts;

  if (profile !== "service") {
    throw new Error(
      `configureNodeConnectTransport: profile must be "service" (got "${profile}"). ` +
        "A Node process cannot issue browser requests, so the webapp profile has no " +
        "meaning here — use @brefwiz/api-bones-connect/web in a browser composition root.",
    );
  }

  // Parsed for its failure as well as its result: indexGeneratedPolicy fails
  // closed to an empty map, so an empty index alongside a non-empty document
  // means the artifact is malformed or has duplicate RPCs. Catching that here
  // stops a drifted policy from being noticed only once a webapp consumes it.
  //
  // The same index then decides retry eligibility, so a service that ships no
  // policy simply gets no retries rather than blind ones.
  const policyByRpc = indexGeneratedPolicy(policy);
  if (policy !== undefined) {
    const declared = (policy as { methods?: unknown })?.methods;
    if (Array.isArray(declared) && declared.length > 0 && policyByRpc.size === 0) {
      throw new Error(
        "configureNodeConnectTransport: connect-method-policy.json is malformed or has " +
          "duplicate RPC entries. Regenerate it (check-connect-read-profile.py --write) " +
          "rather than hand-editing.",
      );
    }
  }

  const httpVersion = opts.httpVersion ?? "2";

  // HTTP/1.1 pools sockets through an Agent, and a pooled socket the peer has
  // already closed is what turns a healthy call into `write EPIPE`. Own the
  // agent so the failure can name its own cause: a consumer that had to pass
  // one in would be carrying a knob for something the transport already knows.
  // HTTP/2 multiplexes over a session connect-node owns and fails with
  // GOAWAY/stream-closed, which already say why.
  const diagnostics = httpVersion === "1.1" ? new ConnectionFactsRecorder() : null;

  const interceptors: Interceptor[] = [
    ...(opts.interceptors ?? []),
    ...(getToken ? [makeAuthInterceptor(getToken)] : []),
    ...(onUnauthorized ? [makeUnauthInterceptor(onUnauthorized)] : []),
    // Inside the retry interceptor: a failure the policy lets us replay never
    // reaches a caller, so only the ones that actually surface get rewritten.
    makeRetryInterceptor({ policyByRpc, backoff: retry }),
    ...(diagnostics ? [makeConnectionDiagnosticsInterceptor(diagnostics)] : []),
  ];

  const common = {
    baseUrl,
    useBinaryFormat: useBinaryFormat ?? false,
    interceptors,
    acceptCompression: [compressionGzip, compressionBrotli],
  };

  // Spelled out per version rather than spread: ConnectTransportOptions is a
  // union discriminated on httpVersion, and each arm accepts a different
  // nodeOptions shape (Agent for h1, session options for h2).
  if (diagnostics !== null) {
    return createConnectTransport({
      ...common,
      httpVersion: "1.1",
      nodeOptions: {
        agent: createDiagnosticAgent(
          new URL(baseUrl).protocol === "https:",
          diagnostics,
          opts.tls,
        ),
      },
    });
  }
  // h2 resolves the identity once, at composition: connect-node owns the
  // session and there is no per-connection hook to re-resolve through. A
  // rotating identity therefore wants httpVersion "1.1" until that lands.
  return createConnectTransport({
    ...common,
    httpVersion: "2",
    ...(opts.tls ? { nodeOptions: resolveTlsIdentity(opts.tls) } : {}),
  });
}
