// SPDX-License-Identifier: MIT
//! Validation shared by every owner of typed definitions.
use crate::{
    FieldDefinition, FieldType, TextFormat, TypedValue, ValidationFailure,
    field_definition::Constraints, typed_value::Value,
};
use std::collections::HashSet;

/// Closed vocabulary carried by `bones.v1.ValidationFailure.rule`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Rule {
    /// The selected arm differs from the definition's type.
    TypeMismatch,
    /// An inclusive bound or schema cardinality is violated.
    Range,
    /// A floating-point number is not finite.
    NonFinite,
    /// A number exceeds its decimal precision.
    FractionDigits,
    /// The enum option does not exist.
    UnknownOption,
    /// The enum option is archived.
    OptionArchived,
    /// Text exceeds its Unicode scalar limit.
    MaxLength,
    /// A structured value or identifier is malformed.
    Format,
    /// A currency differs from its definition.
    CurrencyMismatch,
    /// New writes to an archived definition are refused.
    DefinitionArchived,
}

impl Rule {
    /// Stable protobuf rule token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TypeMismatch => "type_mismatch",
            Self::Range => "range",
            Self::NonFinite => "non_finite",
            Self::FractionDigits => "fraction_digits",
            Self::UnknownOption => "unknown_option",
            Self::OptionArchived => "option_archived",
            Self::MaxLength => "max_length",
            Self::Format => "format",
            Self::CurrencyMismatch => "currency_mismatch",
            Self::DefinitionArchived => "definition_archived",
        }
    }
}

/// A refusal with a JSON pointer, human message and stable rule token.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FieldViolation {
    /// JSON pointer relative to the validated message.
    pub field: String,
    /// Human-readable reason.
    pub message: String,
    /// Machine-readable rule.
    pub rule: Rule,
}

impl FieldViolation {
    fn new(field: &str, rule: Rule, message: &str) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
            rule,
        }
    }
}

impl From<FieldViolation> for ValidationFailure {
    fn from(value: FieldViolation) -> Self {
        Self {
            field: value.field,
            message: value.message,
            rule: value.rule.as_str().into(),
            ..Self::default()
        }
    }
}

/// Whether an allowed change requires the owner's stored-value check.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DefinitionChange {
    /// No field changed.
    Unchanged,
    /// Existing values stay valid without reading storage.
    Compatible,
    /// Owner must reject if any stored value would violate the new definition.
    Narrowing,
}

fn slug(value: &str) -> bool {
    let mut chars = value.bytes();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-')
}

fn reference(value: &str) -> bool {
    !value.trim().is_empty() && !value.chars().any(char::is_whitespace)
}

fn check(
    violations: &mut Vec<FieldViolation>,
    valid: bool,
    field: &str,
    rule: Rule,
    message: &str,
) {
    if !valid {
        violations.push(FieldViolation::new(field, rule, message));
    }
}

fn bounds<T: PartialOrd + Copy>(min: Option<T>, max: Option<T>) -> bool {
    min.zip(max).is_none_or(|(min, max)| min <= max)
}

fn unit_valid(unit: Option<&str>) -> bool {
    unit.is_none_or(|unit| !unit.is_empty() && ucum::validate(unit).is_ok())
}

