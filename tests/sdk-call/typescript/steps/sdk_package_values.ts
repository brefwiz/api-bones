// SPDX-License-Identifier: MIT
/**
 * The TypeScript answer to `tests/sdk-call/features/sdk_package_values.feature`.
 *
 * The call is the SDK's own, through the steps of the sdk-call feature; the
 * assertions read what the package exports and compare it with what the call
 * returned. A caller that builds a message must not need a path into the
 * generated sources.
 */

import assert from "node:assert/strict";

import { Then } from "@cucumber/cucumber";
import * as sdk from "@brefwiz/api-bones-test-service-sdk";

import type { SdkCallWorld } from "./sdk_call.ts";

const exported: Readonly<Record<string, unknown>> = sdk;

/** A message schema as a caller sees it: protobuf-es names it by its full name. */
type Schema = { readonly typeName: string };

Then("the package exports the schemas {string}", function (names: string) {
  for (const name of names.split(",")) {
    assert.equal(typeof exported[name], "object", `the package does not export ${name}`);
  }
});

Then("the schema {string} describes the message {string}", function (name: string, message: string) {
  assert.equal((exported[name] as Schema | undefined)?.typeName, message);
});

Then("every part the caller received is a message of the exported schema {string}", function (this: SdkCallWorld, name: string) {
  assert.ok(this.receivedParts.length > 0, "the call returned no part");
  const schema = exported[name] as Schema | undefined;
  assert.ok(schema, `the package does not export ${name}`);
  for (const part of this.receivedParts) assert.equal(part.$typeName, schema.typeName);
});

Then(
  "the package exports the enum {string} with the member {string} valued {int}",
  function (name: string, member: string, value: number) {
    const values = exported[name] as Readonly<Record<string, unknown>> | undefined;
    assert.equal(typeof values, "object", `the package does not export the enum ${name}`);
    assert.equal(values?.[member], value);
  },
);

Then(
  "every part the caller received carries the member {string} of the exported enum {string}",
  function (this: SdkCallWorld, member: string, name: string) {
    assert.ok(this.receivedParts.length > 0, "the call returned no part");
    const values = exported[name] as Readonly<Record<string, unknown>> | undefined;
    assert.ok(values, `the package does not export the enum ${name}`);
    for (const part of this.receivedParts) assert.equal(part.kind, values[member]);
  },
);
