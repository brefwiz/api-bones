// SPDX-License-Identifier: MIT
import { Code, ConnectError } from "@connectrpc/connect";
import { describe, expect, it, vi } from "vitest";

import { encodeErrorInfo, encodeValidationFailure, ERROR_INFO_TYPE, VALIDATION_FAILURE_TYPE } from "./error-info.js";
import {
  DEFAULT_LOCALE,
  errorArgs,
  ErrorArgError,
  loadErrorCatalog,
  LOCALE_ALIASES,
  localize,
  LocalizeOptionsError,
  negotiateLocale,
  PLATFORM_LOCALES,
  registerErrorCatalogs,
  violationsOf as i18nViolationsOf,
} from "./error-localization.js";

const str = (value: string) => ({ kind: "string", value }) as const;
const int = (value: bigint) => ({ kind: "int64", value }) as const;
const rule = (fn: () => unknown) => {
  try {
    fn();
  } catch (error) {
    return (error as ErrorArgError).rule;
  }
  return undefined;
};

describe("negotiateLocale", () => {
  it("matches tags, aliases and Accept-Language values", () => {
    expect(negotiateLocale("fr-CA")).toBe("fr");
    expect(negotiateLocale("pt")).toBe("pt-BR");
    expect(negotiateLocale("zh-TW")).toBe(DEFAULT_LOCALE);
    expect(negotiateLocale("de;q=0.5, fr;q=0.9")).toBe("fr");
    expect(negotiateLocale("de, fr")).toBe("de");
    expect(negotiateLocale("fr;q=0, de;q=0.1")).toBe("de");
    expect(negotiateLocale("*")).toBe("en");
    expect(PLATFORM_LOCALES).toHaveLength(29);
    expect(LOCALE_ALIASES.iw).toBe("he");
  });
});

describe("errorArgs", () => {
  it("reports the first failing pair in input order", () => {
    expect(rule(() => errorArgs({ ratio: { kind: "double", value: Number.NaN }, name: str("\u0007") }))).toBe(
      "non_finite_double",
    );
    expect(rule(() => errorArgs({ name: str("\u0007"), ratio: { kind: "double", value: Number.NaN } }))).toBe(
      "string_forbidden_char",
    );
    expect(rule(() => errorArgs({ Bad: str("x") }))).toBe("arg_key_format");
    expect(rule(() => errorArgs({ n: int(2n ** 63n) }))).toBe("int64_range");
    expect(rule(() => errorArgs({ s: str("é".repeat(513)) }))).toBe("string_too_long");
    expect(rule(() => errorArgs({ t: { kind: "timestamp", seconds: 0n, nanos: 1e9 } }))).toBe("timestamp_range");
  });

  it("bounds the count at 32 and sorts keys", () => {
    const many = Object.fromEntries(Array.from({ length: 33 }, (_, n) => [`k${n + 1}`, int(1n)]));
    expect(rule(() => errorArgs(many))).toBe("too_many_args");
    expect([...errorArgs({ b: int(1n), a: int(2n) }).keys()]).toEqual(["a", "b"]);
  });
});

function connect(code: Code, code_: string, emitter: string, args: ReturnType<typeof errorArgs>): ConnectError {
  const err = new ConnectError("operator text", code);
  const value = encodeErrorInfo({ code: code_, emitter, args });
  err.details.push({ type: ERROR_INFO_TYPE, value } as never);
  return err;
}

