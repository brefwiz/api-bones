// SPDX-License-Identifier: MIT

/** What one request carried. */
export interface Seen {
  traceparent: string;
  /** The `authorization` header; empty when the request carried none. */
  authorization: string;
  timeoutMs: number;
  /** The `bones-list-cache` header; empty when the request carried none. */
  listCache: string;
  /** The `if-none-match` header; empty when the request carried none. */
  ifNoneMatch: string;
  /** The page size a list request asked for; zero for any other request or none asked. */
  pageSize: number;
}

/** What a scenario scripts on the server; a `null` clears a scripted change. */
export interface Patch {
  /** The scenario that runs; the server names it on every line it narrates. */
  scenario?: string;
  widgets?: Record<string, string>;
  parts?: Record<string, string[][]>;
  pages?: number[][];
  pageDelayMs?: number;
  ticks?: number;
  tickMs?: number;
  unavailable?: number;
  unauthenticated?: number;
  /** Answer the next `count` requests as resource exhausted, naming `retryAfterSeconds` whole seconds in `retry-after`. */
  exhausted?: { count: number; retryAfterSeconds: number };
  switchTo?: string | null;
  switchOnPage?: { index: number; subject: string } | null;
  refreshTo?: string | null;
  expireAtPage?: number | null;
  garble?: boolean;
}
