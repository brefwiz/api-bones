// SPDX-License-Identifier: MIT
//! Canonical field messages and pure, owner-independent validation.
//!
//! Owners resolve definition ids and enforce eligibility and stored-value
//! checks before accepting a narrowing change. No validator performs IO.
// Generated code (buffa-build) doesn't satisfy the crate's pedantic clippy bar:
// wire-format casts, generated view types, and decoding paths have their
// own lint shape. Scope expectations to the module boundary because the
// generator is the source of truth; generated output is never hand-edited.
// The private module exists only to scope these expectations. Public field
// types remain available directly from the crate root.
#[expect(
    clippy::cast_lossless,
    clippy::default_trait_access,
    clippy::elidable_lifetime_names,
    clippy::match_same_arms,
    clippy::missing_fields_in_debug,
    clippy::pub_underscore_fields,
    clippy::redundant_closure_for_method_calls,
    clippy::semicolon_if_nothing_returned,
    clippy::single_match_else,
    clippy::too_many_lines,
    clippy::unreadable_literal,
    reason = "buffa-build generates this module; the generator, not this crate, owns its lint shape"
)]
mod generated {
    include!(concat!(env!("OUT_DIR"), "/_fields.rs"));
}
mod validate;
pub use generated::bones::v1::{
    CalendarDate, DataClassification, EnumConstraints, EnumOption, FieldDefinition, FieldType,
    FieldValue, IntegerConstraints, Label, MoneyAmount, MoneyConstraints, NumberConstraints,
    RecordLinkConstraints, TextConstraints, TextFormat, TypedValue, ValidationFailure,
    field_definition, typed_value,
};
pub use validate::{
    DefinitionChange, FieldViolation, Rule, validate_definition, validate_definition_change,
    validate_value,
};

#[cfg(test)]
mod tests;
