// SPDX-License-Identifier: MIT
/**
 * Renders a refused call's typed error identity in the viewer's locale from
 * the owner's catalog. Rendering is synchronous; non-`en` platform catalogs
 * load per locale through `loadErrorCatalog`. The Rust counterpart is the
 * `api-bones-i18n` crate and both answer `tests/features/error_localization.feature`.
 */

import { Code, ConnectError } from "@connectrpc/connect";
import { parse, TYPE, type MessageFormatElement } from "@formatjs/icu-messageformat-parser";
import { IntlMessageFormat, type Formats } from "intl-messageformat";

import BONES_EN from "./error-catalogs/en.generated.js";
import { LOADERS } from "./error-catalogs/loaders.generated.js";
import {
  decodeValidationFailureKept,
  ErrorArgError,
  errorInfoOf,
  validateErrorArgs,
  VALIDATION_FAILURE_TYPE,
  type ErrorArg,
  type ErrorInfo,
} from "./error-info.js";

export { ErrorArgError, type ErrorArgRule } from "./error-info.js";

/** The typed, bounded arguments of one message, in ascending key order. */
export type ErrorArgs = ReadonlyMap<string, ErrorArg>;
/** One argument as a caller supplies it to {@link errorArgs}. */
export type ErrorArgValue = ErrorArg;

/** One validation failure of a refused call. */
export interface Violation {
  readonly field: string;
  readonly rule: string;
  readonly message: string;
  /** The refusal stands in for arguments that broke the bounds or did not decode. */
  readonly args: ErrorArgs | ErrorArgError;
}

/** The parsed body of a `problem+json` response. */
export type ProblemJson = { readonly [member: string]: unknown };

/** A generated `bones.v1.ValidationFailure`. */
interface GeneratedValidationFailure {
  readonly $typeName: "bones.v1.ValidationFailure";
  readonly field: string;
  readonly rule: string;
  readonly message: string;
  readonly args: { readonly [key: string]: GeneratedErrorArg };
}
interface GeneratedErrorArg {
  readonly value:
    | { readonly case: "string"; readonly value: string }
    | { readonly case: "int64"; readonly value: bigint }
    | { readonly case: "double"; readonly value: number }
    | { readonly case: "timestamp"; readonly value: { readonly seconds: bigint; readonly nanos: number } }
    | { readonly case: undefined; readonly value?: undefined };
}

export type ValidationFailure = GeneratedValidationFailure;
export type Localizable = ConnectError | ProblemJson | ValidationFailure | Violation;

/** An option the call refused. */
export class LocalizeOptionsError extends Error {
  override name = "LocalizeOptionsError";
  readonly rule: "unknown_time_zone" | "locale_not_loaded";
  constructor(rule: "unknown_time_zone" | "locale_not_loaded" = "unknown_time_zone") {
    super(`option refused: ${rule}`);
    this.rule = rule;
  }
}

// ---------------------------------------------------------------------------
// Locale negotiation: the committed copy of core-ui's platform locale registry.

export const DEFAULT_LOCALE = "en";
export const PLATFORM_LOCALES: readonly string[] = [
  "en", "fr", "de", "es", "it", "pt-BR", "nl", "sv", "da", "fi", "nb", "pl", "cs", "sk", "sl",
  "hr", "hu", "ro", "bg", "el", "et", "lv", "lt", "tr", "ja", "ko", "zh-Hans", "he", "ar",
];
export const LOCALE_ALIASES: Readonly<Record<string, string>> = {
  no: "nb",
  nn: "nb",
  iw: "he",
  pt: "pt-BR",
  zh: "zh-Hans",
  "zh-cn": "zh-Hans",
  "zh-sg": "zh-Hans",
};

function exactLocale(lower: string): string | undefined {
  return (
    PLATFORM_LOCALES.find((code) => code.toLowerCase() === lower) ??
    (own(LOCALE_ALIASES, lower) ? LOCALE_ALIASES[lower] : undefined)
  );
}

