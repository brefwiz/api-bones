// SPDX-License-Identifier: MIT
// ===== ./errors.d.ts =====
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
export declare class BonesError extends Error {
    name: string;
    readonly code: string;
    readonly emitter: string;
    readonly platform: string;
    readonly connectCode: string;
    readonly args: Readonly<Record<string, ErrorArg>>;
    constructor(init: BonesErrorInit);
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
export declare class PageError extends BonesError {
    name: string;
    readonly kind: PageErrorKind;
    readonly deliveredPages: number;
    readonly deliveredItems: number;
    constructor(init: BonesErrorInit, page: PageDetail);
}
/** Whether the server refused the list's position rather than the call itself. */
export declare function isCursorRejected(error: unknown): boolean;
/** An error argument as it crosses the wire: tagged like `bones.v1.ErrorArg`. */
export type ErrorArgWire = {
    readonly kind: "string";
    readonly value: string;
} | {
    readonly kind: "int64";
    readonly value: string;
} | {
    readonly kind: "double";
    readonly value: number | "NaN" | "Infinity" | "-Infinity";
} | {
    readonly kind: "timestamp";
    readonly value: string;
};
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
/**
 * Revives the wire JSON of a failed call into its error class: a
 * {@link PageError} when the wire carries `page`, else a {@link BonesError}.
 * An argument with an unknown kind is dropped and the error kept.
 */
export declare function parseBonesError(json: string): BonesError | PageError;
// ===== ./format-error.d.ts =====
import type { BonesError } from "./errors.js";
/** The ICU messages of one emitter in one locale, keyed `<emitter>.errors.<CODE>.title` and `.description`. */
export type ErrorCatalog = Readonly<Record<string, string>>;
/**
 * Adds an emitter's messages for one locale. Only keys prefixed `<emitter>.errors.` are kept; the same entries again change nothing, different ones make that emitter's locale unusable.
 */
export declare function registerErrorCatalog(emitter: string, locale: string, messages: ErrorCatalog): void;
/** What {@link formatError} reads of an error. */
export type FormattableError = Pick<BonesError, "emitter" | "code" | "platform" | "args">;
/** What a caller may pin when formatting. */
export interface FormatErrorOptions {
    /** The IANA time zone dates and times render in. Defaults to `UTC`. */
    readonly timeZone?: string;
}
/** The option is not a time zone the platform knows. */
export declare class FormatErrorOptionError extends Error {
    name: string;
    readonly rule: "unknown_time_zone";
    constructor();
}
/** A formatted error: the text and, when its catalog has one, the longer hint. */
export interface FormattedError {
    readonly text: string;
    readonly hint?: string;
}
/**
 * The error's message in `locale`, falling back to `en`, then to the platform text of its code; `text` is empty only while no platform catalog is registered.
 */
export declare function formatError(error: FormattableError, locale: string, options?: FormatErrorOptions): FormattedError;
// ===== ./index.d.ts =====
export { BonesError, PageError, isCursorRejected, parseBonesError, type BonesErrorInit, type BonesErrorWire, type ErrorArg, type ErrorArgWire, type PageDetail, type PageErrorKind } from "./errors.js";
export { iterableOf, iterateHandle, listCallOf, type ErrorMapper, type Handle, type ListCall, type ListCallSource, type Page, type PageOptions, type Total } from "./list-call.js";
export { FormatErrorOptionError, formatError, registerErrorCatalog, type ErrorCatalog, type FormatErrorOptions, type FormattableError, type FormattedError } from "./format-error.js";
export { JsSpanSink, callCtx, type CallCtx, type SpanEnd, type SpanStart } from "./spans.js";
export { lazyOnce, wasmInput } from "./loader.js";
// ===== ./list-call.d.ts =====
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
/** A Rust-side cursor over a sequence of documents; a document is JSON text unless the handle delivers more with it. */
export interface Handle<Raw = string> {
    /** The next document, or `undefined` when the sequence has ended. */
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
export declare function iterateHandle<T, Raw = string>(handle: Handle<Raw>, decode: (raw: Raw) => T, mapError?: ErrorMapper): AsyncIterableIterator<T>;
/** A sequence that opens a fresh handle for every walk, such as a server stream. */
export declare function iterableOf<T>(open: () => Handle, decode: (json: string) => T, mapError?: ErrorMapper): AsyncIterable<T>;
/** What a generated wrapper supplies to build a {@link ListCall}. */
export interface ListCallSource<T, Raw = string> {
    /** Opens a handle over the list's pages. Called once per walk. */
    openPages(): Handle<Raw>;
    decodePage(raw: Raw): Page<T>;
    mapError?: ErrorMapper;
    /** Resolves a handle's iteration span to the live span. */
    sink: JsSpanSink;
}
/** A {@link ListCall} over Rust page handles. */
export declare function listCallOf<T, Raw = string>(source: ListCallSource<T, Raw>): ListCall<T>;
// ===== ./loader.d.ts =====
/**
 * Starts something once, on first use. Concurrent callers share the one start.
 * A failed start is forgotten, so the next caller starts again.
 */
export declare function lazyOnce<T>(start: () => Promise<T>): () => Promise<T>;
/**
 * What the wasm module's initialiser takes for the module at `url`. A `file:`
 * module can only be reached from Node, so its bytes are read; any other
 * location is handed back for the initialiser to load.
 */
export declare function wasmInput(url: URL): Promise<URL | Uint8Array>;
// ===== ./spans.d.ts =====
import type { Attributes, Span, Tracer } from "@opentelemetry/api";
/** The per-call inputs the host hands the Rust core. */
export interface CallCtx {
    signal?: AbortSignal;
    traceparent?: string;
    tracestate?: string;
}
/**
 * The call context for the active JavaScript trace: the W3C trace fields the
 * registered propagator writes for `context.active()`, plus the caller's abort
 * signal. Only the host can see the active context.
 */
export declare function callCtx(signal?: AbortSignal): CallCtx;
/** What the Rust core reports when it opens a span. Times are epoch milliseconds. */
export interface SpanStart {
    readonly name: string;
    readonly startTime: number;
    readonly kind: "client" | "internal";
    /** The parent's W3C `traceparent`; absent for a new root. */
    readonly parent?: string;
    readonly tracestate?: string;
    readonly attributes?: Attributes;
}
/** What the Rust core reports when it ends a span it opened. */
export interface SpanEnd {
    /** The value `start` returned. */
    readonly traceparent: string;
    readonly endTime: number;
    readonly status: "ok" | "error" | "unset";
    /** `<connectCode> <code>`, set when `status` is `error`. */
    readonly error?: string;
    readonly attributes?: Attributes;
}
/** The live JavaScript OpenTelemetry spans of the Rust core's spans. */
export declare class JsSpanSink {
    #private;
    constructor(tracer?: Tracer);
    /**
     * Opens a span from a `SpanStart` JSON document and returns its
     * `traceparent`. A span that is not recording is ended at once and gives
     * `undefined`, so the core mints its own ids.
     */
    start(json: string): string | undefined;
    /** Ends the span `start` returned, from a `SpanEnd` JSON document. An unknown `traceparent` is ignored. */
    end(json: string): void;
    /** The open span for `traceparent`, if the core has not ended it. */
    lookup(traceparent: string): Span | undefined;
}
