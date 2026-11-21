// SPDX-License-Identifier: MIT
/**
 * The TypeScript answer to `tests/features/error_localization_loading.feature`.
 *
 * The lane loads this contract before any other that loads a catalog, so the
 * package's one module instance still holds English only. Rust embeds every
 * locale and has no counterpart.
 */

import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";

import { Given, Then, When, World } from "@cucumber/cucumber";
import { Code, ConnectError } from "@connectrpc/connect";
import {
  DEFAULT_LOCALE,
  ERROR_INFO_TYPE,
  LocalizeOptionsError,
  PLATFORM_LOCALES,
  encodeErrorInfo,
  loadErrorCatalog,
  localize,
  registerErrorCatalogs,
  type ErrorCatalogLoader,
  type Localizable,
} from "@brefwiz/api-bones-connect";

interface LoadingWorld extends World {
  subject?: Localizable;
  localized?: { text: string; hint?: string };
  refused?: string;
}

function subjectOf(world: LoadingWorld): Localizable {
  if (!world.subject) throw new Error("no error given");
  return world.subject;
}

/** One loader per locale file of a fixture directory, each reading its file only when called. */
async function loadersOf(fixture: string): Promise<Record<string, ErrorCatalogLoader>> {
  const dir = new URL(`../../features/fixtures/error_localization/${fixture}/`, import.meta.url);
  const loaders: Record<string, ErrorCatalogLoader> = {};
  for (const name of await readdir(dir)) {
    const stem = name.slice(0, -".json".length);
    if (!name.endsWith(".json") || stem.includes(".")) continue;
    loaders[stem] = async () => ({ default: JSON.parse(await readFile(new URL(name, dir), "utf8")) });
  }
  return loaders;
}

Given("no platform catalog but English is loaded", function (this: LoadingWorld) {
  const subject = subjectOf(this);
  for (const locale of PLATFORM_LOCALES.filter((tag) => tag !== DEFAULT_LOCALE)) {
    assert.throws(
      () => localize(subject, locale),
      (error: unknown) => error instanceof LocalizeOptionsError && error.rule === "locale_not_loaded",
      `the ${locale} catalog was loaded before this contract ran`,
    );
  }
});

Given(
  "the error catalogs of emitter {string} are registered for loading from fixture {string}",
  async function (emitter: string, fixture: string) {
    registerErrorCatalogs(emitter, await loadersOf(fixture));
  },
);

When("the catalog for {string} is loaded", async function (locale: string) {
  await loadErrorCatalog(locale);
});

When("the error is localized for {string} from the loaded catalogs", function (this: LoadingWorld, locale: string) {
  const subject = subjectOf(this);
  try {
    const localized = localize(subject, locale);
    this.localized = localized;
  } catch (error) {
    if (!(error instanceof LocalizeOptionsError)) throw error;
    this.refused = error.rule;
  }
});

Then("the localization outcome is {string}", function (this: LoadingWorld, outcome: string) {
  const actual = this.refused === undefined ? this.localized?.text : `refused: ${this.refused}`;
  assert.equal(actual, outcome);
});

Then(
  "code {string} of emitter {string} renders {string} for {string}",
  function (code: string, emitter: string, text: string, locale: string) {
    const error = new ConnectError("operator message", Code.FailedPrecondition);
    error.details.push({ type: ERROR_INFO_TYPE, value: encodeErrorInfo({ code, emitter, args: new Map() }) } as never);
    assert.equal(localize(error, locale).text, text);
  },
);
