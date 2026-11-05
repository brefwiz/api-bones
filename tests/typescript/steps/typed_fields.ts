// SPDX-License-Identifier: MIT
import assert from "node:assert/strict";
import { clone, create } from "@bufbuild/protobuf";
import { Given, Then, When, World } from "@cucumber/cucumber";
import {
  validateDefinition, validateDefinitionChange, validateValue,
  FieldType, TextFormat, DataClassification, FieldDefinitionSchema, TypedValueSchema,
  type FieldDefinition, type TypedValue,
} from "@brefwiz/api-bones-connect";

interface FieldsWorld extends World {
  valueResult?: ReturnType<typeof validateValue>;
  definitionResult?: ReturnType<typeof validateDefinition>;
  changeResult?: ReturnType<typeof validateDefinitionChange>;
  operation?: "value" | "definition" | "change";
  fields: { definition: FieldDefinition };
}
const label = (text: string) => ({ text });
const option = (id: string, archived = false) => ({ id, label: label(id), archived });
Given("a typed field for {string}", function (this: FieldsWorld, type: string) {
  const definition = create(FieldDefinitionSchema, { id: "field-1", key: "example", label: label("Example"), classification: DataClassification.INTERNAL });
  switch (type) {
    case "boolean": definition.type = FieldType.BOOLEAN; break;
    case "number": definition.type = FieldType.NUMBER; definition.constraints = create(FieldDefinitionSchema, { constraints: { case: "number", value: { min: 0, max: 100, unit: "{seat}", maxFractionDigits: 2 } } }).constraints; break;
    case "integer": definition.type = FieldType.INTEGER; definition.constraints = create(FieldDefinitionSchema, { constraints: { case: "integer", value: { min: 0n, max: 100n } } }).constraints; break;
    case "enum": definition.type = FieldType.ENUM; definition.constraints = create(FieldDefinitionSchema, { constraints: { case: "enumeration", value: { options: [option("ready"), option("retired", true)] } } }).constraints; break;
    case "text": case "email": case "uri": case "phone":
      definition.type = FieldType.TEXT; definition.constraints = create(FieldDefinitionSchema, { constraints: { case: "text", value: { maxLength: type === "text" ? 5 : 256, format: type === "email" ? TextFormat.EMAIL : type === "uri" ? TextFormat.URI : type === "phone" ? TextFormat.PHONE_E164 : TextFormat.PLAIN } } }).constraints; break;
    case "date": definition.type = FieldType.DATE; break;
    case "timestamp": definition.type = FieldType.TIMESTAMP; break;
    case "duration": definition.type = FieldType.DURATION; break;
    case "money": definition.type = FieldType.MONEY; definition.constraints = create(FieldDefinitionSchema, { constraints: { case: "money", value: { currency: "USD", minMinor: 0n, maxMinor: 10000n } } }).constraints; break;
    case "person_reference": definition.type = FieldType.PERSON_REFERENCE; break;
    case "record_link": definition.type = FieldType.RECORD_LINK; definition.constraints = create(FieldDefinitionSchema, { constraints: { case: "recordLink", value: { targetRecordType: "booking" } } }).constraints; break;
    default: throw new Error(`unknown field type ${type}`);
  }
  this.fields = { definition };
});
function setting(d: FieldDefinition, name: string, value: string) {
  switch (name) {
    case "id": d.id = value; return;
    case "key": d.key = value; return;
    case "label": d.label = create(FieldDefinitionSchema, { label: label(value) }).label; return;
    case "classification": d.classification = Number(value); return;
    case "archived": d.archived = value === "true"; return;
    case "constraints": assert.equal(value, "none"); d.constraints = { case: undefined }; return;
    case "type": assert.equal(value, "integer"); d.type = FieldType.INTEGER; d.constraints = create(FieldDefinitionSchema, { constraints: { case: "integer", value: {} } }).constraints; return;
  }
  const c = d.constraints;
  if (c.case === "number") {
    if (name === "min") c.value.min = Number(value);
    else if (name === "max") c.value.max = Number(value);
    else if (name === "unit") c.value.unit = value;
    else if (name === "fraction") c.value.maxFractionDigits = Number(value);
    else throw new Error(`unknown number setting ${name}`);
  } else if (c.case === "text") { assert.equal(name, "max_length"); c.value.maxLength = value === "absent" ? undefined : Number(value); }
  else if (c.case === "money") { if (name === "currency") c.value.currency = value; else { assert.equal(name, "max"); c.value.maxMinor = BigInt(value); } }
  else if (c.case === "recordLink") { assert.equal(name, "target_type"); c.value.targetRecordType = value; }
  else if (c.case === "enumeration") {
    const make = (id: string) => create(FieldDefinitionSchema, { constraints: { case: "enumeration", value: { options: [option(id)] } } }).constraints;
    if (name === "option_id") c.value.options[1].id = value;
    else if (name === "option_label") c.value.options[0].label = create(FieldDefinitionSchema, { label: label(value) }).label;
    else if (name === "remove_option") c.value.options = c.value.options.filter((o) => o.id !== value);
    else if (name === "archive_option") c.value.options.find((o) => o.id === value)!.archived = true;
    else if (name === "add_option") { const next = make(value); if (next.case === "enumeration") c.value.options.push(next.value.options[0]); }
    else if (name === "option_count") { const next = create(FieldDefinitionSchema, { constraints: { case: "enumeration", value: { options: Array.from({ length: Number(value) }, (_, i) => option(`option-${i}`)) } } }).constraints; if (next.case === "enumeration") c.value.options = next.value.options; }
    else throw new Error(`unknown enum setting ${name}`);
  } else throw new Error(`unknown field setting ${name}`);
}
Given("the field setting {string} is {string}", function (this: FieldsWorld, name: string, value: string) { setting(this.fields.definition, name, value); });
function valueOf(input: string): TypedValue {
  if (input === "unset") return create(TypedValueSchema);
  const index = input.indexOf(":"); const kind = input.slice(0, index); const text = input.slice(index + 1);
  switch (kind) {
    case "boolean": return create(TypedValueSchema, { value: { case: "boolean", value: text === "true" } });
    case "number": return create(TypedValueSchema, { value: { case: "number", value: text === "inf" ? Infinity : Number(text) } });
    case "integer": return create(TypedValueSchema, { value: { case: "integer", value: BigInt(text) } });
    case "enum": return create(TypedValueSchema, { value: { case: "enumOptionId", value: text } });
    case "text": return create(TypedValueSchema, { value: { case: "text", value: text } });
    case "person": return create(TypedValueSchema, { value: { case: "personId", value: text } });
    case "record": return create(TypedValueSchema, { value: { case: "recordId", value: text } });
    case "date": { const [year, month, day] = text.split("-").map(Number); return create(TypedValueSchema, { value: { case: "date", value: { year, month, day } } }); }
    case "timestamp": case "duration": { const [seconds, nanos] = text.split(":"); return create(TypedValueSchema, { value: { case: kind, value: { seconds: BigInt(seconds), nanos: Number(nanos) } } }); }
    case "money": { const [minor, currency] = text.split(":"); return create(TypedValueSchema, { value: { case: "money", value: { minorUnits: BigInt(minor), currency } } }); }
    default: throw new Error(`unknown value type ${kind}`);
  }
}
When("the owner validates value {string}", function (this: FieldsWorld, input: string) {
  const value = valueOf(input);
  const result = validateValue(this.fields.definition, value);
  this.operation = "value";
  this.valueResult = result;
});
When("the owner validates the definition", function (this: FieldsWorld) {
  const result = validateDefinition(this.fields.definition);
  this.operation = "definition";
  this.definitionResult = result;
});
When("the owner changes field setting {string} to {string}", function (this: FieldsWorld, name: string, value: string) {
  const previous = clone(FieldDefinitionSchema, this.fields.definition);
  setting(this.fields.definition, name, value);
  const result = validateDefinitionChange(previous, this.fields.definition);
  this.operation = "change";
  this.changeResult = result;
});
Then("the SDK field result has rule {string}", function (this: FieldsWorld, expected: string) {
  assert.ok(this.operation);
  const violations = this.operation === "value"
    ? this.valueResult ? [this.valueResult] : []
    : this.operation === "definition" ? this.definitionResult! : this.changeResult!.violations;
  assert.ok(violations);
  if (!expected) assert.deepEqual(violations, []);
  else { const violation = violations.find((v) => v.rule === expected); assert.ok(violation); assert.ok(violation.field.startsWith("/")); assert.ok(violation.message.length); }
});
Then("the SDK field change is {string}", function (this: FieldsWorld, expected: string) {
  assert.ok(this.changeResult);
  assert.equal(this.changeResult.change ?? "", expected);
});