function matchLocale(tag: string): string | undefined {
  const lower = tag.trim().replaceAll("_", "-").toLowerCase();
  if (lower === "" || lower === "*") return undefined;
  const exact = exactLocale(lower);
  if (exact) return exact;
  const [language, ...subtags] = lower.split("-");
  if (language === "zh") return subtags.includes("hans") ? "zh-Hans" : undefined;
  return exactLocale(language);
}

function quality(params: string): number | undefined {
  let q = 1;
  for (const raw of params.split(";")) {
    const param = raw.trim();
    if (param.startsWith("q=") || param.startsWith("Q=")) {
      const text = param.slice(2).trim();
      q = /^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/.test(text) ? Number(text) : Number.NaN;
    }
  }
  return Number.isFinite(q) && q > 0 && q <= 1 ? q : undefined;
}

/** A BCP 47 tag or an Accept-Language value, as a platform locale. */
export function negotiateLocale(requested: string): string {
  const entries: Array<{ q: number; tag: string }> = [];
  for (const entry of requested.split(",")) {
    const cut = entry.indexOf(";");
    const q = quality(cut < 0 ? "" : entry.slice(cut + 1));
    if (q !== undefined) entries.push({ q, tag: cut < 0 ? entry : entry.slice(0, cut) });
  }
  entries.sort((a, b) => b.q - a.q);
  for (const { tag } of entries) {
    const matched = matchLocale(tag);
    if (matched) return matched;
  }
  return DEFAULT_LOCALE;
}

// ---------------------------------------------------------------------------
// Arguments.

/**
 * Build a bounded argument set. Entries are applied in input order and the
 * first rule broken wins.
 *
 * @throws {@link ErrorArgError} with the rule that failed.
 */
export function errorArgs(entries: Readonly<Record<string, ErrorArgValue>>): ErrorArgs {
  const checked = new Map<string, ErrorArg>();
  for (const [key, value] of Object.entries(entries)) {
    if (!checked.has(key) && checked.size >= 32) throw new ErrorArgError("too-many");
    validateErrorArgs(new Map([[key, value]]));
    checked.set(key, value);
  }
  const order = new TextEncoder();
  const bytes = (key: string) => [...order.encode(key)];
  return new Map(
    [...checked].sort(([a], [b]) => {
      const x = bytes(a);
      const y = bytes(b);
      for (let i = 0; i < Math.min(x.length, y.length); i++) if (x[i] !== y[i]) return x[i] - y[i];
      return x.length - y.length;
    }),
  );
}

const own = (object: object, key: string): boolean => Object.prototype.hasOwnProperty.call(object, key);

const isObject = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

const INT64 = /^-?(0|[1-9][0-9]*)$/;
const RFC3339 = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.(\d{1,9}))?(Z|[+-]\d{2}:\d{2})$/;

function daysFromCivil(year: number, month: number, day: number): number {
  const y = month <= 2 ? year - 1 : year;
  const era = Math.floor(y / 400);
  const yoe = y - era * 400;
  const doy = Math.floor((153 * (month + (month > 2 ? -3 : 9)) + 2) / 5) + day - 1;
  const doe = yoe * 365 + Math.floor(yoe / 4) - Math.floor(yoe / 100) + doy;
  return era * 146097 + doe - 719468;
}

function parseTimestamp(text: string): { seconds: bigint; nanos: number } | undefined {
  const m = RFC3339.exec(text);
  if (!m) return undefined;
  const [year, month, day, hour, minute, second] = m.slice(1, 7).map(Number);
  const daysInMonth = new Date(Date.UTC(2000 + (year % 400), month, 0)).getUTCDate();
  if (month < 1 || month > 12 || day < 1 || day > daysInMonth) return undefined;
  if (hour > 23 || minute > 59 || second > 59) return undefined;
  let offset = 0;
  if (m[8] !== "Z") {
    const [oh, om] = [Number(m[8].slice(1, 3)), Number(m[8].slice(4, 6))];
    if (oh > 23 || om > 59) return undefined;
    offset = (m[8][0] === "+" ? 1 : -1) * (oh * 3600 + om * 60);
  }
  const seconds = daysFromCivil(year, month, day) * 86400 + hour * 3600 + minute * 60 + second - offset;
  const nanos = m[7] ? Number(m[7].padEnd(9, "0")) : 0;
  return { seconds: BigInt(seconds), nanos };
}

