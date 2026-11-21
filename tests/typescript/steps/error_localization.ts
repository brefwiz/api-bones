// SPDX-License-Identifier: MIT
/**
 * The TypeScript half of `tests/features/error_localization.feature`.
 *
 * The Rust half answers the same file; neither keeps its own copy of the rows.
 * Every error is rendered by the package's own `localize`, argument sets are
 * built by `errorArgs` and carried by `encodeErrorInfo`, and a received error whose
 * arguments are out of bounds is put on the wire by the generated codec, which
 * applies no bound.
 */

import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";

import { Given, Then, When, World } from "@cucumber/cucumber";
import { create, fromBinary, toBinary } from "@bufbuild/protobuf";
import { Code } from "@connectrpc/connect";

import {
  ConnectError,
  ERROR_INFO_TYPE,
  ErrorArgError,
  LocalizeOptionsError,
  VALIDATION_FAILURE_TYPE,
  DEFAULT_LOCALE,
  LOCALE_ALIASES,
  PLATFORM_LOCALES,
  encodeErrorInfo,
  errorArgs,
  i18n,
  loadErrorCatalog,
  localize,
  negotiateLocale,
  registerErrorCatalogs,
  type ErrorArg,
  type Localizable,
  type ProblemJson,
  type Violation,
} from "@brefwiz/api-bones-connect";

import {
  ErrorInfoSchema,
  ValidationFailureSchema,
  type ErrorArg as WireErrorArg,
} from "../generated/bones/v1/errors_pb.ts";

const OPERATOR_MESSAGE = "seat limit reached";

const fixtureUrl = (path: string): URL =>
  new URL(`../../features/fixtures/error_localization/${path}`, import.meta.url);

interface LocalizationWorld extends World {
  subject?: Localizable;
  violations?: Violation[];
  localized?: { text: string; hint?: string };
  refused?: string;
  built?: string;
  registry?: { default: string; locales: string[]; aliases: Record<string, string> };
  negotiated?: Array<[string, string, string]>;
  cldrPin?: string;
}

/** Expand `repeat(<text>,<n>)` and `\uXXXX` the way the feature defines them. */
function expand(text: string): string {
  let out = "";
  let rest = text;
  while (rest !== "") {
    const code = /^\\u([0-9a-fA-F]{4})/.exec(rest);
    if (rest.startsWith("repeat(")) {
      const close = rest.indexOf(")");
      const call = rest.slice("repeat(".length, close);
      const comma = call.lastIndexOf(",");
      out += call.slice(0, comma).repeat(Number(call.slice(comma + 1)));
      rest = rest.slice(close + 1);
    } else if (code) {
      out += String.fromCodePoint(Number.parseInt(code[1]!, 16));
      rest = rest.slice(6);
    } else {
      const char = String.fromCodePoint(rest.codePointAt(0)!);
      out += char;
      rest = rest.slice(char.length);
    }
  }
  return out;
}

function parseDouble(value: string): number {
  if (value === "inf") return Number.POSITIVE_INFINITY;
  if (value === "-inf") return Number.NEGATIVE_INFINITY;
  return Number(value);
}

function parseValue(kind: string, value: string): ErrorArg {
  switch (kind) {
    case "string":
      return { kind: "string", value };
    case "int64":
      return { kind: "int64", value: BigInt(value) };
    case "double":
      return { kind: "double", value: parseDouble(value) };
    case "timestamp":
      return { kind: "timestamp", seconds: BigInt(Math.floor(Date.parse(value) / 1000)), nanos: 0 };
    case "timestamp_raw": {
      const [seconds, nanos] = value.split(":");
      return { kind: "timestamp", seconds: BigInt(seconds!), nanos: Number(nanos) };
    }
    default:
      throw new Error(`the contract names an argument type this step cannot build: ${kind}`);
  }
}

/** `key=type:value` items joined by `; `, `(none)` for none, `args(<n>)` for `n` numbered int64 arguments. */
function parseArgs(spec: string): Array<[string, ErrorArg]> {
  if (spec === "(none)") return [];
  const entries: Array<[string, ErrorArg]> = [];
  for (const raw of spec.split("; ")) {
    const numbered = /^args\((\d+)\)$/.exec(raw);
    if (numbered) {
      for (let n = 1; n <= Number(numbered[1]); n++) entries.push([`k${n}`, { kind: "int64", value: BigInt(n) }]);
      continue;
    }
    const item = expand(raw);
    const eq = item.indexOf("=");
    const typed = item.slice(eq + 1);
    const colon = typed.indexOf(":");
    entries.push([item.slice(0, eq), parseValue(typed.slice(0, colon), typed.slice(colon + 1))]);
  }
  return entries;
}

function wireArg(arg: ErrorArg): WireErrorArg["value"] {
  switch (arg.kind) {
    case "string":
      return { case: "string", value: arg.value };
    case "int64":
      return { case: "int64", value: arg.value };
    case "double":
      return { case: "double", value: arg.value };
    case "timestamp":
      return {
        case: "timestamp",
        value: { $typeName: "google.protobuf.Timestamp", seconds: arg.seconds, nanos: arg.nanos },
      };
  }
}

