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
import { rmSync } from "node:fs";

import { After, Given, Then, When, World } from "@cucumber/cucumber";

import {
  type DescField,
  type FileRegistry,
  ScalarType,
  getExtension,
  hasExtension,
} from "@bufbuild/protobuf";

import { compileContract } from "./descriptor_contract.ts";

interface FieldLabelsWorld extends World {
  source?: string;
  workdir?: string;
  registry?: FileRegistry;
  refusal?: string;
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
  const { workdir, registry } = compileContract("field-labels", world.source);
  world.workdir = workdir;
  return registry;
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

Given(
  "a record field {string} of type {string} labelled with value format {string}",
  function (this: FieldLabelsWorld, field: string, kind: string, format: string) {
    this.source = recordSource(field, kind, `[(bones.v1.format) = ${format}]`);
  },
);

Given(
  "a record field {string} of type {string} labelled with a reference to the field {string}",
  function (this: FieldLabelsWorld, field: string, kind: string, target: string) {
    this.source = recordSource(field, kind, `[(bones.v1.references).field = "${target}"]`);
  },
);

Given(
  "a record field {string} of type {string} labelled as a principal of the caller's organization",
  function (this: FieldLabelsWorld, field: string, kind: string) {
    this.source = recordSource(field, kind, "[(bones.v1.references).principal = true]");
  },
);

Given(
  "a record field {string} of type {string} labelled with a reference to both a field and a principal",
  function (this: FieldLabelsWorld, field: string, kind: string) {
    this.source = recordSource(
      field,
      kind,
      `[(bones.v1.references) = { field: "acme.v1.Event.id" principal: true }]`,
    );
  },
);

Given(
  "a contract enum {string} whose value {string} is labelled output-only beside {string}",
  function (this: FieldLabelsWorld, name: string, labelled: string, siblings: string) {
    const values = siblings.split(", ").map((value, number) => `${value} = ${number};`);
    values.push(`${labelled} = ${values.length} [(bones.v1.output_only) = true];`);
    this.source =
      `syntax = "proto3";\npackage contract.v1;\n` +
      `import "bones/v1/annotations.proto";\n` +
      `enum ${name} { ${values.join(" ")} }\n` +
      `message Record { string id = 1; }\n`;
  },
);

Given("the shipped field vocabulary", function (this: FieldLabelsWorld) {
  this.source = recordSource("id", "string", "");
});

Given("the shipped stream control", function (this: FieldLabelsWorld) {
  this.source =
    `syntax = "proto3";\npackage contract.v1;\n` +
    `import "bones/v1/stream.proto";\n` +
    `message Record { string id = 1; }\n`;
});

Given("the shipped pagination", function (this: FieldLabelsWorld) {
  this.source =
    `syntax = "proto3";\npackage contract.v1;\n` +
    `import "bones/v1/pagination.proto";\n` +
    `message Record { string id = 1; }\n`;
});

When("the record is read back through descriptor reflection", function (this: FieldLabelsWorld) {
  this.registry = compile(this);
});

When("the vocabulary is read back through descriptor reflection", function (this: FieldLabelsWorld) {
  this.registry = compile(this);
});

When("the contract is compiled", function (this: FieldLabelsWorld) {
  try {
    this.registry = compile(this);
  } catch (error) {
    this.refusal = String(error);
  }
});

Then(
  "the contract is refused at compile because {string} and {string} are mutually exclusive in {string}",
  function (this: FieldLabelsWorld, first: string, second: string, option: string) {
    assert.ok(this.refusal !== undefined, "the contract compiled");
    const lowered = this.refusal.toLowerCase();
    assert.ok(
      lowered.includes("oneof") &&
        lowered.includes("bones.v1.fieldreference.target") &&
        lowered.includes("set multiple times"),
      `refusal does not name the "${first}"/"${second}" conflict in "${option}": ${this.refusal}`,
    );
  },
);

Then("the field value format reads back as {string}", function (this: FieldLabelsWorld, expected: string) {
  const options = theField(this).proto.options;
  assert.ok(options, "the field carries no options");
  const value = getExtension(options, extensionOf(this, "bones.v1.format"));
  const enumType = registryOf(this).getEnum("bones.v1.ValueFormat");
  assert.ok(enumType, "the format enum is not declared");
  assert.equal(enumType.values.find((v) => v.number === (value as number))?.name, expected);
});

function referenceTarget(world: FieldLabelsWorld) {
  const options = theField(world).proto.options;
  assert.ok(options, "the field carries no options");
  const value = getExtension(options, extensionOf(world, "bones.v1.references")) as {
    target: { case?: string; value?: unknown };
  };
  return value.target;
}

Then("the field reference reads back as the field {string}", function (this: FieldLabelsWorld, expected: string) {
  assert.deepEqual(referenceTarget(this), { case: "field", value: expected });
});

Then("the field reference reads back as a principal of the caller's organization", function (this: FieldLabelsWorld) {
  assert.deepEqual(referenceTarget(this), { case: "principal", value: true });
});

/** The `output_only` label of one enum value, absent when the value carries none. */
function outputOnlyOf(world: FieldLabelsWorld, enumName: string, valueName: string): boolean | undefined {
  const found = registryOf(world).getEnum(enumName);
  if (!found) throw new Error(`${enumName} is not declared`);
  const value = found.values.find((v) => v.name === valueName);
  if (!value) throw new Error(`${valueName} is not declared`);
  const options = value.proto.options;
  const extension = extensionOf(world, "bones.v1.output_only");
  return options && hasExtension(options, extension) ? (getExtension(options, extension) as boolean) : undefined;
}

Then(
  "the enum value {string} of {string} reads back as output-only",
  function (this: FieldLabelsWorld, value: string, enumName: string) {
    assert.equal(outputOnlyOf(this, enumName, value), true);
  },
);

Then(
  "the enum value {string} of {string} is not output-only",
  function (this: FieldLabelsWorld, value: string, enumName: string) {
    assert.equal(outputOnlyOf(this, enumName, value), undefined);
  },
);

Then("the enum value options it declares are {string}", function (this: FieldLabelsWorld, expected: string) {
  const found: string[] = [];
  for (const item of registryOf(this)) {
    if (
      item.kind === "extension" &&
      item.extendee.typeName === "google.protobuf.EnumValueOptions" &&
      item.file.proto.package === "bones.v1"
    ) {
      found.push(item.name);
    }
  }
  assert.equal(sorted(found), expected);
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

Then(
  "every field of the message {string} is classified {string}",
  function (this: FieldLabelsWorld, name: string, expected: string) {
    const message = registryOf(this).getMessage(name);
    assert.ok(message, `${name} is not declared`);
    assert.ok(message.fields.length > 0, "the message has no fields");
    const enumType = registryOf(this).getEnum("bones.v1.DataClassification");
    assert.ok(enumType, "the classification enum is not declared");
    const extension = extensionOf(this, "bones.v1.classification");
    for (const field of message.fields) {
      const options = field.proto.options;
      assert.ok(options && hasExtension(options, extension), `${field.name} carries no classification`);
      assert.equal(enumType.value[getExtension(options, extension) as number]?.name, expected, field.name);
    }
  },
);

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


function shippedMessage(world: FieldLabelsWorld, name: string) {
  const message = registryOf(world).getMessage(name);
  if (!message) throw new Error(`${name} is not declared`);
  return message;
}

Then(
  "the message {string} declares the oneof {string} holding exactly {string}",
  function (this: FieldLabelsWorld, name: string, oneof: string, expected: string) {
    const message = shippedMessage(this, name);
    assert.equal(message.oneofs.length, 1, "one oneof");
    const signal = message.oneofs[0]!;
    assert.equal(signal.name, oneof);
    const members = signal.fields.map((f) => f.name).join(", ");
    assert.equal(members, expected);
    assert.equal(message.fields.map((f) => f.name).join(", "), members, "every field belongs to the oneof");
  },
);

Then(
  "the enum {string} declares exactly the values {string}",
  function (this: FieldLabelsWorld, name: string, expected: string) {
    const found = registryOf(this).getEnum(name);
    if (!found) throw new Error(`${name} is not declared`);
    assert.equal(found.values.map((v) => `${v.name}=${v.number}`).join(", "), expected);
  },
);

Then(
  "the message {string} declares exactly the fields {string}",
  function (this: FieldLabelsWorld, name: string, expected: string) {
    assert.equal(
      shippedMessage(this, name)
        .fields.map((f) => f.name)
        .join(", "),
      expected,
    );
  },
);