/// Validate an owner-independent definition before storage.
///
/// # Errors
/// Returns every detected schema violation. Empty ids are allowed before the
/// owner mints an id; changing an existing id is never allowed.
pub fn validate_definition(definition: &FieldDefinition) -> Result<(), Vec<FieldViolation>> {
    let mut errors = Vec::new();
    check(
        &mut errors,
        definition.id.is_empty() || reference(&definition.id),
        "/id",
        Rule::Format,
        "definition id must be nonblank and contain no whitespace",
    );
    check(
        &mut errors,
        slug(&definition.key),
        "/key",
        Rule::Format,
        "key must be a lowercase slug",
    );
    check(
        &mut errors,
        definition
            .label
            .as_option()
            .is_some_and(|label| !label.text.trim().is_empty()),
        "/label/text",
        Rule::Format,
        "label must be nonblank",
    );
    check(
        &mut errors,
        matches!(definition.classification.to_i32(), 1..=4),
        "/classification",
        Rule::Format,
        "classification must be specified",
    );
    if let Some(meaning) = &definition.meaning {
        check(
            &mut errors,
            meaning.strip_prefix("schema:").is_some_and(reference),
            "/meaning",
            Rule::Format,
            "meaning must name a schema term",
        );
    }
    let arm_matches = matches!(
        (definition.r#type.as_known(), &definition.constraints),
        (
            Some(
                FieldType::FIELD_TYPE_BOOLEAN
                    | FieldType::FIELD_TYPE_DATE
                    | FieldType::FIELD_TYPE_TIMESTAMP
                    | FieldType::FIELD_TYPE_DURATION
                    | FieldType::FIELD_TYPE_PERSON_REFERENCE
            ),
            None
        ) | (
            Some(FieldType::FIELD_TYPE_NUMBER),
            Some(Constraints::Number(_))
        ) | (
            Some(FieldType::FIELD_TYPE_INTEGER),
            Some(Constraints::Integer(_))
        ) | (
            Some(FieldType::FIELD_TYPE_ENUM),
            Some(Constraints::Enumeration(_))
        ) | (Some(FieldType::FIELD_TYPE_TEXT), Some(Constraints::Text(_)))
            | (
                Some(FieldType::FIELD_TYPE_MONEY),
                Some(Constraints::Money(_))
            )
            | (
                Some(FieldType::FIELD_TYPE_RECORD_LINK),
                Some(Constraints::RecordLink(_))
            )
    );
    check(
        &mut errors,
        arm_matches,
        "/constraints",
        Rule::TypeMismatch,
        "constraints must match field type",
    );
    errors.extend(validate_numeric_constraints(
        definition.constraints.as_ref(),
    ));
    errors.extend(validate_other_constraints(definition.constraints.as_ref()));
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn validate_numeric_constraints(constraints: Option<&Constraints>) -> Vec<FieldViolation> {
    let mut errors = Vec::new();
    match constraints {
        Some(Constraints::Number(c)) => {
            check(
                &mut errors,
                c.min.is_none_or(f64::is_finite) && c.max.is_none_or(f64::is_finite),
                "/number",
                Rule::NonFinite,
                "bounds must be finite",
            );
            check(
                &mut errors,
                bounds(c.min, c.max),
                "/number",
                Rule::Range,
                "minimum must not exceed maximum",
            );
            check(
                &mut errors,
                unit_valid(c.unit.as_deref()),
                "/number/unit",
                Rule::Format,
                "unit must parse as UCUM",
            );
        }
        Some(Constraints::Integer(c)) => {
            check(
                &mut errors,
                bounds(c.min, c.max),
                "/integer",
                Rule::Range,
                "minimum must not exceed maximum",
            );
            check(
                &mut errors,
                unit_valid(c.unit.as_deref()),
                "/integer/unit",
                Rule::Format,
                "unit must parse as UCUM",
            );
        }
        Some(Constraints::Money(c)) => {
            check(
                &mut errors,
                iso_currency::Currency::from_code(&c.currency).is_some(),
                "/money/currency",
                Rule::Format,
                "currency must be ISO 4217",
            );
            check(
                &mut errors,
                bounds(c.min_minor, c.max_minor),
                "/money",
                Rule::Range,
                "minimum must not exceed maximum",
            );
        }
        _ => {}
    }
    errors
}

fn validate_other_constraints(constraints: Option<&Constraints>) -> Vec<FieldViolation> {
    let mut errors = Vec::new();
    match constraints {
        Some(Constraints::Enumeration(c)) => {
            check(
                &mut errors,
                (1..=200).contains(&c.options.len()),
                "/enumeration/options",
                Rule::Range,
                "enum requires 1..200 options",
            );
            let mut ids = HashSet::new();
            for (index, option) in c.options.iter().enumerate() {
                check(
                    &mut errors,
                    slug(&option.id) && ids.insert(&option.id),
                    &format!("/enumeration/options/{index}/id"),
                    Rule::Format,
                    "option id must be a unique immutable slug",
                );
                check(
                    &mut errors,
                    option
                        .label
                        .as_option()
                        .is_some_and(|label| !label.text.trim().is_empty()),
                    &format!("/enumeration/options/{index}/label/text"),
                    Rule::Format,
                    "option label must be nonblank",
                );
            }
        }
        Some(Constraints::Text(c)) => {
            check(
                &mut errors,
                (1..=4096).contains(&c.max_length.unwrap_or_default()),
                "/text/maxLength",
                Rule::Range,
                "text limit must be 1..4096",
            );
            check(
                &mut errors,
                matches!(
                    c.format.as_known(),
                    Some(
                        TextFormat::TEXT_FORMAT_PLAIN
                            | TextFormat::TEXT_FORMAT_EMAIL
                            | TextFormat::TEXT_FORMAT_URI
                            | TextFormat::TEXT_FORMAT_PHONE_E164
                    )
                ),
                "/text/format",
                Rule::Format,
                "text format must be specified",
            );
        }
        Some(Constraints::RecordLink(c)) => check(
            &mut errors,
            reference(&c.target_record_type),
            "/recordLink/targetRecordType",
            Rule::Format,
            "record target type must be nonblank",
        ),
        _ => {}
    }
    errors
}

fn in_range<T: PartialOrd + Copy>(value: T, min: Option<T>, max: Option<T>) -> bool {
    min.is_none_or(|min| value >= min) && max.is_none_or(|max| value <= max)
}

fn decimal_places(number: f64) -> usize {
    let text = number.to_string();
    let (mantissa, exponent) = text
        .split_once(['e', 'E'])
        .map_or((text.as_str(), 0_i32), |(m, e)| (m, e.parse().unwrap_or(0)));
    let fractional = mantissa.split_once('.').map_or(0, |(_, tail)| tail.len());
    usize::try_from(i64::try_from(fractional).unwrap_or(i64::MAX) - i64::from(exponent))
        .unwrap_or(0)
}

fn date_valid(date: &crate::CalendarDate) -> bool {
    let leap = date.year % 4 == 0 && (date.year % 100 != 0 || date.year % 400 == 0);
    let days = match date.month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    (1..=9999).contains(&date.year) && (1..=days).contains(&date.day)
}

fn text_valid(text: &str, format: Option<TextFormat>) -> bool {
    match format {
        Some(TextFormat::TEXT_FORMAT_PLAIN) => true,
        Some(TextFormat::TEXT_FORMAT_EMAIL) => email_address::EmailAddress::is_valid(text),
        Some(TextFormat::TEXT_FORMAT_URI) => url::Url::parse(text).is_ok(),
        Some(TextFormat::TEXT_FORMAT_PHONE_E164) => text.strip_prefix('+').is_some_and(|number| {
            (2..=15).contains(&number.len())
                && !number.starts_with('0')
                && number.bytes().all(|c| c.is_ascii_digit())
        }),
        _ => false,
    }
}

fn require(valid: bool, rule: Rule, message: &str) -> Result<(), FieldViolation> {
    if valid {
        Ok(())
    } else {
        Err(FieldViolation::new("/value", rule, message))
    }
}

fn validate_number(c: &crate::NumberConstraints, n: f64) -> Result<(), FieldViolation> {
    require(n.is_finite(), Rule::NonFinite, "number must be finite")?;
    require(
        in_range(n, c.min, c.max),
        Rule::Range,
        "number exceeds inclusive bounds",
    )?;
    require(
        c.max_fraction_digits
            .is_none_or(|digits| decimal_places(n) <= digits as usize),
        Rule::FractionDigits,
        "number exceeds fractional digit limit",
    )
}

fn validate_enum(c: &crate::EnumConstraints, id: &str) -> Result<(), FieldViolation> {
    let option = c
        .options
        .iter()
        .find(|option| option.id == id)
        .ok_or_else(|| FieldViolation::new("/value", Rule::UnknownOption, "unknown enum option"))?;
    require(
        !option.archived,
        Rule::OptionArchived,
        "enum option is archived",
    )
}

fn validate_text(c: &crate::TextConstraints, text: &str) -> Result<(), FieldViolation> {
    require(
        text.chars().count() <= c.max_length.unwrap_or_default() as usize,
        Rule::MaxLength,
        "text exceeds Unicode scalar limit",
    )?;
    require(
        text_valid(text, c.format.as_known()),
        Rule::Format,
        "text does not match declared format",
    )
}

fn validate_money(
    c: &crate::MoneyConstraints,
    amount: &crate::MoneyAmount,
) -> Result<(), FieldViolation> {
    require(
        amount.currency == c.currency,
        Rule::CurrencyMismatch,
        "currency differs from definition",
    )?;
    require(
        in_range(amount.minor_units, c.min_minor, c.max_minor),
        Rule::Range,
        "money exceeds inclusive minor-unit bounds",
    )
}

/// Validate a new value against its resolved definition.
///
/// # Errors
/// Returns one stable violation; archived definitions reject every new write.
pub fn validate_value(
    definition: &FieldDefinition,
    value: &TypedValue,
) -> Result<(), FieldViolation> {
    let failure = |rule, message| FieldViolation::new("/value", rule, message);
    if definition.archived {
        return Err(failure(Rule::DefinitionArchived, "definition is archived"));
    }
    match (
        definition.r#type.as_known(),
        &definition.constraints,
        &value.value,
    ) {
        (Some(FieldType::FIELD_TYPE_BOOLEAN), None, Some(Value::Boolean(_))) => Ok(()),
        (
            Some(FieldType::FIELD_TYPE_NUMBER),
            Some(Constraints::Number(c)),
            Some(Value::Number(n)),
        ) => validate_number(c, *n),
        (
            Some(FieldType::FIELD_TYPE_INTEGER),
            Some(Constraints::Integer(c)),
            Some(Value::Integer(n)),
        ) => require(
            in_range(*n, c.min, c.max),
            Rule::Range,
            "integer exceeds inclusive bounds",
        ),
        (
            Some(FieldType::FIELD_TYPE_ENUM),
            Some(Constraints::Enumeration(c)),
            Some(Value::EnumOptionId(id)),
        ) => validate_enum(c, id),
        (Some(FieldType::FIELD_TYPE_TEXT), Some(Constraints::Text(c)), Some(Value::Text(text))) => {
            validate_text(c, text)
        }
        (Some(FieldType::FIELD_TYPE_DATE), None, Some(Value::Date(date))) => {
            require(date_valid(date), Rule::Format, "invalid calendar date")
        }
        (Some(FieldType::FIELD_TYPE_TIMESTAMP), None, Some(Value::Timestamp(time))) => require(
            (-62_135_596_800..=253_402_300_799).contains(&time.seconds)
                && (0..=999_999_999).contains(&time.nanos),
            Rule::Format,
            "invalid protobuf timestamp",
        ),
        (Some(FieldType::FIELD_TYPE_DURATION), None, Some(Value::Duration(time))) => require(
            (-315_576_000_000..=315_576_000_000).contains(&time.seconds)
                && (-999_999_999..=999_999_999).contains(&time.nanos)
                && (time.seconds == 0
                    || time.nanos == 0
                    || time.seconds.signum() == i64::from(time.nanos.signum())),
            Rule::Format,
            "invalid protobuf duration",
        ),
        (
            Some(FieldType::FIELD_TYPE_MONEY),
            Some(Constraints::Money(c)),
            Some(Value::Money(amount)),
        ) => validate_money(c, amount),
        (Some(FieldType::FIELD_TYPE_PERSON_REFERENCE), None, Some(Value::PersonId(id)))
        | (
            Some(FieldType::FIELD_TYPE_RECORD_LINK),
            Some(Constraints::RecordLink(_)),
            Some(Value::RecordId(id)),
        ) => require(
            reference(id),
            Rule::Format,
            "reference id must be nonblank and contain no whitespace",
        ),
        _ => Err(failure(
            Rule::TypeMismatch,
            "value arm differs from definition type",
        )),
    }
}

fn narrowed<T: PartialOrd>(
    old_min: Option<T>,
    old_max: Option<T>,
    new_min: Option<T>,
    new_max: Option<T>,
) -> bool {
    new_min.is_some_and(|new| old_min.is_none_or(|old| new > old))
        || new_max.is_some_and(|new| old_max.is_none_or(|old| new < old))
}

/// Validate a schema update while preserving stable identities.
///
/// # Errors
/// Returns schema violations or immutable identity/type/key/currency changes
/// and removed options. Narrowing requires the owner's stored-value check.
pub fn validate_definition_change(
    old: &FieldDefinition,
    new: &FieldDefinition,
) -> Result<DefinitionChange, Vec<FieldViolation>> {
    validate_definition(new)?;
    let mut errors = Vec::new();
    check(
        &mut errors,
        old.id == new.id,
        "/id",
        Rule::Format,
        "definition id is immutable",
    );
    check(
        &mut errors,
        old.key == new.key,
        "/key",
        Rule::Format,
        "definition key is immutable",
    );
    check(
        &mut errors,
        old.r#type == new.r#type,
        "/type",
        Rule::TypeMismatch,
        "definition type is immutable",
    );
    let narrowing = match (&old.constraints, &new.constraints) {
        (Some(Constraints::Number(a)), Some(Constraints::Number(b))) => {
            narrowed(a.min, a.max, b.min, b.max)
                || b.max_fraction_digits
                    .is_some_and(|digits| a.max_fraction_digits.is_none_or(|old| digits < old))
        }
        (Some(Constraints::Integer(a)), Some(Constraints::Integer(b))) => {
            narrowed(a.min, a.max, b.min, b.max)
        }
        (Some(Constraints::Enumeration(a)), Some(Constraints::Enumeration(b))) => {
            for option in &a.options {
                check(
                    &mut errors,
                    b.options.iter().any(|candidate| candidate.id == option.id),
                    "/enumeration/options",
                    Rule::UnknownOption,
                    "archive enum options instead of removing them",
                );
            }
            false
        }
        (Some(Constraints::Text(a)), Some(Constraints::Text(b))) => {
            b.max_length.unwrap_or_default() < a.max_length.unwrap_or_default()
                || (a.format != b.format
                    && b.format.as_known() != Some(TextFormat::TEXT_FORMAT_PLAIN))
        }
        (Some(Constraints::Money(a)), Some(Constraints::Money(b))) => {
            check(
                &mut errors,
                a.currency == b.currency,
                "/money/currency",
                Rule::CurrencyMismatch,
                "currency is immutable",
            );
            narrowed(a.min_minor, a.max_minor, b.min_minor, b.max_minor)
        }
        _ => false,
    };
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(if old == new {
        DefinitionChange::Unchanged
    } else if narrowing {
        DefinitionChange::Narrowing
    } else {
        DefinitionChange::Compatible
    })
}
