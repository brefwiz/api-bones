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
import { startWatcherSafe } from "@brefwiz/spiffe-client";
import { clientTlsIdentityFor, WATCHER_ATTEMPTS, WorkloadIdentityError } from "./workload-identity.js";

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
  /**
   * Wire encoding. Defaults to binary protobuf, which is the platform
   * default on both hops — see `rpc_protocols` in SPEC.md.
   *
   * Set false for JSON when a body has to be readable in a proxy or a log.
   * A Connect server built from the same protos serves either, so this is a
   * debugging choice rather than a compatibility one.
   */
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
  /**
   * Supplying this at all is the outside-the-mesh case. An in-fleet caller
   * omits it and gets its identity from the Workload API — naming a cert, key
   * or CA here is the consumer assembling trust, which is what this transport
   * exists to stop.
   */
  tls?: NodeTlsIdentity | (() => NodeTlsIdentity);
  /**
   * Per-call deadline in milliseconds, applied when the caller sets none.
   *
   * A transport with no default deadline waits forever on a peer that accepts
   * the connection and then says nothing — the failure mode a tier-zero
   * bootstrap call cannot afford, since nothing downstream of it has started
   * yet to notice.
   */
  defaultTimeoutMs?: number;
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
export async function configureNodeConnectTransport(
  opts: NodeConnectTransportOptions,
): Promise<Transport> {
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
    useBinaryFormat: useBinaryFormat ?? true,
    interceptors,
    acceptCompression: [compressionGzip, compressionBrotli],
    ...(opts.defaultTimeoutMs === undefined
      ? {}
      : { defaultTimeoutMs: opts.defaultTimeoutMs }),
  };

  // Spelled out per version rather than spread: ConnectTransportOptions is a
  // union discriminated on httpVersion, and each arm accepts a different
  // nodeOptions shape (Agent for h1, session options for h2).
  // An https:// peer is an in-fleet peer unless the caller said otherwise by
  // passing `tls`. Resolving the watcher here — once per transport — keeps the
  // per-connection hook below synchronous, so a rotating SVID still takes
  // effect on the next connection without rebuilding the transport.
  const isTls = new URL(baseUrl).protocol === "https:";
  let tlsSource: NodeTlsIdentity | (() => NodeTlsIdentity) | undefined = opts.tls;

  if (tlsSource === undefined && isTls) {
    const watcher = await startWatcherSafe(undefined, WATCHER_ATTEMPTS);
    if (watcher === null) {
      throw new WorkloadIdentityError(
        `SPIFFE Workload API unavailable after ${WATCHER_ATTEMPTS} attempts; ` +
          `cannot open an in-fleet transport to ${baseUrl}. Pass \`tls\` explicitly ` +
          `if this caller is outside the mesh.`,
        "workload_api_unavailable",
      );
    }
    tlsSource = () => clientTlsIdentityFor(watcher);
  }

  if (diagnostics !== null) {
    return createConnectTransport({
      ...common,
      httpVersion: "1.1",
      nodeOptions: {
        agent: createDiagnosticAgent(isTls, diagnostics, tlsSource),
      },
    });
  }
  // h2 resolves the identity once, at composition: connect-node owns the
  // session and there is no per-connection hook to re-resolve through. A
  // rotating identity therefore wants httpVersion "1.1" until that lands.
  return createConnectTransport({
    ...common,
    httpVersion: "2",
    ...(tlsSource ? { nodeOptions: resolveTlsIdentity(tlsSource) } : {}),
  });
}
