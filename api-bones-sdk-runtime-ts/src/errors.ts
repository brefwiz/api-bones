// SPDX-License-Identifier: MIT
/**
 * The error model a generated wrapper raises. The Rust core sorts every
 * failure into these fields; the runtime only revives them into classes.
 */

/** One typed argument of an error message, as the wrapper revives it. */
export type ErrorArg = string | bigint | number | Date;

/** The fields the Rust core reports for a failed call. */
export interface BonesErrorInit {
  /** The emitted error code, such as `GRANT_MISSING`. */
  readonly code: string;
  /** The emitting deployable. */
  readonly emitter: string;
  /** The platform `ErrorCode` the Rust core maps the refusal to, such as `FORBIDDEN`. */
  readonly platform: string;
  /** The Connect code of the refusal, in its lower-case wire form. */
  readonly connectCode: string;
  /** The message arguments, keyed by placeholder name. */
  readonly args?: Readonly<Record<string, ErrorArg>>;
  /** The diagnostic text, for logs. Clients render {@link formatError} instead. */
  readonly message: string;
  /**
   * The whole milliseconds a `resource_exhausted` refusal named in its
   * `retry-after`; absent when it named none.
   */
  readonly retryAfterMs?: number;
}

/** A failed call. A generated wrapper extends it once per service. */
export class BonesError extends Error {
  override name = "BonesError";
  readonly code: string;
  readonly emitter: string;
  readonly platform: string;
  readonly connectCode: string;
  readonly args: Readonly<Record<string, ErrorArg>>;
  /** The wait the refusal named, in milliseconds; `null` when it named none. */
  readonly retryAfterMs: number | null;

  constructor(init: BonesErrorInit) {
    super(init.message);
    this.code = init.code;
    this.emitter = init.emitter;
    this.platform = init.platform;
    this.connectCode = init.connectCode;
    this.args = init.args ?? {};
    this.retryAfterMs = init.retryAfterMs ?? null;
  }
}

/** Why a page of a list could not be delivered. */
export type PageErrorKind = "cursor_invalid" | "cursor_expired" | "cursor_query_mismatch" | "rpc";

/** The page-level detail the Rust core attaches to a list failure. */
export interface PageDetail {
  readonly kind: PageErrorKind;
  readonly deliveredPages: number;
  readonly deliveredItems: number;
}

/** A list failed part-way: what was delivered before it, and why it stopped. */
export class PageError extends BonesError {
  override name = "PageError";
  readonly kind: PageErrorKind;
  readonly deliveredPages: number;
  readonly deliveredItems: number;

  constructor(init: BonesErrorInit, page: PageDetail) {
    super(init);
    this.kind = page.kind;
    this.deliveredPages = page.deliveredPages;
    this.deliveredItems = page.deliveredItems;
  }
}

/** Whether the server refused the list's position rather than the call itself. */
export function isCursorRejected(error: unknown): boolean {
  return error instanceof PageError && error.kind !== "rpc";
}
