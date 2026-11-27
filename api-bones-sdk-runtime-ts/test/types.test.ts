// SPDX-License-Identifier: MIT
import { describe, expect, it } from "vitest";
import type { Span } from "@opentelemetry/api";
import {
  isCursorRejected,
  type ListCall,
  type Page,
  type PageError,
  type PageErrorKind,
} from "../src/index.js";

/** `true` only when each type is assignable to the other. */
type Same<A, B> = [A] extends [B] ? ([B] extends [A] ? true : false) : false;

/**
 * The surface a paged-list consumer reads: a list with `pages()`, `items()`,
 * `firstPage()`, `withPrefetch()` and the iteration span; pages with their revision, total, cache
 * flag and `hasNext`; and the page error's kind and delivered counts.
 */
interface ConsumedPage<T> {
  readonly items: readonly T[];
  readonly revision: string;
  readonly total?: { readonly value: bigint; readonly kind: "estimate" | "exact" };
  readonly fromCache: boolean;
  readonly hasNext: boolean;
}
interface ConsumedListCall<T> {
  pages(): AsyncIterable<ConsumedPage<T>>;
  items(): AsyncIterable<T>;
  firstPage(): Promise<ConsumedPage<T>>;
  withPrefetch(pages: 0 | 1): ConsumedListCall<T>;
  readonly span?: Span;
}
type ConsumedPageError = {
  readonly kind: "cursor_invalid" | "cursor_expired" | "cursor_query_mismatch" | "rpc";
  readonly deliveredPages: number;
  readonly deliveredItems: number;
};

interface Row {
  readonly id: string;
}

describe("the list surface is structurally the one a paged-list consumer reads", () => {
  it("ListCall and Page", () => {
    const consumed: Same<ListCall<Row>, ConsumedListCall<Row>> = true;
    const page: Same<Page<Row>, ConsumedPage<Row>> = true;
    expect([consumed, page]).toEqual([true, true]);
  });

  it("PageError and its kinds", () => {
    const error: Same<Pick<PageError, "kind" | "deliveredPages" | "deliveredItems">, ConsumedPageError> = true;
    const kinds: Same<PageErrorKind, ConsumedPageError["kind"]> = true;
    expect([error, kinds]).toEqual([true, true]);
  });

  it("isCursorRejected takes any value and answers a boolean", () => {
    const signature: Same<typeof isCursorRejected, (error: unknown) => boolean> = true;
    expect(signature).toBe(true);
  });
});
