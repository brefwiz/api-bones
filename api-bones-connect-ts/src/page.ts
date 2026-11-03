// SPDX-License-Identifier: MIT
import type { DescService } from "@bufbuild/protobuf";
import { Code, ConnectError, type CallOptions, type Client } from "@connectrpc/connect";
import { trace, SpanKind, SpanStatusCode, type Span } from "@opentelemetry/api";
import { BonesError } from "./error-info.js";
import { assertSupportedPolicySchema, indexGeneratedPolicy, type GeneratedMethodPolicyDocument, type GeneratedPagingPolicy } from "./policy.js";
import { RevisionCache } from "./revision-cache.js";
import { platformBinding } from "./platform-client.js";

export interface PageOptions {
  pageSize?: number;
  prefetch?: 0 | 1;
  signal?: AbortSignal;
  cache?: "use" | "bypass";
}
export interface Total { readonly value: bigint; readonly kind: "estimate" | "exact"; }
export interface Page<T> {
  readonly items: readonly T[];
  readonly revision: string;
  readonly total?: Total;
  readonly fromCache: boolean;
}
export type PageErrorKind = "cursor_invalid" | "cursor_expired" | "cursor_query_mismatch" | "rpc";
export class PageError extends BonesError {
  override name = "PageError";
  readonly kind: PageErrorKind;
  constructor(cause: ConnectError, readonly deliveredPages: number, readonly deliveredItems: number) {
    super(cause);
    this.kind = this.code === "CURSOR_INVALID" ? "cursor_invalid" : this.code === "CURSOR_EXPIRED" ? "cursor_expired" : this.code === "CURSOR_QUERY_MISMATCH" ? "cursor_query_mismatch" : "rpc";
  }
}
export function isCursorRejected(error: unknown): boolean {
  return error instanceof PageError && error.kind !== "rpc";
}
export interface ListCall<T> {
  pages(): AsyncIterable<Page<T>>;
  items(): AsyncIterable<T>;
  firstPage(): Promise<Page<T>>;
  /** Active iteration span, usable by the UI for first-row measurements. */
  readonly span?: Span;
}

export type PageFetch = (request: Record<string, unknown>, options: CallOptions) => Promise<unknown>;
export interface ListConfig<T> {
  method: string;
  paging: GeneratedPagingPolicy;
  cache?: RevisionCache;
  initialPage?: Page<T>;
  nextCursor?: string;
}

function canonical(value: unknown): string {
  if (typeof value === "bigint") return JSON.stringify({ bigint: String(value) });
  if (value instanceof Uint8Array) return JSON.stringify({ bytes: Array.from(value) });
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (typeof value === "object" && value !== null) {
    const object = value as Record<string, unknown>;
    return `{${Object.keys(object).filter((key) => object[key] !== undefined && key !== "$typeName").sort().map((key) => `${JSON.stringify(key)}:${canonical(object[key])}`).join(",")}}`;
  }
  return JSON.stringify(value) ?? "null";
}
async function requestHash(request: Record<string, unknown>): Promise<string> {
  const bytes = new TextEncoder().encode(canonical(request));
  return Array.from(new Uint8Array(await globalThis.crypto.subtle.digest("SHA-256", bytes)), (byte) => byte.toString(16).padStart(2, "0")).join("");
}
function record(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null) throw new ConnectError("invalid list response", Code.Internal);
  return value as Record<string, unknown>;
}

