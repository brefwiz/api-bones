// SPDX-License-Identifier: MIT
import { create, fromBinary, toBinary, ScalarType, type DescField, type DescMessage, type DescMethodUnary, type DescService } from "@bufbuild/protobuf";
import { Code, ConnectError, type Client } from "@connectrpc/connect";
import { trace, SpanKind, SpanStatusCode } from "@opentelemetry/api";
import { assertSupportedPolicySchema, indexGeneratedPolicy, type GeneratedBatchPolicy, type GeneratedMethodPolicy, type GeneratedMethodPolicyDocument } from "./policy.js";
import { listCall, type ListCall } from "./page.js";
import { platformBinding, prepareHeaders, dispatchPrepared, captureSession, inheritPlatformBinding } from "./platform-client.js";
import { BatchScheduler } from "./batch-scheduler.js";
import { batchError, batchHash, captureBatchHeaders, checkBatchIdentity, itemErrorToConnect, planBatch, sameBatchKey, BATCH_MAX_ITEMS, BATCH_MAX_REQUEST_BYTES, type CapturedBatchHeaders, type CapturedBatchKey, type ItemError, type ItemResult, type BatchResults } from "./batch-primitives.js";
export { BATCH_MAX_ITEMS, BATCH_MAX_REQUEST_BYTES, allOk, authorizeBatchResults, serveBatchRequest, itemErrorToConnect, type ItemResult, type BatchResults } from "./batch-primitives.js";
export { MAX_WIRE_IN_FLIGHT, MAX_QUEUED_CHUNKS } from "./batch-scheduler.js";
export interface BatchCallOptions { signal?: AbortSignal; headers?: HeadersInit; timeoutMs?: number; }
export interface BatchGroup<T> { readonly parentId: string; readonly call?: ListCall<T>; readonly error?: ConnectError; }
type OutcomeItem<T> = T extends { outcome: infer O } ? O extends { case: "item"; value: infer V } ? V : never : never;
type BatchedMethod<F> = F extends (request: infer R, ...args: never[]) => Promise<infer O>
  ? O extends { results: readonly (infer E)[] } ? (ids: readonly string[], template?: Partial<R>, options?: BatchCallOptions) => Promise<BatchResults<OutcomeItem<E>>>
    : O extends { groups: readonly (infer G)[] } ? G extends { items: readonly (infer I)[] } ? (ids: readonly string[], template?: Partial<R>, options?: BatchCallOptions) => Promise<readonly BatchGroup<I>[]> : F : F
  : F;
