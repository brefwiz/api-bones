// SPDX-License-Identifier: MIT
import type { Span } from "@opentelemetry/api";
import type { JsSpanSink } from "./spans.js";

/** Options a caller passes to a list method; the Rust core acts on them. */
export interface PageOptions {
  pageSize?: number;
  prefetch?: 0 | 1;
  signal?: AbortSignal;
  cache?: "use" | "bypass";
}

export interface Total {
  readonly value: bigint;
  readonly kind: "estimate" | "exact";
}

export interface Page<T> {
  readonly items: readonly T[];
  readonly revision: string;
  readonly total?: Total;
  readonly fromCache: boolean;
  /** True exactly when the response carried a non-empty next cursor. */
  readonly hasNext: boolean;
}

/** A list a caller can walk page by page, item by item, or read the first page of. */
export interface ListCall<T> {
  pages(): AsyncIterable<Page<T>>;
  items(): AsyncIterable<T>;
  firstPage(): Promise<Page<T>>;
  /** Active iteration span, usable by the UI for first-row measurements. */
  readonly span?: Span;
}

/** A Rust-side cursor over a sequence of JSON documents. */
export interface Handle {
  /** The next document, or `undefined` when the sequence has ended. */
  next(): Promise<string | undefined>;
  /** Releases the Rust side, dropping anything it read ahead. */
  cancel(): void;
  /** The `traceparent` of the handle's open iteration span, or `undefined` when it is not recording. */
  span(): string | undefined;
}

/** What turns a handle's rejection into the error the caller sees. */
export type ErrorMapper = (rejection: unknown) => unknown;

/**
 * Walks a handle as an async iterator. The handle is cancelled exactly once:
 * when the sequence ends, when `next()` fails, or when the consumer calls
 * `return()` (a `break`, an unmount or an abort), whichever comes first.
 */
export function iterateHandle<T>(
  handle: Handle,
  decode: (json: string) => T,
  mapError: ErrorMapper = (rejection) => rejection,
): AsyncIterableIterator<T> {
  let closed = false;
  const close = (): void => {
    if (closed) return;
    closed = true;
    handle.cancel();
  };
  const ended: IteratorResult<T, undefined> = { done: true, value: undefined };
  return {
    async next(): Promise<IteratorResult<T, undefined>> {
      if (closed) return ended;
      let json: string | undefined;
      try {
        json = await handle.next();
      } catch (rejection) {
        close();
        throw mapError(rejection);
      }
      if (json === undefined) {
        close();
        return ended;
      }
      try {
        return { done: false, value: decode(json) };
      } catch (failure) {
        close();
        throw failure;
      }
    },
    async return(): Promise<IteratorResult<T, undefined>> {
      close();
      return ended;
    },
    [Symbol.asyncIterator]() {
      return this;
    },
  };
}

/** A sequence that opens a fresh handle for every walk, such as a server stream. */
export function iterableOf<T>(
  open: () => Handle,
  decode: (json: string) => T,
  mapError?: ErrorMapper,
): AsyncIterable<T> {
  return { [Symbol.asyncIterator]: () => iterateHandle(open(), decode, mapError) };
}

/** What a generated wrapper supplies to build a {@link ListCall}. */
export interface ListCallSource<T> {
  /** Opens a handle over the list's pages. Called once per walk. */
  openPages(): Handle;
  decodePage(json: string): Page<T>;
  mapError?: ErrorMapper;
  /** Resolves a handle's iteration span to the live span. */
  sink: JsSpanSink;
}

/** A {@link ListCall} over Rust page handles. */
export function listCallOf<T>(source: ListCallSource<T>): ListCall<T> {
  let current: Handle | undefined;
  const pages = (): AsyncIterableIterator<Page<T>> => {
    current = source.openPages();
    return iterateHandle(current, source.decodePage, source.mapError);
  };
  return {
    get span(): Span | undefined {
      const traceparent = current?.span();
      return traceparent === undefined ? undefined : source.sink.lookup(traceparent);
    },
    pages,
    items(): AsyncIterableIterator<T> {
      const walk = pages();
      let buffered: readonly T[] = [];
      let at = 0;
      return {
        async next(): Promise<IteratorResult<T, undefined>> {
          while (at >= buffered.length) {
            const step = await walk.next();
            if (step.done) return { done: true, value: undefined };
            buffered = step.value.items;
            at = 0;
          }
          return { done: false, value: buffered[at++] };
        },
        async return(): Promise<IteratorResult<T, undefined>> {
          await walk.return?.();
          return { done: true, value: undefined };
        },
        [Symbol.asyncIterator]() {
          return this;
        },
      };
    },
    async firstPage(): Promise<Page<T>> {
      const walk = pages();
      try {
        const step = await walk.next();
        if (step.done) throw new TypeError("the page handle ended before delivering a page");
        return step.value;
      } finally {
        await walk.return?.();
      }
    },
  };
}
