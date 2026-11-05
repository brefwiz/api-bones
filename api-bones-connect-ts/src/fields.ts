// SPDX-License-Identifier: MIT
/** Pure validation of canonical field definitions and their typed values. */
import { create, equals } from "@bufbuild/protobuf";
import { createUcumService, UcumValidationError } from "@fhir-toolkit/ucum";
import currencyCodes from "currency-codes";
import validator from "validator";
import { FieldType, TextFormat, FieldDefinitionSchema, type FieldDefinition, type TypedValue } from "./generated/bones/v1/fields_pb.js";
import { ValidationFailureSchema } from "./generated/bones/v1/errors_pb.js";
export * from "./generated/bones/v1/fields_pb.js";
export { DataClassification, LabelSchema, type Label } from "./generated/bones/v1/annotations_pb.js";

/** Closed vocabulary carried by ValidationFailure.rule. */
export type FieldRule = "type_mismatch" | "range" | "non_finite" | "fraction_digits" | "unknown_option" | "option_archived" | "max_length" | "format" | "currency_mismatch" | "definition_archived";
export type FieldViolation = Omit<ReturnType<typeof violation>, never>;
export type DefinitionChange = "unchanged" | "compatible" | "narrowing";
export type DefinitionChangeResult = { change: DefinitionChange; violations: readonly FieldViolation[] } | { change?: undefined; violations: readonly FieldViolation[] };
function violation(field: string, rule: FieldRule, message: string) {
  return Object.assign(create(ValidationFailureSchema, { field, rule, message }), { rule });
}
const slug = (s: string) => /^[a-z][a-z0-9_-]*$/.test(s);
const reference = (s: string) => s.trim().length > 0 && !/\s/u.test(s);
const bounds = <T extends number | bigint>(min?: T, max?: T) => min === undefined || max === undefined || min <= max;
const inRange = <T extends number | bigint>(n: T, min?: T, max?: T) => (min === undefined || n >= min) && (max === undefined || n <= max);
function unitValid(unit?: string): boolean {
  if (unit === undefined) return true;
  if (!unit) return false;
  // ucum-units rejects nested braces and nongraphic annotation characters.
  // The upstream lexer accepts both: https://github.com/robertoAraneda/fhir-toolkit/blob/main/packages/ucum/src/lexer.ts
  if (/\{[^}]*(?:\{|[^\x21-\x7e}])/u.test(unit)) return false;
  // The upstream service caches successful inputs without a bound; keep that cache call-local.
  try { createUcumService().validate(unit); return true; }
  catch (error) { if (error instanceof UcumValidationError) return false; throw error; }
}
/** Validate a definition before owner stores it; empty ids are allowed before minting. */
export function validateDefinition(definition: FieldDefinition): FieldViolation[] {
  const errors: FieldViolation[] = [];
  const check = (valid: boolean, field: string, rule: FieldRule, message: string) => { if (!valid) errors.push(violation(field, rule, message)); };
  check(!definition.id || reference(definition.id), "/id", "format", "definition id must be nonblank and contain no whitespace");
  check(slug(definition.key), "/key", "format", "key must be a lowercase slug");
  check(Boolean(definition.label?.text.trim()), "/label/text", "format", "label must be nonblank");
  check(definition.classification >= 1 && definition.classification <= 4, "/classification", "format", "classification must be specified");
  if (definition.meaning !== undefined) check(definition.meaning.startsWith("schema:") && reference(definition.meaning.slice(7)), "/meaning", "format", "meaning must name a schema term");
  const c = definition.constraints;
  const expected: Partial<Record<FieldType, string | undefined>> = {
    [FieldType.BOOLEAN]: undefined, [FieldType.DATE]: undefined, [FieldType.TIMESTAMP]: undefined,
    [FieldType.DURATION]: undefined, [FieldType.PERSON_REFERENCE]: undefined,
    [FieldType.NUMBER]: "number", [FieldType.INTEGER]: "integer", [FieldType.ENUM]: "enumeration",
    [FieldType.TEXT]: "text", [FieldType.MONEY]: "money", [FieldType.RECORD_LINK]: "recordLink",
  };
  check(definition.type in expected && c.case === expected[definition.type], "/constraints", "type_mismatch", "constraints must match field type");
  if (c.case === "number") {
    check((c.value.min === undefined || Number.isFinite(c.value.min)) && (c.value.max === undefined || Number.isFinite(c.value.max)), "/number", "non_finite", "bounds must be finite");
    check(bounds(c.value.min, c.value.max), "/number", "range", "minimum must not exceed maximum");
    check(unitValid(c.value.unit), "/number/unit", "format", "unit must parse as UCUM");
  } else if (c.case === "integer") {
    check(bounds(c.value.min, c.value.max), "/integer", "range", "minimum must not exceed maximum");
    check(unitValid(c.value.unit), "/integer/unit", "format", "unit must parse as UCUM");
  } else if (c.case === "money") {
    check(/^[A-Z]{3}$/.test(c.value.currency) && Boolean(currencyCodes.code(c.value.currency)), "/money/currency", "format", "currency must be ISO 4217");
    check(bounds(c.value.minMinor, c.value.maxMinor), "/money", "range", "minimum must not exceed maximum");
  } else if (c.case === "enumeration") {
    check(c.value.options.length >= 1 && c.value.options.length <= 200, "/enumeration/options", "range", "enum requires 1..200 options");
    const ids = new Set<string>();
    c.value.options.forEach((option, index) => {
      check(slug(option.id) && !ids.has(option.id), `/enumeration/options/${index}/id`, "format", "option id must be a unique immutable slug"); ids.add(option.id);
      check(Boolean(option.label?.text.trim()), `/enumeration/options/${index}/label/text`, "format", "option label must be nonblank");
    });
  } else if (c.case === "text") {
    check((c.value.maxLength ?? 0) >= 1 && (c.value.maxLength ?? 0) <= 4096, "/text/maxLength", "range", "text limit must be 1..4096");
    check(c.value.format >= TextFormat.PLAIN && c.value.format <= TextFormat.PHONE_E164, "/text/format", "format", "text format must be specified");
  } else if (c.case === "recordLink") check(reference(c.value.targetRecordType), "/recordLink/targetRecordType", "format", "record target type must be nonblank");
  return errors;
}
function decimalPlaces(value: number): number {
  const [mantissa, exponent = "0"] = value.toString().split(/[eE]/);
  return Math.max(0, (mantissa.split(".")[1]?.length ?? 0) - Number(exponent));
}
function textValid(text: string, format: TextFormat): boolean {
  switch (format) {
    case TextFormat.PLAIN: return true;
    case TextFormat.EMAIL: return validator.isEmail(text, { require_tld: false, allow_ip_domain: true });
    case TextFormat.URI: try { new URL(text); return true; } catch { return false; }
    case TextFormat.PHONE_E164: return /^\+[1-9][0-9]{1,14}$/.test(text);
    default: return false;
  }
}
/** Refuse one new value against its resolved definition, with a stable JSON pointer. */
export function validateValue(definition: FieldDefinition, value: TypedValue): FieldViolation | undefined {
  const fail = (rule: FieldRule, message: string) => violation("/value", rule, message);
  if (definition.archived) return fail("definition_archived", "definition is archived");
  const c = definition.constraints; const v = value.value;
  if (definition.type === FieldType.BOOLEAN && c.case === undefined && v.case === "boolean") return;
  if (definition.type === FieldType.NUMBER && c.case === "number" && v.case === "number") {
    if (!Number.isFinite(v.value)) return fail("non_finite", "number must be finite");
    if (!inRange(v.value, c.value.min, c.value.max)) return fail("range", "number exceeds inclusive bounds");
    if (c.value.maxFractionDigits !== undefined && decimalPlaces(v.value) > c.value.maxFractionDigits) return fail("fraction_digits", "number exceeds fractional digit limit");
    return;
  }
  if (definition.type === FieldType.INTEGER && c.case === "integer" && v.case === "integer") return inRange(v.value, c.value.min, c.value.max) ? undefined : fail("range", "integer exceeds inclusive bounds");
  if (definition.type === FieldType.ENUM && c.case === "enumeration" && v.case === "enumOptionId") {
    const option = c.value.options.find((option) => option.id === v.value);
    return !option ? fail("unknown_option", "unknown enum option") : option.archived ? fail("option_archived", "enum option is archived") : undefined;
  }
  if (definition.type === FieldType.TEXT && c.case === "text" && v.case === "text") {
    if ([...v.value].length > (c.value.maxLength ?? 0)) return fail("max_length", "text exceeds Unicode scalar limit");
    return textValid(v.value, c.value.format) ? undefined : fail("format", "text does not match declared format");
  }
  if (definition.type === FieldType.DATE && c.case === undefined && v.case === "date") {
    const { year, month, day } = v.value;
    const leap = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
    const days = [31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][month - 1] ?? 0;
    return year >= 1 && year <= 9999 && day >= 1 && day <= days ? undefined : fail("format", "invalid calendar date");
  }
  if (definition.type === FieldType.TIMESTAMP && c.case === undefined && v.case === "timestamp") return inRange(v.value.seconds, -62135596800n, 253402300799n) && inRange(v.value.nanos, 0, 999999999) ? undefined : fail("format", "invalid protobuf timestamp");
  if (definition.type === FieldType.DURATION && c.case === undefined && v.case === "duration") return inRange(v.value.seconds, -315576000000n, 315576000000n) && inRange(v.value.nanos, -999999999, 999999999) && (v.value.seconds === 0n || v.value.nanos === 0 || (v.value.seconds > 0n) === (v.value.nanos > 0)) ? undefined : fail("format", "invalid protobuf duration");
  if (definition.type === FieldType.MONEY && c.case === "money" && v.case === "money") {
    if (v.value.currency !== c.value.currency) return fail("currency_mismatch", "currency differs from definition");
    return inRange(v.value.minorUnits, c.value.minMinor, c.value.maxMinor) ? undefined : fail("range", "money exceeds inclusive minor-unit bounds");
  }
  if ((definition.type === FieldType.PERSON_REFERENCE && c.case === undefined && v.case === "personId") || (definition.type === FieldType.RECORD_LINK && c.case === "recordLink" && v.case === "recordId")) return reference(v.value) ? undefined : fail("format", "reference id must be nonblank and contain no whitespace");
  return fail("type_mismatch", "value arm differs from definition type");
}
const narrowed = <T extends number | bigint>(aMin: T | undefined, aMax: T | undefined, bMin: T | undefined, bMax: T | undefined) => (bMin !== undefined && (aMin === undefined || bMin > aMin)) || (bMax !== undefined && (aMax === undefined || bMax < aMax));
/** Preserve stable identities; narrowing requires owner's stored-value check. */
export function validateDefinitionChange(old: FieldDefinition, next: FieldDefinition): DefinitionChangeResult {
  const violations = validateDefinition(next);
  if (violations.length) return { violations };
  if (old.id !== next.id) violations.push(violation("/id", "format", "definition id is immutable"));
  if (old.key !== next.key) violations.push(violation("/key", "format", "definition key is immutable"));
  if (old.type !== next.type) violations.push(violation("/type", "type_mismatch", "definition type is immutable"));
  let narrowing = false;
  const a = old.constraints; const b = next.constraints;
  if (a.case === "number" && b.case === "number") narrowing = narrowed(a.value.min, a.value.max, b.value.min, b.value.max) || (b.value.maxFractionDigits !== undefined && (a.value.maxFractionDigits === undefined || b.value.maxFractionDigits < a.value.maxFractionDigits));
  else if (a.case === "integer" && b.case === "integer") narrowing = narrowed(a.value.min, a.value.max, b.value.min, b.value.max);
  else if (a.case === "money" && b.case === "money") {
    if (a.value.currency !== b.value.currency) violations.push(violation("/money/currency", "currency_mismatch", "currency is immutable"));
    narrowing = narrowed(a.value.minMinor, a.value.maxMinor, b.value.minMinor, b.value.maxMinor);
  } else if (a.case === "text" && b.case === "text") narrowing = (b.value.maxLength ?? 0) < (a.value.maxLength ?? 0) || (a.value.format !== b.value.format && b.value.format !== TextFormat.PLAIN);
  else if (a.case === "enumeration" && b.case === "enumeration") for (const option of a.value.options) if (!b.value.options.some((candidate) => candidate.id === option.id)) violations.push(violation("/enumeration/options", "unknown_option", "archive enum options instead of removing them"));
  if (violations.length) return { violations };
  return { change: equals(FieldDefinitionSchema, old, next) ? "unchanged" : narrowing ? "narrowing" : "compatible", violations };
}
