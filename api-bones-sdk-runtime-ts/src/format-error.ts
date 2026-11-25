// SPDX-License-Identifier: MIT
import { IntlMessageFormat } from "intl-messageformat";
import type { BonesError } from "./errors.js";

/** The ICU messages of one emitter in one locale, keyed `<emitter>.errors.<CODE>.title` and `.description`. */
export type ErrorCatalog = Readonly<Record<string, string>>;

/** The entries of one (emitter, locale) slot; `null` once two different registrations disagreed. */
const slots = new Map<string, ErrorCatalog | null>();

const slotOf = (emitter: string, locale: string): string => JSON.stringify([emitter, locale]);

const byKey = ([a]: readonly [string, string], [b]: readonly [string, string]): number => (a < b ? -1 : a > b ? 1 : 0);

const sameEntries = (a: ErrorCatalog, b: ErrorCatalog): boolean =>
  JSON.stringify(Object.entries(a).sort(byKey)) === JSON.stringify(Object.entries(b).sort(byKey));

/**
 * Adds an emitter's messages for one locale. Only keys prefixed `<emitter>.errors.` are kept; the same entries again change nothing, different ones make that emitter's locale unusable.
 */
export function registerErrorCatalog(emitter: string, locale: string, messages: ErrorCatalog): void {
  if (emitter === "") return;
  const prefix = `${emitter}.errors.`;
  const kept: ErrorCatalog = Object.fromEntries(Object.entries(messages).filter(([key]) => key.startsWith(prefix)));
  const slot = slotOf(emitter, locale);
  const held = slots.get(slot);
  if (held === undefined) slots.set(slot, kept);
  else if (held !== null && !sameEntries(held, kept)) slots.set(slot, null);
}

/** What {@link formatError} reads of an error. */
export type FormattableError = Pick<BonesError, "emitter" | "code" | "platform" | "args">;

/** What a caller may pin when formatting. */
export interface FormatErrorOptions {
  /** The IANA time zone dates and times render in. Defaults to `UTC`. */
  readonly timeZone?: string;
}

/** The option is not a time zone the platform knows. */
export class FormatErrorOptionError extends Error {
  override name = "FormatErrorOptionError";
  readonly rule = "unknown_time_zone" as const;

  constructor() {
    super("option refused: unknown_time_zone");
  }
}

/** A formatted error: the text and, when its catalog has one, the longer hint. */
export interface FormattedError {
  readonly text: string;
  readonly hint?: string;
}

function checkTimeZone(timeZone: string): void {
  if (timeZone === "" || timeZone.startsWith("+") || timeZone.startsWith("-")) throw new FormatErrorOptionError();
  try {
    Intl.DateTimeFormat("en", { timeZone });
  } catch {
    throw new FormatErrorOptionError();
  }
}

/**
 * The error's message in `locale`, falling back to `en`, then to the platform text of its code; `text` is empty only while no platform catalog is registered.
 */
export function formatError(error: FormattableError, locale: string, options: FormatErrorOptions = {}): FormattedError {
  const timeZone = options.timeZone ?? "UTC";
  checkTimeZone(timeZone);
  const render = (pattern: string | undefined, tag: string): string | undefined => {
    if (pattern === undefined) return undefined;
    try {
      const format = new IntlMessageFormat(pattern, tag, undefined, {
        formatters: {
          // The message-format option type admits `trailingZeroDisplay` values newer than this build's lib typing; the runtime honors them.
          getNumberFormat: (l, o) => new Intl.NumberFormat(l, o as Intl.NumberFormatOptions | undefined),
          getPluralRules: (l, o) => new Intl.PluralRules(l, o),
          getDateTimeFormat: (l, o) => new Intl.DateTimeFormat(l, { ...o, timeZone }),
        },
      });
      return String(format.format({ ...error.args }));
    } catch {
      return undefined;
    }
  };
  const tags = locale === "en" ? ["en"] : [locale, "en"];
  const owners: ReadonlyArray<readonly [string, string]> = [
    [error.emitter, error.code],
    ["bones", error.code],
    ["bones", error.platform],
    ["bones", "INTERNAL_SERVER_ERROR"],
  ];
  const tried = new Set<string>();
  for (const [owner, code] of owners) {
    const key = `${owner}.errors.${code}`;
    if (owner === "" || code === "" || tried.has(key)) continue;
    tried.add(key);
    for (const tag of tags) {
      const entries = slots.get(slotOf(owner, tag));
      const text = render(entries?.[`${key}.title`], tag);
      if (text === undefined) continue;
      const hint = render(entries?.[`${key}.description`], tag);
      return hint === undefined ? { text } : { text, hint };
    }
  }
  return { text: "" };
}