/** One proto3 JSON `ErrorArg`: exactly one member, strictly typed. `undefined` is malformed. */
function jsonArg(value: unknown): ErrorArg | undefined {
  if (!isObject(value)) return undefined;
  const members = Object.keys(value);
  if (members.length !== 1) return undefined;
  const [name] = members;
  const raw = value[name];
  switch (name) {
    case "stringValue":
      return typeof raw === "string" ? { kind: "string", value: raw } : undefined;
    case "int64Value":
      return typeof raw === "string" && INT64.test(raw) ? { kind: "int64", value: BigInt(raw) } : undefined;
    case "doubleValue":
      return typeof raw === "number" && Number.isFinite(raw) ? { kind: "double", value: raw } : undefined;
    case "timestampValue": {
      const parsed = typeof raw === "string" ? parseTimestamp(raw) : undefined;
      return parsed ? { kind: "timestamp", ...parsed } : undefined;
    }
    default:
      return undefined;
  }
}

/** The arguments of a JSON member, or the refusal. Absent arguments are an empty set. */
function jsonArgs(raw: unknown): ErrorArgs | ErrorArgError {
  if (raw === undefined) return new Map();
  if (!isObject(raw)) return new ErrorArgError("malformed");
  const args = new Map<string, ErrorArg>();
  for (const [key, value] of Object.entries(raw)) {
    const arg = jsonArg(value);
    if (!arg) return new ErrorArgError("malformed");
    args.set(key, arg);
  }
  return checkedArgs(args);
}

function checkedArgs(args: ErrorArgs): ErrorArgs | ErrorArgError {
  try {
    validateErrorArgs(args);
    return args;
  } catch (error) {
    if (error instanceof ErrorArgError) return error;
    throw error;
  }
}

function generatedArgs(args: GeneratedValidationFailure["args"]): ErrorArgs | ErrorArgError {
  const out = new Map<string, ErrorArg>();
  for (const [key, arg] of Object.entries(args)) {
    const v = arg.value;
    if (v.case === "string") out.set(key, { kind: "string", value: v.value });
    else if (v.case === "int64") out.set(key, { kind: "int64", value: v.value });
    else if (v.case === "double") out.set(key, { kind: "double", value: v.value });
    else if (v.case === "timestamp") out.set(key, { kind: "timestamp", seconds: v.value.seconds, nanos: v.value.nanos });
  }
  return checkedArgs(out);
}

// ---------------------------------------------------------------------------
// Identity extraction.

/** [code, HTTP status] in the order of `ErrorCode`. */
const PLATFORM_CODES: ReadonlyArray<readonly [string, number]> = [
  ["BAD_REQUEST", 400], ["VALIDATION_FAILED", 400], ["UNAUTHORIZED", 401], ["INVALID_CREDENTIALS", 401],
  ["TOKEN_EXPIRED", 401], ["TOKEN_INVALID", 401], ["FORBIDDEN", 403], ["INSUFFICIENT_PERMISSIONS", 403],
  ["ORG_OUTSIDE_SUBTREE", 403], ["ANCESTOR_REQUIRED", 403], ["CROSS_SUBTREE_ACCESS", 403],
  ["RESOURCE_NOT_FOUND", 404], ["METHOD_NOT_ALLOWED", 405], ["NOT_ACCEPTABLE", 406], ["REQUEST_TIMEOUT", 408],
  ["CONFLICT", 409], ["RESOURCE_ALREADY_EXISTS", 409], ["GONE", 410], ["PRECONDITION_FAILED", 412],
  ["PAYLOAD_TOO_LARGE", 413], ["UNSUPPORTED_MEDIA_TYPE", 415], ["UNPROCESSABLE_ENTITY", 422],
  ["PRECONDITION_REQUIRED", 428], ["RATE_LIMITED", 429], ["REQUEST_HEADER_FIELDS_TOO_LARGE", 431],
  ["INTERNAL_SERVER_ERROR", 500], ["NOT_IMPLEMENTED", 501], ["BAD_GATEWAY", 502], ["SERVICE_UNAVAILABLE", 503],
  ["GATEWAY_TIMEOUT", 504],
];

