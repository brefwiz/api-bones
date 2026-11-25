// SPDX-License-Identifier: MIT
import { readFileSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { beforeAll, describe, expect, it } from "vitest";
import { BonesError, FormatErrorOptionError, formatError, registerErrorCatalog } from "../src/index.js";
import { filesUnder } from "./files-under.js";

const packageRoot = fileURLToPath(new URL("..", import.meta.url));

function failure(code: string, args: BonesError["args"] = {}, emitter = "example", platform = "NOT_FOUND"): BonesError {
  return new BonesError({ code, emitter, platform, connectCode: "not_found", message: "diagnostic text", args });
}

/** What `formatError` throws for a refused time zone, or `undefined` when it formats. */
function refusal(timeZone: string): unknown {
  try {
    formatError(failure("CUSTOMER_MISSING"), "en", { timeZone });
  } catch (error) {
    return error;
  }
  return undefined;
}

const bonesEn = {
  "bones.errors.NOT_FOUND.title": "Not found",
  "bones.errors.SERVICE_UNAVAILABLE.title": "Service unavailable",
  "bones.errors.SERVICE_UNAVAILABLE.description": "Try again later.",
  "bones.errors.INTERNAL_SERVER_ERROR.title": "Something went wrong",
  "bones.errors.SHARED_CODE.title": "Shared by code",
};
const bonesDe = {
  "bones.errors.NOT_FOUND.title": "Nicht gefunden",
  "bones.errors.INTERNAL_SERVER_ERROR.title": "Etwas ist schiefgelaufen",
};
const plainText = (error: BonesError, locale: string, options?: { timeZone: string }): string => formatError(error, locale, options).text;

/** Source text with comments removed: prose may name what the code must not do. */
function code(path: string): string {
  return readFileSync(path, "utf8").replace(/\/\*[\s\S]*?\*\/|\/\/.*$/gm, "");
}

// The registry is module-global: these assertions run before any test registers a catalogue.
describe("the package ships no catalogue", () => {
  it("formats to no text in any locale before a catalogue is registered, never to the code or the message", () => {
    for (const locale of ["en", "de", "fr", "en-GB", "it", "none-registered"]) {
      for (const name of ["CUSTOMER_MISSING", "TAGS_LIMIT", "LEAD_STALE", "KEY_MISSING", "A", "B"]) {
        expect(formatError(failure(name), locale)).toStrictEqual({ text: "" });
        expect(formatError(failure(name, {}, "other", "INTERNAL_SERVER_ERROR"), locale)).toStrictEqual({ text: "" });
      }
    }
  });

  it("holds no registration and no catalogue entry in its sources", () => {
    const sources = filesUnder(join(packageRoot, "src"));
    expect(sources.filter((path) => !path.endsWith(".ts")).map((path) => relative(packageRoot, path))).toEqual([]);
    for (const path of sources) {
      const text = code(path);
      expect({ path: relative(packageRoot, path), call: /(?<!function )registerErrorCatalog\(/.exec(text)?.[0] }).toEqual({ path: relative(packageRoot, path), call: undefined });
      expect({ path: relative(packageRoot, path), entry: /\.errors\.[A-Z_]+/.exec(text)?.[0] }).toEqual({ path: relative(packageRoot, path), entry: undefined });
    }
    const manifest = JSON.parse(readFileSync(join(packageRoot, "package.json"), "utf8")) as { files: string[] };
    expect(manifest.files).toEqual(["dist", "LICENSE"]);
  });
});

describe("registerErrorCatalog", () => {
  beforeAll(() => {
    registerErrorCatalog("bones", "en", bonesEn);
    registerErrorCatalog("bones", "de", bonesDe);
  });

  it("keeps only the keys prefixed with its emitter and never lets a service override bones", () => {
    registerErrorCatalog("pf", "en", {
      "pf.errors.A.title": "mine",
      "other.errors.A.title": "theirs",
      "bones.errors.NOT_FOUND.title": "overridden",
    });
    expect(plainText(failure("A", {}, "pf"), "en")).toBe("mine");
    expect(plainText(failure("A", {}, "other"), "en")).toBe("Not found");
    expect(plainText(failure("B", {}, "pf"), "en")).toBe("Not found");
  });

  it("ignores an empty emitter", () => {
    registerErrorCatalog("", "en", { ".errors.CODE.title": "from the empty emitter" });
    expect(plainText(failure("CODE", {}, ""), "en")).toBe("Not found");
  });

  it("registering the same entries again changes nothing", () => {
    const entries = { "ca.errors.A.title": "A text" };
    registerErrorCatalog("ca", "en", entries);
    registerErrorCatalog("ca", "en", { ...entries });
    expect(plainText(failure("A", {}, "ca"), "en")).toBe("A text");
  });

  it("different entries for one slot make it unusable, whichever came first, and later ones do not revive it", () => {
    for (const [emitter, first, second] of [["cb", "one", "two"], ["cc", "two", "one"]] as const) {
      registerErrorCatalog(emitter, "en", { [`${emitter}.errors.A.title`]: first });
      registerErrorCatalog(emitter, "en", { [`${emitter}.errors.A.title`]: second });
      registerErrorCatalog(emitter, "en", { [`${emitter}.errors.A.title`]: first });
      registerErrorCatalog(emitter, "de", { [`${emitter}.errors.A.title`]: "eins" });
      expect(plainText(failure("A", {}, emitter), "en")).toBe("Not found");
      expect(plainText(failure("A", {}, emitter), "de")).toBe("eins");
    }
  });

  it("different entries under one key are a conflict even when the key sets differ in size", () => {
    registerErrorCatalog("cd", "en", { "cd.errors.A.title": "one" });
    registerErrorCatalog("cd", "en", { "cd.errors.A.title": "one", "cd.errors.B.title": "extra" });
    expect(plainText(failure("A", {}, "cd"), "en")).toBe("Not found");
  });
});

describe("formatError", () => {
  it("resolves <emitter>.errors.<CODE>.title and formats it with the error's arguments", () => {
    registerErrorCatalog("example", "en", { "example.errors.CUSTOMER_MISSING.title": "No customer named {name}." });
    expect(formatError(failure("CUSTOMER_MISSING", { name: "Ada" }), "en")).toStrictEqual({ text: "No customer named Ada." });
  });

  it("formats plurals and numbers in the requested locale", () => {
    registerErrorCatalog("example", "de", {
      "example.errors.TAGS_LIMIT.title": "{count, plural, one {Ein Tag} other {# Tags}} von {limit, number}",
    });
    expect(plainText(failure("TAGS_LIMIT", { count: 3, limit: 1234567n }), "de")).toBe("3 Tags von 1.234.567");
  });

  it("formats dates and times in UTC unless a time zone is given", () => {
    registerErrorCatalog("zone", "en-GB", {
      "zone.errors.DATE.title": "{at, date, short}",
      "zone.errors.TIME.title": "{at, time, short}",
    });
    registerErrorCatalog("zone", "en", {
      "zone.errors.PLAIN.title": "{at, date}",
      "zone.errors.SKELETON.title": "{at, date, ::yMMMd}",
    });
    const at = new Date(Date.UTC(2026, 10, 22, 20));
    const tokyo = { timeZone: "Asia/Tokyo" };
    expect(plainText(failure("DATE", { at }, "zone"), "en-GB")).toBe("22/11/26");
    expect(plainText(failure("DATE", { at }, "zone"), "en-GB", tokyo)).toBe("23/11/26");
    expect(plainText(failure("TIME", { at }, "zone"), "en-GB")).toBe("20:00");
    expect(plainText(failure("TIME", { at }, "zone"), "en-GB", tokyo)).toBe("05:00");
    expect(plainText(failure("PLAIN", { at }, "zone"), "en")).toBe("11/22/2026");
    expect(plainText(failure("PLAIN", { at }, "zone"), "en", tokyo)).toBe("11/23/2026");
    expect(plainText(failure("SKELETON", { at }, "zone"), "en")).toBe("Nov 22, 2026");
    expect(plainText(failure("SKELETON", { at }, "zone"), "en", tokyo)).toBe("Nov 23, 2026");
    expect(plainText(failure("DATE", { at }, "zone"), "en-GB", { timeZone: "UTC" })).toBe("22/11/26");
  });

  it("falls back from the requested locale to en and never negotiates a locale", () => {
    registerErrorCatalog("fb", "en", { "fb.errors.GONE.title": "Gone in en" });
    registerErrorCatalog("fb", "fr", { "fb.errors.GONE.title": "Parti" });
    expect(plainText(failure("GONE", {}, "fb"), "fr")).toBe("Parti");
    expect(plainText(failure("GONE", {}, "fb"), "de")).toBe("Gone in en");
    expect(plainText(failure("GONE", {}, "fb"), "fr-CA")).toBe("Gone in en");
    expect(plainText(failure("GONE", {}, "fb"), "not a locale")).toBe("Gone in en");
  });

  it("walks the keys in order and each key through the locale then en before the next key", () => {
    registerErrorCatalog("km", "en", { "km.errors.X.title": "emitter key, en" });
    expect(plainText(failure("X", {}, "km"), "de")).toBe("emitter key, en");
    expect(plainText(failure("SHARED_CODE", {}, "unlisted"), "en")).toBe("Shared by code");
    expect(plainText(failure("SHARED_CODE", {}, "unlisted"), "de")).toBe("Shared by code");
    expect(plainText(failure("WHATEVER", {}, "unlisted", "NOT_FOUND"), "de")).toBe("Nicht gefunden");
    expect(plainText(failure("WHATEVER", {}, "unlisted", "NOT_FOUND"), "fr")).toBe("Not found");
    expect(plainText(failure("WHATEVER", {}, "unlisted", "NO_SUCH_PLATFORM"), "de")).toBe("Etwas ist schiefgelaufen");
    expect(plainText(failure("WHATEVER", {}, "unlisted", ""), "en")).toBe("Something went wrong");
    expect(plainText(failure("", {}, "", ""), "fr")).toBe("Something went wrong");
    expect(plainText(failure("NOT_FOUND", {}, "bones"), "en")).toBe("Not found");
  });

  it("moves on when an entry does not format", () => {
    registerErrorCatalog("th", "en", {
      "th.errors.NEEDS.title": "Needs {name}",
      "th.errors.BROKEN.title": "{unclosed",
      "th.errors.MIXED.title": "Mixed {name}",
    });
    registerErrorCatalog("th", "fr", { "th.errors.MIXED.title": "Mixte {name" });
    expect(plainText(failure("NEEDS", { name: "x" }, "th"), "en")).toBe("Needs x");
    expect(plainText(failure("NEEDS", {}, "th"), "en")).toBe("Not found");
    expect(plainText(failure("BROKEN", {}, "th"), "en")).toBe("Not found");
    expect(plainText(failure("MIXED", { name: "x" }, "th"), "fr")).toBe("Mixed x");
  });

  it("adds the hint of the slot that supplied the text, when it exists and formats", () => {
    registerErrorCatalog("hn", "en", {
      "hn.errors.A.title": "Title {n}",
      "hn.errors.A.description": "Hint {n}",
      "hn.errors.B.title": "Title",
      "hn.errors.B.description": "Hint {missing}",
    });
    registerErrorCatalog("hn", "de", { "hn.errors.A.title": "Titel {n}" });
    expect(formatError(failure("A", { n: 1 }, "hn"), "en")).toStrictEqual({ text: "Title 1", hint: "Hint 1" });
    expect(formatError(failure("B", {}, "hn"), "en")).toStrictEqual({ text: "Title" });
    expect(formatError(failure("A", { n: 1 }, "hn"), "de")).toStrictEqual({ text: "Titel 1" });
    expect(formatError(failure("X", {}, "unlisted", "SERVICE_UNAVAILABLE"), "en")).toStrictEqual({
      text: "Service unavailable",
      hint: "Try again later.",
    });
    expect(formatError(failure("X", {}, "unlisted", "NOT_FOUND"), "en")).toStrictEqual({ text: "Not found" });
  });
});

describe("the time zone option", () => {
  it.each(["", "+05:00", "-03:00", "Mars/Olympus"])("refuses %j before any lookup", (timeZone) => {
    const refused = refusal(timeZone);
    expect(refused).toBeInstanceOf(FormatErrorOptionError);
    expect(refused).toBeInstanceOf(Error);
    expect(refused).toMatchObject({ name: "FormatErrorOptionError", rule: "unknown_time_zone", message: "option refused: unknown_time_zone" });
  });

  it.each(["UTC", "Asia/Tokyo", "Europe/Paris"])("accepts %j", (timeZone) => {
    expect(refusal(timeZone)).toBeUndefined();
  });
});