/** The argument map of the generated message, written with no bound applied. */
function wireArgs(spec: string): Record<string, { value: WireErrorArg["value"] }> {
  return Object.fromEntries(parseArgs(spec).map(([key, arg]) => [key, { value: wireArg(arg) }]));
}

/** The Connect code a contract string names, `failed_precondition` being `Code.FailedPrecondition`. */
function connectCodeOf(name: string): Code {
  const pascal = name.replace(/(^|_)([a-z])/g, (_, __, letter: string) => letter.toUpperCase());
  const code = (Code as unknown as Record<string, unknown>)[pascal];
  if (typeof code !== "number") throw new Error(`the contract names a code this step cannot build: ${name}`);
  return code;
}

function failureWith(connectCode: string, detail?: { type: string; value: Uint8Array }): ConnectError {
  const failure = new ConnectError(OPERATOR_MESSAGE, connectCodeOf(connectCode));
  if (detail) failure.details = [detail];
  return failure;
}

function subjectOf(world: LocalizationWorld): Localizable {
  if (!world.subject) throw new Error("no error given");
  return world.subject;
}

/** Every Unicode white-space character becomes one U+0020. */
const folded = (text: string): string => text.replace(/\p{White_Space}/gu, " ");

/** The catalogs of a fixture directory: locale to key to pattern, sources files excluded. */
function catalogsOf(fixture: string): Record<string, Record<string, string>> {
  const dir = fixtureUrl(`${fixture}/`);
  const catalogs: Record<string, Record<string, string>> = {};
  for (const name of readdirSync(dir)) {
    const stem = name.slice(0, -".json".length);
    if (!name.endsWith(".json") || stem.includes(".")) continue;
    catalogs[stem] = JSON.parse(readFileSync(new URL(name, dir), "utf8"));
  }
  return catalogs;
}

Given("the platform error catalogs are loaded", async function () {
  await Promise.all(PLATFORM_LOCALES.map(loadErrorCatalog));
});

Given(
  "the error catalogs of emitter {string} are registered from fixture {string}",
  function (this: LocalizationWorld, emitter: string, fixture: string) {
    registerErrorCatalogs(emitter, catalogsOf(fixture));
  },
);

Given(
  "the error catalogs of emitter {string} are registered again from fixture {string}",
  function (this: LocalizationWorld, emitter: string, fixture: string) {
    registerErrorCatalogs(emitter, catalogsOf(fixture));
  },
);

Given(
  "a Connect error {string} whose ErrorInfo has code {string}, emitter {string} and args {string}",
  function (this: LocalizationWorld, connectCode: string, code: string, emitter: string, args: string) {
    const value = encodeErrorInfo({ code, emitter, args: new Map(parseArgs(args)) });
    this.subject = failureWith(connectCode, { type: ERROR_INFO_TYPE, value });
  },
);

Given(
  "a Connect error {string} whose unchecked ErrorInfo has code {string}, emitter {string} and args {string}",
  function (this: LocalizationWorld, connectCode: string, code: string, emitter: string, args: string) {
    const info = create(ErrorInfoSchema, { code, emitter, args: wireArgs(args) });
    this.subject = failureWith(connectCode, { type: ERROR_INFO_TYPE, value: toBinary(ErrorInfoSchema, info) });
  },
);

Given("a Connect error {string} with no ErrorInfo", function (this: LocalizationWorld, connectCode: string) {
  this.subject = failureWith(connectCode);
});

Given(
  "a Connect error {string} whose ErrorInfo detail is {string}",
  function (this: LocalizationWorld, connectCode: string, wire: string) {
    this.subject = failureWith(connectCode, { type: ERROR_INFO_TYPE, value: Uint8Array.from(Buffer.from(wire, "base64")) });
  },
);

Given(
  "a Connect error {string} whose ValidationFailure detail is {string}",
  function (this: LocalizationWorld, connectCode: string, wire: string) {
    this.subject = failureWith(connectCode, {
      type: VALIDATION_FAILURE_TYPE,
      value: Uint8Array.from(Buffer.from(wire, "base64")),
    });
  },
);

Given("a ValidationFailure whose wire value is {string}", function (this: LocalizationWorld, wire: string) {
  this.subject = fromBinary(ValidationFailureSchema, Uint8Array.from(Buffer.from(wire, "base64")));
});

Given(
  "a ValidationFailure with field {string}, rule {string} and unchecked args {string}",
  function (this: LocalizationWorld, field: string, rule: string, args: string) {
    this.subject = create(ValidationFailureSchema, { field, rule, message: OPERATOR_MESSAGE, args: wireArgs(args) });
  },
);

Given("the problem+json body from fixture {string}", function (this: LocalizationWorld, fixture: string) {
  this.subject = JSON.parse(readFileSync(fixtureUrl(fixture), "utf8")) as ProblemJson;
});

