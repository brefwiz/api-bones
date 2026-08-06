// SPDX-License-Identifier: MIT
//
// Per-call auth scoping (ADR platform/0269).
//
// A client configured with a static credential — a default `Authorization`
// header, or a callback the transport invokes with no argument — carries ONE
// identity for the whole process. That is right for a service presenting its
// own rotating token, and wrong for any caller whose credential varies per
// call, which is then left with a single option: park the credential somewhere
// the callback can reach and hope no concurrent call observes it.
//
// It does not hold. quorumauth's sealwiz namespace provisioner held a per-org
// claim in one shared cell, with a comment explaining why that was safe —
// true while the claim named no org, false the moment it had to. Concurrent
// org creations then presented each other's claims and sealwiz refused 1892
// per run, naming neither operand, which hid the cause for two investigation
// cycles.
//
// Connect already threads `contextValues` through every call, so the
// credential can ride the request that needs it. Bind it at the call site:
//
//   import { CALL_CREDENTIAL, callScopedAuthInterceptor } from
//     "@brefwiz/api-bones-connect/auth";
//
//   const transport = configureConnectTransport({
//     baseUrl,
//     interceptors: [callScopedAuthInterceptor()],
//   });
//
//   await client.createNamespace(req, {
//     contextValues: createContextValues().set(CALL_CREDENTIAL, claim),
//   });
//
// One transport, one connection pool, no per-call TLS work — and a credential
// that is visible to exactly the call it was bound to. Two concurrent calls
// carrying different credentials cannot observe each other's, because neither
// is stored anywhere both can read.

import { createContextKey, type Interceptor } from "@connectrpc/connect";

/**
 * The credential for a single call.
 *
 * Defaults to empty, meaning "this call supplies none" — the interceptor then
 * leaves whatever the transport was configured with untouched, so a client
 * presenting a process-wide service identity keeps working unchanged.
 */
export const CALL_CREDENTIAL = createContextKey<string>("", {
  description: "Bearer credential scoped to a single Connect call",
});

/** Options for {@link callScopedAuthInterceptor}. */
export interface CallScopedAuthOptions {
  /**
   * Header to carry the credential. Defaults to `Authorization`.
   */
  readonly header?: string;
  /**
   * Scheme prefix. Defaults to `Bearer`; pass an empty string to send the
   * credential verbatim.
   */
  readonly scheme?: string;
}

/**
 * Apply the calling context's credential to the outgoing request.
 *
 * Absent a call-scoped credential this is a no-op: the request goes out with
 * whatever the transport already set, so adding the interceptor cannot break a
 * client that authenticates process-wide. A call that DOES bind one overrides
 * the default for that request alone.
 */
export function callScopedAuthInterceptor(
  options: CallScopedAuthOptions = {},
): Interceptor {
  const header = options.header ?? "Authorization";
  const scheme = options.scheme ?? "Bearer";
  return (next) => async (req) => {
    const credential = req.contextValues.get(CALL_CREDENTIAL);
    if (credential !== "") {
      req.header.set(header, scheme === "" ? credential : `${scheme} ${credential}`);
    }
    return next(req);
  };
}