const CONNECT_CODES: Readonly<Record<number, string>> = {
  [Code.Unauthenticated]: "UNAUTHORIZED",
  [Code.PermissionDenied]: "FORBIDDEN",
  [Code.NotFound]: "RESOURCE_NOT_FOUND",
  [Code.AlreadyExists]: "RESOURCE_ALREADY_EXISTS",
  [Code.ResourceExhausted]: "RATE_LIMITED",
  [Code.FailedPrecondition]: "PRECONDITION_FAILED",
  [Code.InvalidArgument]: "VALIDATION_FAILED",
  [Code.Unavailable]: "SERVICE_UNAVAILABLE",
  [Code.DeadlineExceeded]: "GATEWAY_TIMEOUT",
  [Code.Unimplemented]: "NOT_IMPLEMENTED",
  [Code.Aborted]: "CONFLICT",
  [Code.OutOfRange]: "UNPROCESSABLE_ENTITY",
};

function platformOfProblem(type: unknown, status: unknown): string {
  const slug = typeof type === "string" ? type.split(/[:/]/).pop() : undefined;
  const bySlug = PLATFORM_CODES.find(([code]) => code.toLowerCase().replaceAll("_", "-") === slug);
  if (bySlug) return bySlug[0];
  const byStatus = PLATFORM_CODES.find(([, http]) => http === status);
  if (byStatus) return byStatus[0];
  return typeof status === "number" && status >= 400 && status < 500 ? "BAD_REQUEST" : "INTERNAL_SERVER_ERROR";
}

interface Identity {
  readonly platform: string;
  readonly code?: string;
  readonly emitter?: string;
  readonly rule?: string;
  readonly violation: boolean;
  readonly args: ErrorArgs | ErrorArgError;
}

const VIOLATION = Symbol.for("@brefwiz/api-bones-connect/error-localization/violation");

function violation(field: string, message: string, rule: string, args: ErrorArgs | ErrorArgError): Violation {
  return { field, rule, message, args, [VIOLATION]: true } as Violation;
}

function problemInfo(body: ProblemJson): ErrorInfo | undefined {
  const raw = body.error_info;
  if (!isObject(raw) || typeof raw.code !== "string") return undefined;
  if (raw.emitter !== undefined && typeof raw.emitter !== "string") return undefined;
  const args = jsonArgs(raw.args);
  if (args instanceof ErrorArgError) return undefined;
  return { code: raw.code, emitter: raw.emitter ?? "", args };
}

function problemViolation(member: unknown): Violation {
  const text = (name: string): string | undefined =>
    isObject(member) && typeof member[name] === "string" ? (member[name] as string) : undefined;
  const parsable =
    isObject(member) &&
    typeof member.field === "string" &&
    typeof member.message === "string" &&
    (member.rule === undefined || typeof member.rule === "string");
  const args = parsable ? jsonArgs((member as Record<string, unknown>).args) : new ErrorArgError("malformed");
  return violation(text("field") ?? "", text("message") ?? "", text("rule") ?? "", args);
}

function isViolation(value: unknown): value is Violation {
  return isObject(value) && (value as Record<symbol, unknown>)[VIOLATION] === true;
}

function isGeneratedFailure(value: unknown): value is GeneratedValidationFailure {
  return isObject(value) && value.$typeName === VALIDATION_FAILURE_TYPE;
}

function identityOf(error: Localizable): Identity {
  if (error instanceof ConnectError) {
    const platform = CONNECT_CODES[error.code] ?? "INTERNAL_SERVER_ERROR";
    const info = errorInfoOf(error);
    return info
      ? { platform, code: info.code, emitter: info.emitter, violation: false, args: info.args ?? new Map() }
      : { platform, violation: false, args: new Map() };
  }
  if (isViolation(error)) return { platform: "VALIDATION_FAILED", rule: error.rule, violation: true, args: error.args };
  if (isGeneratedFailure(error)) {
    return { platform: "VALIDATION_FAILED", rule: error.rule, violation: true, args: generatedArgs(error.args) };
  }
  const body = error as ProblemJson;
  const platform = platformOfProblem(body.type, body.status);
  const info = problemInfo(body);
  return info
    ? { platform, code: info.code, emitter: info.emitter, violation: false, args: info.args ?? new Map() }
    : { platform, violation: false, args: new Map() };
}

