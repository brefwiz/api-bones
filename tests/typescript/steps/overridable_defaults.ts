// SPDX-License-Identifier: MIT
/**
 * The TypeScript half of `tests/features/overridable_defaults.feature`.
 *
 * The Rust half answers the same file; neither keeps its own copy of the rows.
 *
 * The descriptor set is built by `buf` from the shipped `bones/v1` files and
 * the one contract each scenario declares, then read back through the
 * protobuf-es registry. No descriptor is written by hand.
 */

import assert from "node:assert/strict";
import { rmSync } from "node:fs";

import { After, Given, Then, When, World } from "@cucumber/cucumber";

import { type DescMessage, type FileRegistry, getExtension, hasExtension } from "@bufbuild/protobuf";

import { compileContract } from "./descriptor_contract.ts";

interface OverridableDefaultsWorld extends World {
  source?: string;
  instance?: string;
  workdir?: string;
  registry?: FileRegistry;
}

function leaf(name: string): string {
  return name.split(".").pop()!;
}

interface Fixture {
  instance: string;
  messageOptions: string;
  field: string;
  fieldOptions: string;
  origin: string;
  originField: string;
}

function fixtureSource(f: Fixture): string {
  return (
    `syntax = "proto3";\npackage contract.v1;\n` +
    `import "bones/v1/annotations.proto";\n` +
    `message ${leaf(f.origin)} { string ${f.originField} = 1; }\n` +
    `message ${f.instance} { ${f.messageOptions}\n  string ${f.field} = 1 ${f.fieldOptions}; }\n`
  );
}

/** Compile the scenario's contract with the shipped files into a registry. */
function compile(world: OverridableDefaultsWorld): FileRegistry {
  if (world.source === undefined) throw new Error("no contract given");
  const { workdir, registry } = compileContract("overridable-defaults", world.source);
  world.workdir = workdir;
  return registry;
}

function registryOf(world: OverridableDefaultsWorld): FileRegistry {
  if (!world.registry) throw new Error("the instance was not read back");
  return world.registry;
}

function theInstance(world: OverridableDefaultsWorld): DescMessage {
  const message = registryOf(world).getMessage(`contract.v1.${world.instance}`);
  if (!message) throw new Error("the instance is not declared");
  return message;
}

function extensionOf(world: OverridableDefaultsWorld, name: string) {
  const ext = registryOf(world).getExtension(name);
  if (!ext) throw new Error(`${name} is not declared`);
  return ext;
}

After(function (this: OverridableDefaultsWorld) {
  if (this.workdir) rmSync(this.workdir, { recursive: true, force: true });
});

Given(
  "an instance {string} of {string} whose field {string} is defaulted from {string}",
  function (this: OverridableDefaultsWorld, instance: string, origin: string, field: string, originField: string) {
    this.instance = instance;
    this.source = fixtureSource({
      instance,
      messageOptions: `option (bones.v1.instance_of) = "${origin}";`,
      field,
      fieldOptions: `[(bones.v1.defaulted_from) = "${originField}"]`,
      origin,
      originField: leaf(originField),
    });
  },
);

Given(
  "a plain message {string} with a field {string}",
  function (this: OverridableDefaultsWorld, message: string, field: string) {
    this.instance = message;
    this.source = fixtureSource({
      instance: message,
      messageOptions: "",
      field,
      fieldOptions: "",
      origin: "contract.v1.Origin",
      originField: field,
    });
  },
);

Given(
  "an instance {string} of {string} whose field {string} has no default origin",
  function (this: OverridableDefaultsWorld, instance: string, origin: string, field: string) {
    this.instance = instance;
    this.source = fixtureSource({
      instance,
      messageOptions: `option (bones.v1.instance_of) = "${origin}";`,
      field,
      fieldOptions: "",
      origin,
      originField: field,
    });
  },
);

When("the instance is read back through descriptor reflection", function (this: OverridableDefaultsWorld) {
  this.registry = compile(this);
});

Then(
  "the instance reads back as an instance of {string}",
  function (this: OverridableDefaultsWorld, expected: string) {
    const options = theInstance(this).proto.options;
    assert.ok(options, "the message carries no options");
    assert.equal(getExtension(options, extensionOf(this, "bones.v1.instance_of")), expected);
  },
);

Then("the message carries no instance origin", function (this: OverridableDefaultsWorld) {
  const options = theInstance(this).proto.options;
  assert.ok(!options || !hasExtension(options, extensionOf(this, "bones.v1.instance_of")));
});

Then(
  "the field default reads back as originating from {string}",
  function (this: OverridableDefaultsWorld, expected: string) {
    const options = theInstance(this).fields[0]!.proto.options;
    assert.ok(options, "the field carries no options");
    assert.equal(getExtension(options, extensionOf(this, "bones.v1.defaulted_from")), expected);
  },
);

Then("the field carries no default origin", function (this: OverridableDefaultsWorld) {
  const options = theInstance(this).fields[0]!.proto.options;
  assert.ok(!options || !hasExtension(options, extensionOf(this, "bones.v1.defaulted_from")));
});