export type BatchedClient<S extends DescService> = { [K in keyof Client<S>]: BatchedMethod<Client<S>[K]> };
type RecordValue = Record<string, unknown>;
interface Relation { single: DescMethodUnary; batch: DescMethodUnary; policy: GeneratedMethodPolicy; relation: GeneratedBatchPolicy; key: string; keys: string; results: string; resultKey: string; }
interface Prepared { effective: Headers; identity: string; headers: CapturedBatchHeaders; key: CapturedBatchKey; input: RecordValue; }
interface Waiter { id: string; relation: Relation; prepared: Promise<Prepared>; resolve(value: unknown): void; reject(reason: unknown): void; options: BatchCallOptions; detached: boolean; cleanup(): void; }
const record = (value: unknown): RecordValue => value as RecordValue;
const canceled = () => new ConnectError("batch cancelled", Code.Canceled);
function field(message: DescMessage, name: string): DescField {
  const result = message.fields.find(candidate => candidate.name === name || candidate.localName === name || candidate.jsonName === name);
  if (!result) throw new Error(`missing batch field ${message.typeName}.${name}`);
  return result;
}
function method(service: DescService, name: string): DescMethodUnary {
  const tail = name.slice(name.lastIndexOf("/") + 1).split(".").slice(-1)[0];
  const result = service.methods.find((candidate): candidate is DescMethodUnary => candidate.methodKind === "unary" && (candidate.name === tail || candidate.localName === tail));
  if (!result) throw new Error(`missing unary batch method ${name}`);
  return result;
}
function canonical(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonical);
  if (value instanceof Uint8Array || value === null || typeof value !== "object") return value;
  return Object.fromEntries(Object.keys(value).sort().map(key => [key, canonical(record(value)[key])]));
}
function encoded(schema: DescMessage, input: RecordValue): Uint8Array { return toBinary(schema, create(schema, canonical(input) as RecordValue)); }
/** One coalescer and fair wire scheduler per privately bound generated client. */
export function batchedClient<S extends DescService>(client: Client<S>, policy: GeneratedMethodPolicyDocument): BatchedClient<S> {
  const { service, transport, session } = platformBinding(client);
  const identityKey = () => captureSession(session).identityKey;
  assertSupportedPolicySchema(policy);
  const policies = indexGeneratedPolicy(policy);
  if (service.methods.length && !policies.size) throw new Error("empty batch policy discovery");
  const scheduler = new BatchScheduler();
  const wrapped = { ...client } as Record<string, unknown>;
  const relations: Relation[] = [];
  for (const entry of policies.values()) {
    if (!entry.batch || !entry.rpc.startsWith(`/${service.typeName}/`)) continue;
    const single = method(service, entry.rpc), batch = method(service, entry.batch.method);
    const keyField = field(single.input, entry.batch.keyField);
    const keysField = field(batch.input, entry.batch.keysField);
    if (keyField.fieldKind !== "scalar" || keyField.scalar !== ScalarType.STRING || keysField.fieldKind !== "list" || keysField.listKind !== "scalar" || keysField.scalar !== ScalarType.STRING) throw new Error("batch keys must be string fields");
    const key = keyField.localName;
    const keys = keysField.localName;
    const resultsField = field(batch.output, entry.batch.resultsField);
    if (resultsField.fieldKind !== "list" || resultsField.listKind !== "message") throw new Error("batch results must be repeated messages");
    const resultKeyField = field(resultsField.message, entry.batch.kind === "get" ? "id" : entry.batch.keyField);
    if (resultKeyField.fieldKind !== "scalar" || resultKeyField.scalar !== ScalarType.STRING) throw new Error("batch result key must be a string");
    const resultKey = resultKeyField.localName;
    const errorField = field(resultsField.message, "error");
    if (errorField.fieldKind !== "message" || errorField.message.typeName !== "bones.v1.ItemError") throw new Error("batch result error must be bones.v1.ItemError");
    if (entry.batch.kind === "get") {
      const itemField = field(resultsField.message, "item");
      if (itemField.fieldKind !== "message" || itemField.message.typeName !== single.output.typeName || !itemField.oneof || itemField.oneof !== errorField.oneof) throw new Error("batch item must match single response in an outcome oneof");
    } else {
      const items = field(resultsField.message, "items");
      if (items.fieldKind !== "list") throw new Error("batch groups must contain repeated items");
      const singleItems = field(single.output, entry.paging?.itemField ?? "items");
      const matching = singleItems.fieldKind === "list" && (
        items.listKind === "message" && singleItems.listKind === "message" ? items.message.typeName === singleItems.message.typeName :
        items.listKind === "enum" && singleItems.listKind === "enum" ? items.enum.typeName === singleItems.enum.typeName :
        items.listKind === "scalar" && singleItems.listKind === "scalar" && items.scalar === singleItems.scalar
      );
      if (!matching) throw new Error("batch group items must match single List response");
    }
    const declared = policies.get(`/${service.typeName}/${batch.name}`);
    if (declared?.idempotency !== "NO_SIDE_EFFECTS") throw new Error(`batch method ${batch.name} must declare NO_SIDE_EFFECTS`);
    relations.push({ single, batch, policy: entry, relation: entry.batch, key, keys, results: resultsField.localName, resultKey });
  }
  async function prepare(relation: Relation, input: RecordValue, options: BatchCallOptions, single: boolean): Promise<Prepared> {
    const capturedSession = captureSession(session);
    const identity = capturedSession.identityKey;
    const descriptor = single ? relation.single : relation.batch;
    const snapshot = record(fromBinary(descriptor.input, encoded(descriptor.input, input)));
    const effective = await prepareHeaders(transport, descriptor, snapshot, options.headers, options.timeoutMs, options.signal, () => checkBatchIdentity(identityKey, identity), capturedSession);
    checkBatchIdentity(identityKey, identity);
    const headers = await captureBatchHeaders(effective, options.timeoutMs);
    const cleared = { ...snapshot, [single ? relation.key : relation.keys]: single ? "" : [] };
    const requestHash = await batchHash(encoded(descriptor.input, cleared));
    return { effective, identity, headers, input: snapshot, key: { method: `${service.typeName}/${relation.single.name}`, requestHash, headersHash: headers.headersHash, identity } };
  }
  async function send(relation: Relation, prepared: Prepared, ids: readonly string[], template: RecordValue, options: BatchCallOptions, calls: number, requested: number, chunkIndex: number, chunks: number, single = false): Promise<unknown> {
    const descriptor = single ? relation.single : relation.batch;
    const input = { ...template, [single ? relation.key : relation.keys]: single ? ids[0] : [...ids] };
    const enqueued = performance.now();
    return scheduler.submit({ caller: prepared.identity, credentialHash: prepared.headers.credentialHash }, async () => {
      const attributes = { "rpc.method": `${service.typeName}/${descriptor.name}`, "bones.batch.single_method": `${service.typeName}/${relation.single.name}`, "bones.batch.requested": requested, "bones.batch.items": ids.length, "bones.batch.calls": calls, "bones.batch.chunk_index": chunkIndex, "bones.batch.chunks": chunks, "bones.batch.bytes": encoded(descriptor.input, input).byteLength, "bones.batch.wait_ms": performance.now() - enqueued };
      const span = single ? undefined : trace.getTracer("@brefwiz/api-bones-connect").startSpan("bones.batch", { kind: SpanKind.CLIENT, attributes });
      let outcome = "ok", sent = false, errors = 0;
      try {
        checkBatchIdentity(identityKey, prepared.identity);
        if (options.signal?.aborted) throw canceled();
        sent = true;
        const response = await dispatchPrepared(transport, descriptor, input, new Headers(prepared.headers.headers), () => checkBatchIdentity(identityKey, prepared.identity), options.signal, options.timeoutMs, prepared.effective);
        if (!single) {
          const results = record(response)[relation.results];
          if (!Array.isArray(results) || results.length !== ids.length || results.some((result, index) => record(result)[relation.resultKey] !== ids[index])) {
            outcome = "mismatch";
            throw batchError(Code.Internal, "BATCH_RESPONSE_MISMATCH", "batch response does not match requested ids");
          }
          errors = results.filter(result => relation.relation.kind === "get" ? record(record(result).outcome ?? {}).case === "error" : record(result).error !== undefined).length;
        }
        return response;
      } catch (reason) {
        const error = ConnectError.from(reason);
        if (outcome !== "mismatch") outcome = error.code === Code.Unauthenticated ? "identity_changed" : error.code === Code.Canceled ? "cancelled" : "error";
        span?.setStatus({ code: SpanStatusCode.ERROR, message: error.message });
        throw error;
      } finally {
        if (span) {
          const completion = { ...attributes, "bones.batch.item_errors": errors, "bones.batch.outcome": outcome, "bones.batch.sent": sent };
          span.setAttributes(completion); span.addEvent("bones.batch", completion); span.end();
          console.info(JSON.stringify({ target: "bones.batch", ...completion }));
        }
      }
    }, options.signal);
  }
  function item(result: unknown): ItemResult<unknown> {
    const outcome = record(record(result).outcome ?? {});
    if (outcome?.case === "item") return { ok: true, value: outcome.value };
    if (outcome?.case === "error") return { ok: false, error: itemErrorToConnect(outcome.value as ItemError) };
    return { ok: false, error: batchError(Code.Internal, "BATCH_RESPONSE_MISMATCH", "batch result has no outcome") };
  }
  async function explicit(relation: Relation, ids: readonly string[], template: RecordValue = {}, options: BatchCallOptions = {}): Promise<unknown[]> {
    if (!ids.length) return [];
    const output: ItemResult<unknown>[] = Array.from({ length: ids.length });
    let prepared: Prepared;
    try { prepared = await prepare(relation, { ...template, [relation.keys]: [...ids] }, options, false); }
    catch (reason) { if (options.signal?.aborted) throw canceled(); const error = ConnectError.from(reason); return ids.map(parentId => relation.relation.kind === "get" ? { ok: false, error } : { parentId, error }); }
    const input = prepared.input;
    const plan = planBatch(ids, keys => encoded(relation.batch.input, { ...input, [relation.keys]: [...keys] }));
    const assign = (id: string, value: ItemResult<unknown>) => { for (const position of plan.positions.get(id)!) output[position] = value; };
    for (const [id, error] of plan.rejected) assign(id, { ok: false, error });
    for (const [index, chunk] of plan.chunks.entries()) {
      if (options.signal?.aborted) throw canceled();
      try {
        const response = await send(relation, prepared, chunk, input, options, 1, ids.length, index, plan.chunks.length);
        const values = record(response)[relation.results] as unknown[];
        values.forEach((value, i) => assign(chunk[i], relation.relation.kind === "get" ? item(value) : { ok: true, value }));
      } catch (reason) { if (options.signal?.aborted) throw canceled(); const error = ConnectError.from(reason); chunk.forEach(id => assign(id, { ok: false, error })); }
    }
    if (relation.relation.kind === "get") return output;
    return ids.map((parentId, index): BatchGroup<unknown> => {
      const result = output[index];
      if (!result.ok) return { parentId, error: result.error };
      const group = record(result.value);
      if (group.error) return { parentId, error: itemErrorToConnect(group.error as ItemError) };
      const paging = relation.policy.paging ?? { kind: "unary" as const, itemField: "items", pageField: "page", requestPageField: "page" };
      const metadata = record(group.page ?? {});
      const request = { ...template, [relation.key]: parentId };
      delete request[relation.keys];
      const call = listCall<unknown>((next, callOptions) => {
        const headers = new Headers(prepared.headers.headers);
        headers.delete("bones-list-cache"); headers.delete("if-none-match");
        return dispatchPrepared(transport, relation.single, next, headers, () => checkBatchIdentity(identityKey, prepared.identity), callOptions.signal, options.timeoutMs, prepared.effective);
      }, request, {
        method: `${service.typeName}/${relation.single.name}`, paging,
        initialPage: { items: (group.items ?? []) as unknown[], revision: String(metadata.revision ?? ""), fromCache: false }, nextCursor: String(metadata.nextCursor ?? ""),
      }, { signal: options.signal, cache: "bypass" });
      return { parentId, call };
    });
  }
  let pending: Waiter[] = [];
  let scheduled = false;
  async function flush(waiters: Waiter[]): Promise<void> {
    const groups: { relation: Relation; prepared: Prepared; waiters: Waiter[] }[] = [];
    const preparations = await Promise.all(waiters.map(async waiter => {
      try { return await waiter.prepared; }
      catch (reason) { if (!waiter.detached) waiter.reject(ConnectError.from(reason)); waiter.cleanup(); return undefined; }
    }));
    waiters.forEach((waiter, index) => {
      const prepared = preparations[index];
      if (!prepared || waiter.detached) return;
      const group = groups.find(candidate => sameBatchKey(candidate.prepared.key, prepared.key));
      if (group) group.waiters.push(waiter); else groups.push({ relation: waiter.relation, prepared, waiters: [waiter] });
    });
    await Promise.all(groups.map(async group => {
      const { relation, prepared } = group;
      const template = { ...prepared.input }; delete template[relation.key];
      const active = group.waiters.filter(waiter => !waiter.detached);
      const ids = active.map(waiter => waiter.id);
      const plan = planBatch(ids, keys => encoded(relation.batch.input, { ...template, [relation.keys]: [...keys] }));
      for (const [id, error] of plan.rejected) for (const waiter of active.filter(waiter => waiter.id === id)) { waiter.reject(error); waiter.cleanup(); }
      await Promise.all(plan.chunks.map(async (chunk, index) => {
        const callers = active.filter(waiter => chunk.includes(waiter.id));
        const controller = new AbortController();
        const detach = () => { if (callers.every(waiter => waiter.detached)) controller.abort(); };
        callers.forEach(waiter => waiter.options.signal?.addEventListener("abort", detach, { once: true }));
        detach();
        try {
          const lone = chunk.length === 1;
          const response = await send(relation, prepared, chunk, lone ? prepared.input : template, { ...callers[0].options, signal: controller.signal }, callers.length, ids.length, index, plan.chunks.length, lone);
          const results = lone ? [{ ok: true, value: response } as ItemResult<unknown>] : (record(response)[relation.results] as unknown[]).map(item);
          for (const waiter of callers) if (!waiter.detached) {
            const result = results[chunk.indexOf(waiter.id)];
            if (result.ok) waiter.resolve(result.value); else waiter.reject(result.error);
          }
        } catch (reason) { callers.forEach(waiter => { if (!waiter.detached) waiter.reject(ConnectError.from(reason)); }); }
        finally { callers.forEach(waiter => { waiter.cleanup(); waiter.options.signal?.removeEventListener("abort", detach); }); }
      }));
    }));
  }
  for (const relation of relations) {
    wrapped[relation.batch.localName] = (ids: readonly string[], template?: RecordValue, options?: BatchCallOptions) => explicit(relation, ids, template, options);
    if (relation.relation.kind !== "get") continue;
    wrapped[relation.single.localName] = (request: RecordValue, options: BatchCallOptions = {}) => new Promise((resolve, reject) => {
      if (options.signal?.aborted) { reject(canceled()); return; }
      const waiter: Waiter = { id: String(request[relation.key] ?? ""), relation, prepared: prepare(relation, request, options, true), resolve, reject, options, detached: false, cleanup: () => options.signal?.removeEventListener("abort", abort) };
      const abort = () => { waiter.detached = true; reject(canceled()); };
      options.signal?.addEventListener("abort", abort, { once: true });
      pending.push(waiter);
      if (!scheduled) {
        scheduled = true;
        queueMicrotask(() => { const window = pending; pending = []; scheduled = false; void flush(window); });
      }
    });
  }
  inheritPlatformBinding(client, wrapped);
  return wrapped as BatchedClient<S>;
}