/** The validation failures of a refused call, in the order the error carries them. */
export function violationsOf(error: ConnectError | ProblemJson): Violation[] {
  if (error instanceof ConnectError) {
    const found: Violation[] = [];
    for (const detail of error.details) {
      const { type, value } = detail as { type?: unknown; value?: unknown };
      if (typeof type !== "string" || !(value instanceof Uint8Array)) continue;
      const name = type.startsWith("type.googleapis.com/") ? type.slice("type.googleapis.com/".length) : type;
      if (name !== VALIDATION_FAILURE_TYPE) continue;
      const failure = decodeValidationFailureKept(value);
      if (failure) found.push(violation(failure.field, failure.message, failure.rule, failure.args));
    }
    return found;
  }
  return Array.isArray(error.errors) ? error.errors.map(problemViolation) : [];
}

// ---------------------------------------------------------------------------
// Catalogs.

const REGISTRY_KEY = Symbol.for("@brefwiz/api-bones-connect/error-catalogs/v1");
type Entries = Readonly<Record<string, string>>;
type Slot = { readonly entries: Entries } | { readonly conflict: true };
type Registry = Map<string, Slot>;

function registry(): Registry {
  const store = globalThis as unknown as Record<symbol, Registry | undefined>;
  return (store[REGISTRY_KEY] ??= new Map());
}

function sameEntries(a: Entries, b: Entries): boolean {
  const keys = Object.keys(a);
  return keys.length === Object.keys(b).length && keys.every((key) => own(b, key) && a[key] === b[key]);
}

/** A service's chunk for one locale: a dynamic `import()` of its catalog JSON module. */
export type ErrorCatalogLoader = () => Promise<{ readonly default: Readonly<Record<string, string>> }>;

function settle(emitter: string, locale: string, raw: Record<string, unknown>): void {
  const prefix = `${emitter}.errors.`;
  const entries: Record<string, string> = {};
  for (const [key, pattern] of Object.entries(raw)) {
    if (key.startsWith(prefix) && typeof pattern === "string") entries[key] = pattern;
  }
  const slotKey = JSON.stringify([emitter, locale]);
  const slot = registry().get(slotKey);
  if (!slot) registry().set(slotKey, { entries });
  else if ("entries" in slot && !sameEntries(slot.entries, entries)) registry().set(slotKey, { conflict: true });
}

// Loading state lives on `globalThis` beside the catalog registry, so every copy
// of the package in one realm agrees on what is loaded and what is pending.
const LOADING_KEY = Symbol.for("@brefwiz/api-bones-connect/error-catalog-loading/v1");

interface Loading {
  /** api-bones's own catalogs by locale; `en` ships with the module. */
  readonly bones: Map<string, Entries>;
  /** Registered but unloaded service chunks: locale to emitter to loaders. */
  readonly pending: Map<string, Map<string, Set<ErrorCatalogLoader>>>;
  /** Fetches under way, by `bones:<locale>` or by loader, so each runs once. */
  readonly running: Map<string | ErrorCatalogLoader, Promise<void>>;
  /** The promise handed out for a locale while its fetches are under way. */
  readonly calls: Map<string, { readonly jobs: ReadonlyArray<string | ErrorCatalogLoader>; readonly promise: Promise<void> }>;
}

function loading(): Loading {
  const store = globalThis as unknown as Record<symbol, Loading | undefined>;
  return (store[LOADING_KEY] ??= {
    bones: new Map<string, Entries>([[DEFAULT_LOCALE, BONES_EN]]),
    pending: new Map(),
    running: new Map(),
    calls: new Map(),
  });
}

const isPending = (emitter: string, locale: string): boolean =>
  (loading().pending.get(locale)?.get(emitter)?.size ?? 0) > 0;

