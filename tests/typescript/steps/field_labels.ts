// SPDX-License-Identifier: MIT
/**
 * The TypeScript half of `tests/features/field_labels.feature`.
 *
 * The Rust half answers the same file; neither keeps its own copy of the rows.
 *
 * The descriptor set is built by `buf` from the shipped `bones/v1` files and
 * the one contract each scenario declares, then read back through the
 * protobuf-es registry. No descriptor is written by hand.
 */

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { After, Given, Then, When, World } from "@cucumber/cucumber";

import {
  type DescField,
  type FileRegistry,
  ScalarType,
  createFileRegistry,
  fromBinary,
  getExtension,
  hasExtension,
} from "@bufbuild/protobuf";
import { FileDescriptorSetSchema } from "@bufbuild/protobuf/wkt";

const SHIPPED = fileURLToPath(new URL("../../../api-bones-protos/proto", import.meta.url));
const VENDORED = fileURLToPath(new URL("../../../api-bones-protos/vendor", import.meta.url));
const BUF = fileURLToPath(new URL("../node_modules/.bin/buf", import.meta.url));

interface FieldLabelsWorld extends World {
  source?: string;
  workdir?: string;
  registry?: FileRegistry;
}

function recordSource(field: string, kind: string, options: string): string {
  return (
    `syntax = "proto3";\npackage contract.v1;\n` +
    `import "bones/v1/annotations.proto";\n` +
    `message Record { ${kind} ${field} = 1 ${options}; }\n`
  );
}

/** Compile the scenario's contract with the shipped files into a registry. */
function compile(world: FieldLabelsWorld): FileRegistry {
  if (world.source === undefined) throw new Error("no contract given");
  const dir = mkdtempSync(join(tmpdir(), "field-labels-"));
  world.workdir = dir;
  writeFileSync(join(dir, "buf.yaml"), "version: v2\n");
  cpSync(SHIPPED, dir, { recursive: true, filter: (src) => !src.endsWith("buf.yaml") });
  cpSync(VENDORED, dir, { recursive: true });
  mkdirSync(join(dir, "contract", "v1"), { recursive: true });
  writeFileSync(join(dir, "contract", "v1", "contract.proto"), world.source);
  const image = join(dir, "image.binpb");
  execFileSync(BUF, ["build", dir, "-o", image], { stdio: "pipe" });
  return createFileRegistry(fromBinary(FileDescriptorSetSchema, readFileSync(image)));
}

function registryOf(world: FieldLabelsWorld): FileRegistry {
  if (!world.registry) throw new Error("the record was not read back");
  return world.registry;
}

function theField(world: FieldLabelsWorld): DescField {
  const record = registryOf(world).getMessage("contract.v1.Record");
  if (!record) throw new Error("the record is not declared");
  return record.fields[0]!;
}

function extensionOf(world: FieldLabelsWorld, name: string) {
  const ext = registryOf(world).getExtension(name);
  if (!ext) throw new Error(`${name} is not declared`);
  return ext;
}

After(function (this: FieldLabelsWorld) {
  if (this.workdir) rmSync(this.workdir, { recursive: true, force: true });
});

Given(
  "a record field {string} of type {string} labelled with meaning {string} and classification {string}",
  function (this: FieldLabelsWorld, field: string, kind: string, meaning: string, classification: string) {
    this.source = recordSource(
      field,
      kind,
      `[(bones.v1.meaning) = "${meaning}", (bones.v1.classification) = ${classification}]`,
    );
  },
);

Given(
  "a record field {string} of type {string} labelled with classification {string} only",
  function (this: FieldLabelsWorld, field: string, kind: string, classification: string) {
    this.source = recordSource(field, kind, `[(bones.v1.classification) = ${classification}]`);
  },
);

Given(
  "a record field {string} of type {string} labelled with meaning {string} only",
  function (this: FieldLabelsWorld, field: string, kind: string, meaning: string) {
    this.source = recordSource(field, kind, `[(bones.v1.meaning) = "${meaning}"]`);
  },
);

Given("the shipped field vocabulary", function (this: FieldLabelsWorld) {
  this.source = recordSource("id", "string", "");
});

When("the record is read back through descriptor reflection", function (this: FieldLabelsWorld) {
  this.registry = compile(this);
});

When("the vocabulary is read back through descriptor reflection", function (this: FieldLabelsWorld) {
  this.registry = compile(this);
});

Then("the field meaning reads back as {string}", function (this: FieldLabelsWorld, expected: string) {
  const options = theField(this).proto.options;
  assert.ok(options, "the field carries no options");
  assert.equal(getExtension(options, extensionOf(this, "bones.v1.meaning")), expected);
});

Then("the field carries no meaning", function (this: FieldLabelsWorld) {
  const options = theField(this).proto.options;
  assert.ok(!options || !hasExtension(options, extensionOf(this, "bones.v1.meaning")));
});

Then("the field classification reads back as {string}", function (this: FieldLabelsWorld, expected: string) {
  const options = theField(this).proto.options;
  assert.ok(options, "the field carries no options");
  const value = getExtension(options, extensionOf(this, "bones.v1.classification"));
  const enumType = registryOf(this).getEnum("bones.v1.DataClassification");
  assert.ok(enumType, "the classification enum is not declared");
  assert.equal(enumType.value[value as number]?.name, expected);
});

Then("the field carries no classification", function (this: FieldLabelsWorld) {
  const options = theField(this).proto.options;
  assert.ok(!options || !hasExtension(options, extensionOf(this, "bones.v1.classification")));
});

Then("the field value type reads back as {string}", function (this: FieldLabelsWorld, expected: string) {
  const scalar = theField(this).scalar;
  assert.ok(scalar !== undefined, "the field is not a scalar");
  assert.equal(ScalarType[scalar]!.toLowerCase(), expected);
});

function sorted(found: string[]): string {
  return [...found].sort().join(", ");
}

Then("the field options it declares are {string}", function (this: FieldLabelsWorld, expected: string) {
  const found: string[] = [];
  for (const item of registryOf(this)) {
    if (
      item.kind === "extension" &&
      item.extendee.typeName === "google.protobuf.FieldOptions" &&
      item.file.proto.package === "bones.v1"
    ) {
      found.push(item.name);
    }
  }
  assert.equal(sorted(found), expected);
});

Then("the classification enums it declares are {string}", function (this: FieldLabelsWorld, expected: string) {
  const found: string[] = [];
  for (const item of registryOf(this)) {
    if (item.kind === "enum" && item.file.proto.package === "bones.v1" && item.name.includes("Classification")) {
      found.push(item.typeName);
    }
  }
  assert.equal(sorted(found), expected);
});

