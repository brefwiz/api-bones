// SPDX-License-Identifier: MIT
/**
 * The TypeScript answer to `tests/features/connect_tree_shaking.feature`.
 *
 * Bundles the built package the way a browser consumer's bundler does, with
 * the peer dependencies left to the consumer, and reads the result back.
 */

import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { Given, Then, When, World } from "@cucumber/cucumber";
import { create } from "@bufbuild/protobuf";
import { ConnectError, Code } from "@connectrpc/connect";
import { rolldown } from "rolldown";

import { FieldDefinitionSchema, isReplayableTransportFailure, validateDefinition } from "@brefwiz/api-bones-connect";
import { configureConnectTransport } from "@brefwiz/api-bones-connect/web";

import { policy } from "./csrf_fixtures.ts";

const dist = dirname(fileURLToPath(import.meta.resolve("@brefwiz/api-bones-connect")));
const ENTRY_FILE: Readonly<Record<string, string>> = { root: "index.js", web: "web.js" };
const FIELD_VALIDATION_MODULES = ["currency-codes", "validator", "@fhir-toolkit/ucum", "dist/fields.js"];
const PEERS = [/^@connectrpc\//, /^@bufbuild\//, /^@opentelemetry\//, /^@brefwiz\/spiffe-client/];

interface TreeShakingWorld extends World {
  page?: string;
  bundle?: { code: string; modules: string[] };
}

Given(
  "a page that imports only {string} from the {string} entry",
  function (this: TreeShakingWorld, exported: string, entry: string) {
    const file = ENTRY_FILE[entry];
    assert.ok(file, `the contract names an entry this step cannot resolve: ${entry}`);
    this.page = `import { ${exported} } from ${JSON.stringify(resolve(dist, file))}; globalThis.page = ${exported};`;
  },
);

When("the page is bundled for the browser", async function (this: TreeShakingWorld) {
  const id = "\0page";
  const source = this.page ?? "";
  const build = await rolldown({
    input: id,
    external: PEERS,
    platform: "browser",
    logLevel: "silent",
    plugins: [
      {
        name: "page",
        resolveId: (specifier: string) => (specifier === id ? id : null),
        load: (specifier: string) => (specifier === id ? source : null),
      },
    ],
  });
  const { output } = await build.generate({ format: "esm", minify: true });
  this.bundle = { code: output[0].code, modules: output[0].moduleIds };
});

Then("the bundle carries none of the field-validation data", function (this: TreeShakingWorld) {
  const modules = this.bundle?.modules ?? [];
  for (const name of FIELD_VALIDATION_MODULES) {
    assert.deepEqual(modules.filter((id) => id.includes(name)), [], `${name} leaked into the bundle`);
  }
});

Then("the bundle is smaller than {int} bytes", function (this: TreeShakingWorld, ceiling: number) {
  const size = Buffer.byteLength(this.bundle?.code ?? "");
  assert.ok(size < ceiling, `the bundle is ${size} bytes, not under ${ceiling}`);
});

Then("the bundle carries the field-validation data", function (this: TreeShakingWorld) {
  const { code, modules } = this.bundle ?? { code: "", modules: [] };
  assert.ok(modules.some((id) => id.includes("currency-codes")), "currency-codes is missing from the bundle");
  const data = (createRequire(resolve(dist, "index.js"))("currency-codes") as { data: { currency: string }[] }).data;
  assert.ok(code.includes(data[0].currency), "the currency table is missing from the bundle");
});

Then("the replay check still treats an unavailable failure as replayable", function () {
  const replayable = isReplayableTransportFailure(new ConnectError("dropped", Code.Unavailable));
  assert.equal(replayable, true);
});

Then("the browser transport still builds a callable transport", function () {
  const transport = configureConnectTransport({ baseUrl: "https://app.example.com/itinerwiz", profile: "webapp", policy });
  assert.equal(typeof transport.unary, "function");
});

Then("the field-validation API still reports a malformed definition key", function () {
  const violations = validateDefinition(create(FieldDefinitionSchema, { key: "Not A Slug" }));
  assert.ok(violations.some((violation) => violation.field === "/key"));
});