/**
 * Register an emitter's catalogs, `locale` to `key` to pattern. Only keys
 * prefixed `<emitter>.errors.` and locale names of the platform are kept; the
 * same entries again are a no-op and different entries make that locale unusable.
 * A locale may instead be a loader, a dynamic `import()` of the catalog, fetched
 * by {@link loadErrorCatalog} so a page carries only the locales it renders.
 */
export function registerErrorCatalogs(
  emitter: string,
  catalogs: Readonly<Record<string, Readonly<Record<string, string>> | ErrorCatalogLoader>>,
): void {
  if (emitter === "" || emitter === "bones") return;
  for (const [locale, raw] of Object.entries(catalogs)) {
    if (!PLATFORM_LOCALES.includes(locale)) continue;
    if (typeof raw === "function") {
      const byEmitter = loading().pending.get(locale) ?? new Map<string, Set<ErrorCatalogLoader>>();
      const loaders = byEmitter.get(emitter) ?? new Set<ErrorCatalogLoader>();
      loaders.add(raw);
      byEmitter.set(emitter, loaders);
      loading().pending.set(locale, byEmitter);
    } else if (isObject(raw)) {
      settle(emitter, locale, raw);
    }
  }
}

function once(key: string | ErrorCatalogLoader, run: () => Promise<void>): Promise<void> {
  const { running } = loading();
  const started = running.get(key);
  if (started) return started;
  const job = (async () => {
    try {
      await run();
    } finally {
      running.delete(key);
    }
  })();
  running.set(key, job);
  return job;
}

const loadBones = (locale: string): Promise<void> =>
  once(`bones:${locale}`, async () => {
    loading().bones.set(locale, (await LOADERS[locale]()).default);
  });

const loadService = (emitter: string, locale: string, loader: ErrorCatalogLoader): Promise<void> =>
  once(loader, async () => {
    const raw: unknown = (await loader()).default;
    if (!isObject(raw)) throw new TypeError(`the ${locale} error catalog of ${emitter} has no default catalog object`);
    settle(emitter, locale, raw);
    loading().pending.get(locale)?.get(emitter)?.delete(loader);
  });

/**
 * Make `locale` (a BCP 47 tag or an Accept-Language value, negotiated over the
 * platform locales) renderable by {@link localize}: one call fetches the
 * api-bones catalog and every registered service's catalog for it, and `en`
 * of each service, the end of the fallback chain. Idempotent: concurrent and
 * repeated calls share one fetch, `en` of api-bones is always ready, and a
 * service registered after a locale loaded is fetched by the next call. Rejects
 * when a chunk fails to load, and a later call retries.
 */
export function loadErrorCatalog(locale: string): Promise<void> {
  const viewer = negotiateLocale(locale);
  const state = loading();
  const jobs: Array<string | ErrorCatalogLoader> = [];
  const starts: Array<() => Promise<void>> = [];
  for (const wanted of viewer === DEFAULT_LOCALE ? [DEFAULT_LOCALE] : [viewer, DEFAULT_LOCALE]) {
    if (!state.bones.has(wanted)) {
      jobs.push(`bones:${wanted}`);
      starts.push(() => loadBones(wanted));
    }
    for (const [emitter, loaders] of state.pending.get(wanted) ?? []) {
      for (const loader of loaders) {
        jobs.push(loader);
        starts.push(() => loadService(emitter, wanted, loader));
      }
    }
  }
  if (jobs.length === 0) return Promise.resolve();
  const call = state.calls.get(viewer);
  if (call && call.jobs.length === jobs.length && call.jobs.every((job, i) => job === jobs[i])) return call.promise;
  const promise: Promise<void> = Promise.all(starts.map((start) => start())).then(
    () => undefined,
  );
  const release = (): void => {
    if (state.calls.get(viewer)?.promise === promise) state.calls.delete(viewer);
  };
  promise.then(release, release);
  state.calls.set(viewer, { jobs, promise });
  return promise;
}

function entry(emitter: string, locale: string, key: string): string | undefined {
  if (emitter === "bones") {
    const catalog = loading().bones.get(locale);
    return catalog && own(catalog, key) ? catalog[key] : undefined;
  }
  const slot = registry().get(JSON.stringify([emitter, locale]));
  return slot && "entries" in slot && own(slot.entries, key) ? slot.entries[key] : undefined;
}

