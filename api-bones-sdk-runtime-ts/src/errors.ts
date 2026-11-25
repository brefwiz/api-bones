// SPDX-License-Identifier: MIT
/**
 * The error model a generated wrapper raises. The Rust core sorts every
 * failure into these fields; the wrapper only revives them into classes.
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
}

/** A failed call. A generated wrapper extends it once per service. */
export class BonesError extends Error {
  override name = "BonesError";
  readonly code: string;
  readonly emitter: string;
  readonly platform: string;
  readonly connectCode: string;
  readonly args: Readonly<Record<string, ErrorArg>>;

  constructor(init: BonesErrorInit) {
    super(init.message);
    this.code = init.code;
    this.emitter = init.emitter;
    this.platform = init.platform;
    this.connectCode = init.connectCode;
    this.args = init.args ?? {};
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

/** An error argument as it crosses the wire: tagged like `bones.v1.ErrorArg`. */
export type ErrorArgWire =
  | { readonly kind: "string"; readonly value: string }
  | { readonly kind: "int64"; readonly value: string }
  | { readonly kind: "double"; readonly value: number | "NaN" | "Infinity" | "-Infinity" }
  | { readonly kind: "timestamp"; readonly value: string };

/** The wire JSON of a failed call, schema `api-bones.error/v1`. */
export interface BonesErrorWire {
  readonly code: string;
  readonly emitter: string;
  readonly platform: string;
  readonly connectCode: string;
  readonly message: string;
  readonly args: Readonly<Record<string, ErrorArgWire>>;
  /** Present only for list failures. */
  readonly page?: PageDetail;
}

/** Revives one tagged argument. An unknown kind or an unreadable value gives `undefined`. */
function reviveArg(wire: ErrorArgWire): ErrorArg | undefined {
  switch (wire.kind) {
    case "string":
      return wire.value;
    case "int64":
      try {
        return BigInt(wire.value);
      } catch {
        return undefined;
      }
    case "double":
      return typeof wire.value === "number" ? wire.value : Number(wire.value);
    case "timestamp": {
      const date = new Date(wire.value.replace(/(\.\d{3})\d+/, "$1"));
      return Number.isNaN(date.getTime()) ? undefined : date;
    }
    default:
      return undefined;
  }
}

/**
 * Revives the wire JSON of a failed call into its error class: a
 * {@link PageError} when the wire carries `page`, else a {@link BonesError}.
 * An argument with an unknown kind is dropped and the error kept.
 */
export function parseBonesError(json: string): BonesError | PageError {
  const wire = JSON.parse(json) as BonesErrorWire;
  const args: Record<string, ErrorArg> = Object.fromEntries(
    Object.entries(wire.args).flatMap(([name, tagged]) => {
      const value = reviveArg(tagged);
      return value === undefined ? [] : [[name, value]];
    }),
  );
  const init: BonesErrorInit = {
    code: wire.code,
    emitter: wire.emitter,
    platform: wire.platform,
    connectCode: wire.connectCode,
    message: wire.message,
    args,
  };
  return wire.page === undefined ? new BonesError(init) : new PageError(init, wire.page);
}