/** Bind one generated unary method; transport owns retry policy. */
export function listCall<T>(fetchPage: PageFetch, request: Record<string, unknown>, config: ListConfig<T>, options: PageOptions = {}): ListCall<T> {
  let activeSpan: Span | undefined;
  async function* iterate(prefetch: 0 | 1, retain: boolean): AsyncGenerator<Page<T>> {
    const controller = new AbortController();
    const abort = () => controller.abort(options.signal?.reason);
    options.signal?.addEventListener("abort", abort, { once: true });
    if (options.signal?.aborted) abort();
    const started = performance.now();
    const span = trace.getTracer("@brefwiz/api-bones-connect").startSpan("bones.list", { kind: SpanKind.CLIENT, attributes: { "rpc.method": config.method } });
    activeSpan = span;
    let count = 0, itemCount = 0, hits = 0, modifiedHits = 0;
    let ended = false;
    let failed = false;
    let prefetched = false;
    let firstBatchMs: number | undefined;
    let pending: Promise<{ value?: unknown; error?: unknown; notModified: boolean }> | undefined;
    const pageField = config.paging.requestPageField;
    const inputPage = record(request[pageField] ?? {});
    const pageSize = options.pageSize ?? inputPage.pageSize;
    const basePage = { ...inputPage, cursor: "", pageSize: pageSize ?? 0 };
    const base = { ...request, [pageField]: basePage };
    let key: string | undefined;
    const cache = options.cache === "bypass" || config.initialPage || inputPage.cursor ? undefined : config.cache;
    const identityCurrent = () => key !== undefined && cache?.key(config.method, hash) === key;
    let hash = "";
    const delivered = (page: Page<T>) => {
      count++;
      if (count === 1 && config.paging.kind === "stream") {
        firstBatchMs = performance.now() - started;
        span.setAttribute("bones.list.time_to_first_batch_ms", firstBatchMs);
      }
      itemCount += page.items.length;
      if (itemCount > 0 && !firstItem) {
        firstItem = true;
        firstItemMs = performance.now() - started;
        span.setAttribute("bones.list.time_to_first_item_ms", firstItemMs);
      }
    };
    let firstItem = false;
    let firstItemMs: number | undefined;
    const fetch = (cursor: string, revision?: string) => {
      let notModified = false;
      const headers = new Headers({ "bones-list-cache": "1" });
      if (revision) headers.set("if-none-match", `"${revision}"`);
      return Promise.resolve().then(() => {
        if (controller.signal.aborted) throw new ConnectError("list cancelled", Code.Canceled);
        return fetchPage({ ...base, [pageField]: { ...basePage, cursor } }, {
          signal: controller.signal, headers,
          onHeader(header) { notModified = header.get("bones-not-modified") === "1"; },
        });
      }).then((value) => ({ value, notModified }), (error: unknown) => ({ error, notModified }));
    };
    try {
      hash = await requestHash(base);
      key = cache?.key(config.method, hash);
      const cached = key && !inputPage.cursor && !config.initialPage ? cache?.replay<T>(key) : undefined;
      if (key) cache?.begin(key);
      let cursor = config.nextCursor ?? String(inputPage.cursor ?? "");
      let seed = config.initialPage;
      if (!seed) pending = fetch(cursor, cached?.[0]?.revision);
      for (;;) {
        if (controller.signal.aborted) throw new ConnectError("list cancelled", Code.Canceled);
        let page: Page<T>;
        if (seed) { page = seed; seed = undefined; }
        else {
          if (prefetched) { hits++; prefetched = false; }
          const response = await pending!;
          pending = undefined;
          if (key && !identityCurrent()) throw new ConnectError("list cache identity changed", Code.Unauthenticated);
          if (response.error !== undefined) throw response.error;
          if (response.notModified) {
            if (count !== 0 || !cached || !identityCurrent()) throw new ConnectError("not-modified without a replayable list", Code.Internal);
            modifiedHits++;
            for (const original of cached) {
              if (controller.signal.aborted) throw new ConnectError("list cancelled", Code.Canceled);
              if (!identityCurrent()) throw new ConnectError("list cache identity changed", Code.Unauthenticated);
              page = { ...original, fromCache: true };
              if (key && retain) cache?.keep(key, page, original === cached[cached.length - 1]);
              delivered(page);
              yield page;
            }
            ended = true;
            return;
          }
          const responseValue = record(response.value);
          const metadata = record(responseValue[config.paging.pageField] ?? {});
          const values = responseValue[config.paging.itemField];
          if (!Array.isArray(values)) throw new ConnectError("list response has no items", Code.Internal);
          const total = metadata.total === undefined ? undefined : record(metadata.total);
          page = {
            items: values as T[], revision: String(metadata.revision ?? ""), fromCache: false,
            ...(total && (total.kind === 1 || total.kind === 2) ? { total: { value: BigInt(total.value as bigint), kind: total.kind === 1 ? "estimate" as const : "exact" as const } } : {}),
          };
          cursor = String(metadata.nextCursor ?? "");
        }
        if (key && retain && identityCurrent()) cache?.keep(key, page, cursor === "");
        if (cursor && prefetch) { pending = fetch(cursor); prefetched = true; }
        delivered(page);
        yield page;
        if (!cursor) { ended = true; return; }
        if (!pending) pending = fetch(cursor);
      }
    } catch (reason) {
      const cause = ConnectError.from(reason);
      failed = cause.code !== Code.Canceled;
      if (cause.code === Code.Unauthenticated) config.cache?.clear();
      span.setStatus({ code: SpanStatusCode.ERROR, message: cause.message });
      throw new PageError(cause, count, itemCount);
    } finally {
      controller.abort();
      options.signal?.removeEventListener("abort", abort);
      const attributes = {
        "rpc.method": config.method,
        "bones.list.pages": count,
        "bones.list.prefetch_hits": hits,
        "bones.list.not_modified_hits": modifiedHits,
        "bones.list.cancelled": !ended && !failed,
        ...(firstItemMs === undefined ? {} : { "bones.list.time_to_first_item_ms": firstItemMs }),
        ...(firstBatchMs === undefined ? {} : { "bones.list.time_to_first_batch_ms": firstBatchMs }),
        ...(ended ? { "bones.list.time_to_full_list_ms": performance.now() - started } : {}),
      };
      span.setAttributes(attributes);
      span.addEvent("bones.list", attributes);
      console.info(JSON.stringify({ target: "bones.list", ...attributes }));
      span.end();
      activeSpan = undefined;
    }
  }
  return {
    get span() { return activeSpan; },
    pages: () => iterate(options.prefetch ?? 1, true),
    async *items() {
      let deliveredPages = 0;
      let deliveredItems = 0;
      for await (const page of iterate(options.prefetch ?? 1, false)) {
        deliveredPages++;
        for (const item of page.items) {
          if (options.signal?.aborted) {
            throw new PageError(new ConnectError("list cancelled", Code.Canceled), deliveredPages, deliveredItems);
          }
          yield item;
          deliveredItems++;
        }
      }
    },
    async firstPage() {
      for await (const page of iterate(0, true)) return page;
      throw new PageError(new ConnectError("list returned no page", Code.Internal), 0, 0);
    },
  };
}
export function pages<T>(fetchPage: PageFetch, request: Record<string, unknown>, config: ListConfig<T>, options?: PageOptions): AsyncIterable<Page<T>> {
  return listCall(fetchPage, request, config, options).pages();
}
export function items<T>(fetchPage: PageFetch, request: Record<string, unknown>, config: ListConfig<T>, options?: PageOptions): AsyncIterable<T> {
  return listCall(fetchPage, request, config, options).items();
}