describe("localize", () => {
  registerErrorCatalogs("ut", {
    en: {
      "ut.errors.SEATS.title": "{count, plural, =0 {No seats} one {# seat} other {# seats}} on {when, date, long} at {when, time, medium}",
      "ut.errors.SEATS.description": "Owner {owner, select, admin {is admin} other {is {owner}}}",
      "ut.errors.NEEDS.title": "Need {n, number}",
    },
  });

  it("renders the platform code of a Connect code", async () => {
    await loadErrorCatalog("fr");
    expect(localize(new ConnectError("x", Code.NotFound), "en").text).toBe("Not found");
    expect(localize(new ConnectError("x", Code.NotFound), "fr").text).not.toBe("Not found");
  });

  it("renders an emitter key with exact int64, dates in a zone and select", () => {
    const args = errorArgs({
      count: int(9007199254740993n),
      when: { kind: "timestamp", seconds: 1_700_000_000n, nanos: 0 },
      owner: str("constructor"),
    });
    const out = localize(connect(Code.PermissionDenied, "SEATS", "ut", args), "en", { timeZone: "Asia/Tokyo" });
    expect(out.text.replaceAll(/\s/g, " ")).toBe("9,007,199,254,740,993 seats on November 15, 2023 at 7:13:20 AM");
    expect(out.hint).toBe("Owner is constructor");
  });

  it("falls through to the platform key when an argument does not fit", () => {
    const wrong = connect(Code.PermissionDenied, "NEEDS", "ut", errorArgs({ n: str("3") }));
    expect(localize(wrong, "en").text).toBe("Access denied");
    expect(localize(connect(Code.PermissionDenied, "NEEDS", "ut", errorArgs({ n: int(1234n) })), "en").text).toBe(
      "Need 1,234",
    );
  });

  it("refuses an unknown time zone", () => {
    expect(() => localize(new ConnectError("x", Code.Internal), "en", { timeZone: "Mars/Base" })).toThrow(
      LocalizeOptionsError,
    );
    expect(() => localize(new ConnectError("x", Code.Internal), "en", { timeZone: "" })).toThrow(LocalizeOptionsError);
  });

  it("decodes problem+json strictly", () => {
    const body = (args: unknown) => ({
      type: "urn:api-bones:error:conflict",
      status: 409,
      error_info: { code: "NEEDS", emitter: "ut", args },
    });
    expect(localize(body({ n: { int64Value: "1234" } }), "en").text).toBe("Need 1,234");
    for (const bad of [{ n: { int64Value: 1234 } }, { n: { doubleValue: "NaN" } }, { n: { int64Value: "01" } }]) {
      expect(localize(body(bad), "en").text).toBe("Conflict");
    }
    expect(localize({ status: 418 }, "en").text).toBe("Invalid request");
    expect(localize({ status: 599 }, "en").text).toBe("Server error");
  });

  it("lists and renders validation failures", () => {
    const err = new ConnectError("x", Code.InvalidArgument);
    const value = encodeValidationFailure({ field: "/n", message: "m", rule: "max", args: errorArgs({ max: int(10n) }) });
    err.details.push({ type: VALIDATION_FAILURE_TYPE, value } as never);
    const [violation] = i18nViolationsOf(err);
    expect(localize(violation, "en").text).toBe("Must be at most 10.");
    expect(localize({ ...violation, args: new ErrorArgError("malformed") } as never, "en").text).toBeDefined();
  });
});

describe("loadErrorCatalog", () => {
  const notFound = new ConnectError("x", Code.NotFound);
  const notLoaded = (fn: () => unknown) => {
    try {
      fn();
    } catch (error) {
      return (error as LocalizeOptionsError).rule;
    }
    return undefined;
  };

  it("refuses to render a platform locale whose catalog is not loaded", () => {
    expect(notLoaded(() => localize(notFound, "es"))).toBe("locale_not_loaded");
    expect(notLoaded(() => localize(notFound, "pt-BR"))).toBe("locale_not_loaded");
    expect(notLoaded(() => localize(notFound, "es-MX,en"))).toBe("locale_not_loaded");
  });

  it("renders a locale after it loads, from a tag or an Accept-Language value", async () => {
    await loadErrorCatalog("it-IT");
    const italian = localize(notFound, "it").text;
    expect(italian).not.toBe("Not found");
    expect(italian).not.toBe("");
    expect(localize(notFound, "it;q=0.9, de;q=0.1").text).toBe(italian);
  });

  it("renders unmatched tags and en in English without loading", async () => {
    await expect(loadErrorCatalog("en")).resolves.toBeUndefined();
    await expect(loadErrorCatalog("xx")).resolves.toBeUndefined();
    expect(localize(notFound, "xx").text).toBe("Not found");
    expect(localize(notFound, "*").text).toBe("Not found");
    expect(localize(notFound, "en").text).toBe("Not found");
  });

  it("shares one fetch per locale and is idempotent", async () => {
    const first = loadErrorCatalog("sv");
    expect(loadErrorCatalog("sv-SE")).toBe(first);
    await first;
    await expect(loadErrorCatalog("sv")).resolves.toBeUndefined();
    expect(notLoaded(() => localize(notFound, "sv"))).toBeUndefined();
  });

  it("rejects with the import error when a chunk fails and retries on the next call", async () => {
    vi.resetModules();
    let attempts = 0;
    vi.doMock("./error-catalogs/nl.generated.js", () => {
      attempts += 1;
      if (attempts === 1) throw new Error("chunk unavailable");
      return { default: { "bones.errors.RESOURCE_NOT_FOUND.title": "Niet gevonden" } };
    });
    try {
      const fresh = await import("./error-localization.js");
      await expect(fresh.loadErrorCatalog("nl")).rejects.toBeInstanceOf(Error);
      expect(notLoaded(() => fresh.localize(notFound, "nl"))).toBe("locale_not_loaded");
      await fresh.loadErrorCatalog("nl");
      expect(fresh.localize(notFound, "nl").text).toBe("Niet gevonden");
    } finally {
      vi.doUnmock("./error-catalogs/nl.generated.js");
      vi.resetModules();
    }
  });
});

