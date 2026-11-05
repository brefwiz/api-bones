// SPDX-License-Identifier: MIT
import type { DescService, DescMethodUnary } from "@bufbuild/protobuf";
import { Code, ConnectError, createClient, createContextKey, createContextValues, type Client, type Interceptor, type Transport } from "@connectrpc/connect";

import { identityChanged } from "./batch-primitives.js";

export interface SessionSnapshot { readonly identityKey: string; readonly token?: string | null; readonly relayedCredential?: string; }
export interface SessionSource { snapshot(): SessionSnapshot; }
export const anonymousSession: SessionSource = Object.freeze({ snapshot: (): SessionSnapshot => ({ identityKey: "anonymous" }) });
interface PreparedCredentials { readonly identityKey?: string; readonly bearer: boolean; readonly relayed: boolean; }
interface Preparation { readonly phase: "prepare" | "dispatch"; headers?: Headers; guard?: () => void; identityKey?: string; credentials?: PreparedCredentials; }
const credentialOwnership = new WeakMap<Headers, PreparedCredentials>();
const preparation = createContextKey<Preparation | undefined>(undefined);
const transports = new WeakMap<Transport, { session?: SessionSource }>();
const bindings = new WeakMap<object, { service: DescService; transport: Transport; session?: SessionSource }>();
const captured = new ConnectError("batch headers prepared", Code.Canceled);
/** Platform transport adapters alone install preparation support. */
export function preparedTransport<T extends Transport>(transport: T, session?: SessionSource): T { transports.set(transport, { session }); return transport; }
function snapshot(source: SessionSource, state?: Preparation): SessionSnapshot {
  try {
    const { identityKey, token, relayedCredential } = source.snapshot();
    if (typeof identityKey !== "string" || !identityKey || (state?.identityKey !== undefined && identityKey !== state.identityKey)) throw identityChanged();
    if ((token !== null && token !== undefined && (typeof token !== "string" || !token)) ||
      (relayedCredential !== undefined && (typeof relayedCredential !== "string" || !relayedCredential)) ||
      (state?.credentials?.bearer && !token) || (state?.credentials?.relayed && !relayedCredential)) throw identityChanged();
    const headers = new Headers();
    if (token) { const value = `Bearer ${token}`; headers.set("authorization", value); if (headers.get("authorization") !== value) throw identityChanged(); }
    if (relayedCredential) { headers.set("brefwiz-relayed-credential", relayedCredential); if (headers.get("brefwiz-relayed-credential") !== relayedCredential) throw identityChanged(); }
    return { identityKey, token, relayedCredential };
  } catch { throw identityChanged(); }
}
export function captureSession(source: SessionSource): SessionSnapshot { return snapshot(source); }
function attach(headers: Headers, current: SessionSnapshot): void {
  headers.delete("authorization"); headers.delete("brefwiz-relayed-credential");
  if (current.token) headers.set("authorization", `Bearer ${current.token}`);
  if (current.relayedCredential) headers.set("brefwiz-relayed-credential", current.relayedCredential);
}
/** Metadata producers run once, before requests enter the batch queue. */
export function preparationMetadata(interceptor: Interceptor): Interceptor {
  return next => {
    const run = interceptor(next);
    return req => req.contextValues.get(preparation)?.phase === "dispatch" ? next(req) : run(req);
  };
}
export const preparationBoundary: Interceptor = next => async req => {
  const state = req.contextValues.get(preparation);
  if (state?.phase === "prepare") { state.headers = new Headers(req.header); throw captured; }
  return next(req);
};
/** Credential source is evaluated with the same session identity guard. */
export function preparationAuth(getToken: () => string | null | undefined): Interceptor {
  return next => req => {
    const state = req.contextValues.get(preparation);
    if (state?.phase === "dispatch") return next(req);
    state?.guard?.();
    let token: string | null | undefined;
    try { token = getToken(); } catch (error) { if (state) throw identityChanged(); throw error; }
    state?.guard?.();
    if (state) state.credentials = { bearer: Boolean(token), relayed: false };
    if (token) req.header.set("authorization", `Bearer ${token}`);
    return next(req);
  };
}
/** One producer snapshot owns both identity and transport credentials. */
export function preparationSession(source: SessionSource): Interceptor {
  return next => req => {
    const state = req.contextValues.get(preparation);
    if (state?.phase === "dispatch") return next(req);
    if (req.header.has("authorization") || req.header.has("brefwiz-relayed-credential")) throw new ConnectError("credentials must come from the bound session source", Code.InvalidArgument);
    const current = snapshot(source, state);
    if (state) state.credentials = { identityKey: current.identityKey, bearer: Boolean(current.token), relayed: Boolean(current.relayedCredential) };
    attach(req.header, current);
    return next(req);
  };
}
/** Inside retry: source-owned credentials refresh under the captured session. */
export function preparationAttempt(getToken?: () => string | null | undefined, source?: SessionSource): Interceptor {
  return next => async req => {
    const state = req.contextValues.get(preparation);
    if (state?.phase === "dispatch") {
      state.guard?.();
      if (source) {
        const current = snapshot(source, state);
        attach(req.header, current);
      } else if (getToken && state.credentials?.bearer) {
        let token: string | null | undefined;
        try { token = getToken(); } catch { throw identityChanged(); }
        state.guard?.();
        if (!token) throw identityChanged();
        req.header.set("authorization", `Bearer ${token}`);
      }
    }
    const response = await next(req);
    if (state?.phase === "dispatch") { if (source) snapshot(source, state); state.guard?.(); }
    return response;
  };
}
/** Shared SDK composition constructor; descriptor and transport stay private. */
export function createPlatformClient<S extends DescService>(service: S, transport: Transport): Client<S> {
  const prepared = transports.get(transport);
  if (!prepared) throw new Error("platform client requires a prepared platform transport");
  const client = createClient(service, transport);
  bindings.set(client, { service, transport, session: prepared.session });
  return client;
}
export function platformBinding(client: object): { service: DescService; transport: Transport; session: SessionSource } {
  const binding = bindings.get(client);
  if (!binding) throw new Error("batching requires createPlatformClient with a prepared platform transport");
  if (!binding.session) throw new Error("batching requires a producer-owned session binding");
  return { ...binding, session: binding.session };
}
export async function prepareHeaders(transport: Transport, method: DescMethodUnary, input: Record<string, unknown>, headers?: HeadersInit, timeoutMs?: number, signal?: AbortSignal, guard?: () => void, capturedSession?: SessionSnapshot): Promise<Headers> {
  const supplied = new Headers(headers);
  if (supplied.has("authorization") || supplied.has("brefwiz-relayed-credential")) throw new ConnectError("batch credentials must come from the bound session source", Code.InvalidArgument);
  const credentials = capturedSession ? { identityKey: capturedSession.identityKey, bearer: Boolean(capturedSession.token), relayed: Boolean(capturedSession.relayedCredential) } : undefined;
  const state: Preparation = { phase: "prepare", guard, identityKey: capturedSession?.identityKey, credentials };
  try { await transport.unary(method, signal, timeoutMs, headers, input, createContextValues().set(preparation, state)); }
  catch (error) { if (error !== captured) throw error; }
  if (!state.headers) throw new Error("transport did not prepare effective batch headers");
  credentialOwnership.set(state.headers, state.credentials ?? { identityKey: capturedSession?.identityKey, bearer: false, relayed: false });
  return state.headers;
}
export async function dispatchPrepared(transport: Transport, method: DescMethodUnary, input: Record<string, unknown>, headers: HeadersInit, guard: () => void, signal?: AbortSignal, timeoutMs?: number, preparedHeaders?: Headers): Promise<unknown> {
  guard();
  const credentials = preparedHeaders ? credentialOwnership.get(preparedHeaders) : undefined;
  const response = await transport.unary(method, signal, timeoutMs, headers, input, createContextValues().set(preparation, { phase: "dispatch", guard, credentials, identityKey: credentials?.identityKey }));
  guard();
  return response.message;
}