When("the error is localized for {string}", function (this: LocalizationWorld, locale: string) {
  const subject = subjectOf(this);
  const localized = localize(subject, locale);
  this.localized = localized;
});

When(
  "the error is localized for {string} in time zone {string}",
  function (this: LocalizationWorld, locale: string, zone: string) {
    const options = zone === "(none)" ? undefined : { timeZone: zone };
    const subject = subjectOf(this);
    try {
      const localized = localize(subject, locale, options);
      this.localized = localized;
    } catch (error) {
      if (!(error instanceof LocalizeOptionsError)) throw error;
      this.refused = error.rule;
    }
  },
);

When("the violations of the error are read", function (this: LocalizationWorld) {
  const subject = subjectOf(this) as ConnectError | ProblemJson;
  const violations = i18n.violationsOf(subject);
  this.violations = violations;
});

When("violation {int} is localized for {string}", function (this: LocalizationWorld, index: number, locale: string) {
  const violation = this.violations?.[index - 1];
  if (!violation) throw new Error("the violation does not exist");
  const localized = localize(violation, locale);
  this.localized = localized;
});

When(
  "the emitter builds ErrorInfo code {string} with args {string}",
  function (this: LocalizationWorld, code: string, args: string) {
    let byArgs = "ok";
    try {
      const built = errorArgs(Object.fromEntries(parseArgs(args)));
      encodeErrorInfo({ code, emitter: "acme", args: built });
    } catch (error) {
      if (!(error instanceof ErrorArgError)) throw error;
      byArgs = error.rule;
    }
    this.built = byArgs;
  },
);

Then("the localized text is {string}", function (this: LocalizationWorld, expected: string) {
  assert.equal(folded(this.localized?.text ?? "<not localized>"), folded(expected));
});

Then("the localized hint is {string}", function (this: LocalizationWorld, expected: string) {
  assert.ok(this.localized, "the error was not localized");
  assert.equal(
    this.localized.hint === undefined ? undefined : folded(this.localized.hint),
    expected === "none" ? undefined : folded(expected),
  );
});

Then("the localized text does not contain {string}", function (this: LocalizationWorld, absent: string) {
  assert.ok(this.localized, "the error was not localized");
  assert.ok(!this.localized.text.includes(absent), `text: ${this.localized.text}`);
});

Then("the localization is refused with rule {string}", function (this: LocalizationWorld, rule: string) {
  assert.equal(this.refused, rule);
});

Then("the construction succeeds", function (this: LocalizationWorld) {
  assert.equal(this.built, "ok");
});

Then("the construction is refused with rule {string}", function (this: LocalizationWorld, rule: string) {
  assert.equal(this.built, rule);
});

Then("there is/are {int} violation(s)", function (this: LocalizationWorld, count: number) {
  assert.equal(this.violations?.length, count);
});

Then(
  "violation {int} has field {string} and rule {string}",
  function (this: LocalizationWorld, index: number, field: string, rule: string) {
    const violation = this.violations?.[index - 1];
    assert.deepEqual(violation && { field: violation.field, rule: violation.rule }, { field, rule });
  },
);

Given("the platform locale registry from fixture {string}", function (this: LocalizationWorld, fixture: string) {
  this.registry = JSON.parse(readFileSync(fixtureUrl(fixture), "utf8"));
});

When("each of its locales and aliases is negotiated", function (this: LocalizationWorld) {
  const registry = this.registry;
  if (!registry) throw new Error("no registry given");
  const empty = negotiateLocale("");
  const requests: Array<[string, string]> = [
    ...registry.locales.map((code): [string, string] => [code, code]),
    ...Object.entries(registry.aliases),
  ];
  const negotiated: Array<[string, string, string]> = [["", empty, registry.default]];
  for (const [request, expected] of requests) negotiated.push([request, negotiateLocale(request), expected]);
  this.negotiated = negotiated;
});

Then(
  "each negotiates to its registry locale and the empty request to the default locale",
  function (this: LocalizationWorld) {
    assert.ok(this.negotiated?.length, "nothing was negotiated");
    for (const [request, got, expected] of this.negotiated) assert.equal(got, expected, `negotiating ${JSON.stringify(request)}`);
  },
);

Then(
  "this language's platform locales, aliases and default locale equal it",
  function (this: LocalizationWorld) {
    const registry = this.registry;
    if (!registry) throw new Error("no registry given");
    assert.deepEqual([...PLATFORM_LOCALES], registry.locales);
    assert.equal(DEFAULT_LOCALE, registry.default);
    assert.deepEqual({ ...LOCALE_ALIASES }, registry.aliases);
  },
);

Given("the CLDR release pinned in {string}", function (this: LocalizationWorld, file: string) {
  const pins = JSON.parse(readFileSync(new URL(`../../../${file}`, import.meta.url), "utf8")) as { node: string };
  this.cldrPin = pins.node;
});

Then("this language's formatting data is that CLDR release", function (this: LocalizationWorld) {
  assert.equal(process.versions.cldr, this.cldrPin);
});
