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
  /**
   * The same list reading `pages` ahead of the consumer: 0 reads only the
   * pages asked for, 1 reads one ahead; the Rust core reads one ahead for any
   * larger value. It wins over the `prefetch` the list was called with.
   */
  withPrefetch(pages: 0 | 1): ListCall<T>;
  /** Active iteration span, usable by the UI for first-row measurements. */
  readonly span?: Span;
}

/** A Rust-side cursor over a sequence of messages, each delivered as its protobuf bytes. */
export interface Handle<Raw = Uint8Array> {
  /** The next message, or `undefined` when the sequence has ended. */
  next(): Promise<Raw | undefined>;
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
export function iterateHandle<T, Raw = Uint8Array>(
  handle: Handle<Raw>,
  decode: (raw: Raw) => T,
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
      let raw: Raw | undefined;
      try {
        raw = await handle.next();
      } catch (rejection) {
        const mapped = mapError(rejection);
        // A busy refusal is about the overlapping call; the pending read still owns the handle.
        if ((mapped as { code?: unknown } | null)?.code !== "HANDLE_BUSY") close();
        throw mapped;
      }
      if (raw === undefined) {
        close();
        return ended;
      }
      try {
        return { done: false, value: decode(raw) };
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
export function iterableOf<T, Raw = Uint8Array>(
  open: () => Handle<Raw>,
  decode: (raw: Raw) => T,
  mapError?: ErrorMapper,
): AsyncIterable<T> {
  return { [Symbol.asyncIterator]: () => iterateHandle(open(), decode, mapError) };
}

/** What a generated wrapper supplies to build a {@link ListCall}. */
export interface ListCallSource<T, Raw = Uint8Array> {
  /**
   * Opens a handle over the list's pages. Called once per walk. A `prefetch`
   * names the pages the Rust core fetches ahead and wins over the caller's
   * option; `undefined` leaves the caller's option. `firstOnly` asks the core
   * for its first-page read: the host forwards it and keeps no prefetch rule.
   */
  openPages(prefetch?: 0 | 1, firstOnly?: boolean): Handle<Raw>;
  decodePage(raw: Raw): Page<T>;
  mapError?: ErrorMapper;
  /** Resolves a handle's iteration span to the live span. */
  sink: JsSpanSink;
}

/** A {@link ListCall} over Rust page handles. */
export function listCallOf<T, Raw = Uint8Array>(source: ListCallSource<T, Raw>): ListCall<T> {
  return listCallReading(source, undefined);
}

function listCallReading<T, Raw>(source: ListCallSource<T, Raw>, chosen: 0 | 1 | undefined): ListCall<T> {
  let current: Handle<Raw> | undefined;
  const walkPages = (firstOnly?: true): AsyncIterableIterator<Page<T>> => {
    current = firstOnly ? source.openPages(undefined, true) : source.openPages(chosen);
    return iterateHandle(current, source.decodePage, source.mapError);
  };
  const pages = (): AsyncIterableIterator<Page<T>> => walkPages();
  return {
    get span(): Span | undefined {
      const traceparent = current?.span();
      return traceparent === undefined ? undefined : source.sink.lookup(traceparent);
    },
    pages,
    withPrefetch: (pages) => listCallReading(source, pages),
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
          buffered = [];
          at = 0;
          await walk.return?.();
          return { done: true, value: undefined };
        },
        [Symbol.asyncIterator]() {
          return this;
        },
      };
    },
    async firstPage(): Promise<Page<T>> {
      const walk = walkPages(true);
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