describe("lazy error catalogs", () => {
  const raised = (emitter: string, code = "LZ_GONE") => connect(Code.NotFound, code, emitter, errorArgs({}));
  const ruleOf = (fn: () => unknown) => {
    try {
      fn();
    } catch (error) {
      return (error as LocalizeOptionsError).rule;
    }
    return undefined;
  };
  const chunk = (emitter: string, text: string) => async () => ({ default: { [`${emitter}.errors.LZ_GONE.title`]: text } });

  it("one call loads api-bones and every registered service for the locale", async () => {
    const one = vi.fn(chunk("lz-a", "A pl"));
    const two = vi.fn(chunk("lz-b", "B pl"));
    registerErrorCatalogs("lz-a", { pl: one });
    registerErrorCatalogs("lz-b", { pl: two });
    expect(ruleOf(() => localize(raised("lz-a"), "pl"))).toBe("locale_not_loaded");
    expect(ruleOf(() => localize(raised("lz-b"), "pl"))).toBe("locale_not_loaded");
    const first = loadErrorCatalog("pl-PL");
    expect(loadErrorCatalog("pl")).toBe(first);
    await first;
    expect(one).toHaveBeenCalledTimes(1);
    expect(two).toHaveBeenCalledTimes(1);
    expect(localize(raised("lz-a"), "pl").text).toBe("A pl");
    expect(localize(raised("lz-b"), "pl").text).toBe("B pl");
    expect(localize(new ConnectError("x", Code.NotFound), "pl").text).toBe("Nie znaleziono");
    await loadErrorCatalog("pl");
    expect(one).toHaveBeenCalledTimes(1);
  });

  it("loads a service registered after the locale loaded on the next call", async () => {
    await loadErrorCatalog("hu");
    const late = vi.fn(chunk("lz-late", "Késői"));
    registerErrorCatalogs("lz-late", { hu: late });
    expect(ruleOf(() => localize(raised("lz-late"), "hu"))).toBe("locale_not_loaded");
    expect(ruleOf(() => localize(new ConnectError("x", Code.NotFound), "hu"))).toBeUndefined();
    await loadErrorCatalog("hu");
    expect(late).toHaveBeenCalledTimes(1);
    expect(localize(raised("lz-late"), "hu").text).toBe("Késői");
  });

  it("rejects with the import error when a service chunk fails and retries", async () => {
    let attempts = 0;
    registerErrorCatalogs("lz-flaky", {
      ro: async () => {
        attempts += 1;
        if (attempts === 1) throw new Error("service chunk unavailable");
        return { default: { "lz-flaky.errors.LZ_GONE.title": "Dispărut" } };
      },
    });
    await expect(loadErrorCatalog("ro")).rejects.toThrow("service chunk unavailable");
    expect(ruleOf(() => localize(raised("lz-flaky"), "ro"))).toBe("locale_not_loaded");
    await loadErrorCatalog("ro");
    expect(attempts).toBe(2);
    expect(localize(raised("lz-flaky"), "ro").text).toBe("Dispărut");
  });

  it("rejects a chunk that is not a catalog object", async () => {
    registerErrorCatalogs("lz-bad", { tr: (async () => ({ default: 7 })) as never });
    await expect(loadErrorCatalog("tr")).rejects.toBeInstanceOf(TypeError);
  });

  it("loads the en chunk of a service with any other locale and keeps keys of the emitter only", async () => {
    registerErrorCatalogs("lz-en", {
      en: async () => ({ default: { "lz-en.errors.LZ_GONE.title": "Gone", "other.errors.LZ_GONE.title": "Hijack" } }),
      ja: chunk("lz-en", "消えた"),
    });
    expect(ruleOf(() => localize(raised("lz-en"), "en"))).toBe("locale_not_loaded");
    await loadErrorCatalog("ja");
    expect(localize(raised("lz-en"), "ja").text).toBe("消えた");
    expect(localize(raised("lz-en"), "en").text).toBe("Gone");
    expect(localize(raised("other"), "en").text).toBe("Not found");
  });

  it("keeps eager and lazy registrations of one emitter conflict-checked", async () => {
    registerErrorCatalogs("lz-mix", { ko: { "lz-mix.errors.LZ_GONE.title": "하나" } });
    registerErrorCatalogs("lz-mix", { ko: chunk("lz-mix", "둘") });
    await loadErrorCatalog("ko");
    expect(localize(raised("lz-mix"), "ko").text).toBe("찾을 수 없어요");
  });

  it("agrees between two copies of the package in one realm", async () => {
    vi.resetModules();
    const copy = await import("./error-localization.js");
    expect(copy.loadErrorCatalog).not.toBe(loadErrorCatalog);
    const err = raised("lz-copy");
    registerErrorCatalogs("lz-copy", { sk: chunk("lz-copy", "Preč") });
    expect(ruleOf(() => copy.localize(err, "sk"))).toBe("locale_not_loaded");
    await copy.loadErrorCatalog("sk");
    expect(localize(err, "sk").text).toBe("Preč");
    expect(copy.localize(new ConnectError("x", Code.NotFound), "sk").text).not.toBe("Not found");
    expect(ruleOf(() => localize(new ConnectError("x", Code.NotFound), "sk"))).toBeUndefined();
    vi.resetModules();
  });
});