// ---------------------------------------------------------------------------
// MessageFormat subset (error-localization C6) and formatting (C7).

const MAX_DEPTH = 16;
const MAX_EXACT = 9_007_199_254_740_991n;
const DATE_STYLES = ["short", "medium", "long", "full"];
const TIME_STYLES = ["short", "medium"];
const PLURAL_SELECTOR = /^(=\d+|zero|one|two|few|many|other)$/;

type Value = ErrorArg;

class Unresolved extends Error {}

const unresolved = (): never => {
  throw new Unresolved();
};

function need(args: ErrorArgs, name: string, kinds: ReadonlyArray<Value["kind"]>): Value {
  const value = args.get(name);
  return value && kinds.includes(value.kind) ? value : unresolved();
}

/** The category of the exact integer; residues past 2^53 survive the reduction. */
function pluralCategory(rules: Intl.PluralRules, value: Value): string {
  if (value.kind === "double") return rules.select(value.value);
  if (value.kind !== "int64") return unresolved();
  const magnitude = value.value < 0n ? -value.value : value.value;
  return rules.select(magnitude <= MAX_EXACT ? Number(magnitude) : 1_000_000_000 + Number(magnitude % 1_000_000n));
}

function exactMatches(selector: string, value: Value): boolean {
  const wanted = BigInt(selector.slice(1));
  if (wanted > MAX_EXACT) return unresolved();
  return value.kind === "int64" ? value.value === wanted : value.kind === "double" && value.value === Number(wanted);
}

function flatten(
  nodes: readonly MessageFormatElement[],
  args: ErrorArgs,
  rules: Intl.PluralRules,
  depth: number,
  pound: string | undefined,
): MessageFormatElement[] {
  if (depth > MAX_DEPTH) return unresolved();
  const out: MessageFormatElement[] = [];
  const inner = (children: readonly MessageFormatElement[], at: string | undefined) =>
    out.push(...flatten(children, args, rules, depth + 1, at));
  for (const node of nodes) {
    switch (node.type) {
      case TYPE.literal:
        out.push(node);
        break;
      case TYPE.argument:
        need(args, node.value, ["string"]);
        out.push(node);
        break;
      case TYPE.number:
        if (node.style !== null && node.style !== "percent") unresolved();
        need(args, node.value, ["int64", "double"]);
        out.push(node);
        break;
      case TYPE.date:
        if (typeof node.style !== "string" || !DATE_STYLES.includes(node.style)) unresolved();
        need(args, node.value, ["timestamp"]);
        out.push({ ...node, style: `explicit-${node.style}` });
        break;
      case TYPE.time:
        if (typeof node.style !== "string" || !TIME_STYLES.includes(node.style)) unresolved();
        need(args, node.value, ["timestamp"]);
        out.push({ ...node, style: `explicit-${node.style}` });
        break;
      case TYPE.select: {
        const value = need(args, node.value, ["string"]);
        const text = value.kind === "string" ? value.value : "";
        const chosen = own(node.options, text) ? node.options[text] : node.options.other;
        inner((chosen ?? unresolved()).value, pound);
        break;
      }
      case TYPE.plural: {
        if (node.pluralType !== "cardinal" || node.offset !== 0) unresolved();
        const keys = Object.keys(node.options);
        if (!keys.every((key) => PLURAL_SELECTOR.test(key))) unresolved();
        const value = need(args, node.value, ["int64", "double"]);
        const exact = keys.find((key) => key.startsWith("=") && exactMatches(key, value));
        const chosen = node.options[exact ?? pluralCategory(rules, value)] ?? node.options.other;
        inner((chosen ?? unresolved()).value, node.value);
        break;
      }
      case TYPE.pound:
        out.push(
          pound === undefined
            ? { type: TYPE.literal, value: "#" }
            : { type: TYPE.number, value: pound, style: null },
        );
        break;
      default:
        unresolved();
    }
  }
  return out;
}