type ListItem<R> = { [K in keyof R]: R[K] extends readonly (infer T)[] ? T : never }[keyof R];
type HasPage<R> = Extract<R[keyof R], { nextCursor: string; revision: string }> extends never ? false : true;
export type PagedClient<S extends DescService> = {
  [K in keyof Client<S>]: Client<S>[K] extends (request: infer R, ...args: never[]) => Promise<infer O>
    ? HasPage<O> extends true ? (request: R, options?: PageOptions) => ListCall<ListItem<O>> : Client<S>[K]
    : Client<S>[K] extends (request: infer R, ...args: never[]) => AsyncIterable<infer O>
      ? HasPage<O> extends true ? (request: R, options?: PageOptions) => ListCall<ListItem<O>> : Client<S>[K]
      : Client<S>[K];
};
export function pagedClient<S extends DescService>(client: Client<S>, policy: GeneratedMethodPolicyDocument, cache?: RevisionCache): PagedClient<S> {
  assertSupportedPolicySchema(policy);
  const { service } = platformBinding(client);
  const wrapped = { ...client } as Record<string, unknown>;
  for (const method of indexGeneratedPolicy(policy).values()) {
    if (!method.paging || !method.rpc.startsWith(`/${service.typeName}/`)) continue;
    const name = method.rpc.slice(method.rpc.lastIndexOf("/") + 1);
    const localName = name.charAt(0).toLowerCase() + name.slice(1);
    const original = wrapped[localName];
    if (typeof original !== "function") throw new Error(`missing list method ${method.rpc}`);
    wrapped[localName] = (request: Record<string, unknown>, options?: PageOptions) => {
      let stream: AsyncIterator<unknown> | undefined;
      const fetchPage: PageFetch = method.paging!.kind === "unary"
        ? (input, callOptions) => original.call(client, input, callOptions)
        : async (input, callOptions) => {
          const open = () => {
            const source = original.call(client, input, callOptions) as AsyncIterable<unknown>;
            stream = source[Symbol.asyncIterator]();
          };
          if (!stream) open();
          let next = await stream!.next();
          if (next.done) {
            // A finite stream cut resumes from the last delivered batch cursor.
            open();
            next = await stream!.next();
          }
          if (next.done) throw new ConnectError("list stream ended without a batch", Code.Internal);
          return next.value;
        };
      return listCall(fetchPage, request, { method: method.rpc, paging: method.paging!, cache }, method.paging!.kind === "stream" ? { ...options, cache: "bypass" } : options);
    };
  }
  return wrapped as PagedClient<S>;
}