function formatValue(value: Value): string | number | bigint | Date {
  switch (value.kind) {
    case "string":
    case "double":
      return value.value;
    case "int64":
      return value.value >= -MAX_EXACT && value.value <= MAX_EXACT ? Number(value.value) : value.value;
    case "timestamp":
      return new Date(Number(value.seconds) * 1000 + Math.floor(value.nanos / 1_000_000));
  }
}

function render(pattern: string, locale: string, args: ErrorArgs, timeZone: string): string | undefined {
  try {
    const ast = parse(pattern, { ignoreTag: true, requiresOtherClause: true });
    const flat = flatten(ast, args, new Intl.PluralRules(locale), 0, undefined);
    const formats: Partial<Formats> = {
      date: Object.fromEntries(DATE_STYLES.map((style) => [`explicit-${style}`, { dateStyle: style, timeZone }])) as Formats["date"],
      time: Object.fromEntries(TIME_STYLES.map((style) => [`explicit-${style}`, { timeStyle: style, timeZone }])) as Formats["time"],
    };
    const values = Object.fromEntries([...args].map(([name, value]) => [name, formatValue(value)]));
    return new IntlMessageFormat(flat, locale, formats, { ignoreTag: true }).format(values) as string;
  } catch (error) {
    if (error instanceof Unresolved || error instanceof Error) return undefined;
    throw error;
  }
}

function zoneOf(options: { timeZone?: string } | undefined): string {
  const zone = options?.timeZone;
  if (zone === undefined) return "UTC";
  if (zone === "" || /^[+-]/.test(zone)) throw new LocalizeOptionsError();
  try {
    return new Intl.DateTimeFormat("en", { timeZone: zone }).resolvedOptions().timeZone;
  } catch {
    throw new LocalizeOptionsError();
  }
}

/**
 * Render `error` for a viewer: `locale` is a BCP 47 tag or an Accept-Language
 * value, negotiated over the platform locales. The last key of the chain
 * always resolves, so neither the raw code nor the operator's message is
 * rendered.
 *
 * @throws {@link LocalizeOptionsError} for an unknown or empty time zone, or
 * (`locale_not_loaded`) for a platform locale other than `en` whose catalog
 * {@link loadErrorCatalog} has not yet loaded, and for an emitter registered
 * with a loader whose catalog for the locale has not loaded. Unmatched tags render in `en`.
 */
export function localize(
  error: Localizable,
  locale: string,
  options?: { timeZone?: string },
): { text: string; hint?: string } {
  const timeZone = zoneOf(options);
  const viewer = negotiateLocale(locale);
  if (!loading().bones.has(viewer)) throw new LocalizeOptionsError("locale_not_loaded");
  const id = identityOf(error);
  if (id.emitter && (isPending(id.emitter, viewer) || isPending(id.emitter, DEFAULT_LOCALE))) {
    throw new LocalizeOptionsError("locale_not_loaded");
  }
  const good = !(id.args instanceof ErrorArgError);
  const args: ErrorArgs = id.args instanceof ErrorArgError ? new Map() : id.args;
  const keys: Array<[string, string]> = [];
  if (id.violation) {
    if (good && id.rule) keys.push(["bones", `bones.validation.${id.rule}`]);
    keys.push(["bones", "bones.errors.VALIDATION_FAILED"]);
  } else {
    if (good && id.code) {
      if (id.emitter) keys.push([id.emitter, `${id.emitter}.errors.${id.code}`]);
      keys.push(["bones", `bones.errors.${id.code}`]);
    }
    keys.push(["bones", `bones.errors.${PLATFORM_CODES.some(([code]) => code === id.platform) ? id.platform : "INTERNAL_SERVER_ERROR"}`]);
  }
  const locales = viewer === DEFAULT_LOCALE ? [DEFAULT_LOCALE] : [viewer, DEFAULT_LOCALE];
  for (const [emitter, key] of keys) {
    for (const loc of locales) {
      const title = entry(emitter, loc, `${key}.title`);
      const text = title === undefined ? undefined : render(title, loc, args, timeZone);
      if (text === undefined) continue;
      const description = entry(emitter, loc, `${key}.description`);
      const hint = description === undefined ? undefined : render(description, loc, args, timeZone);
      return hint === undefined ? { text } : { text, hint };
    }
  }
  return { text: "" };
}
