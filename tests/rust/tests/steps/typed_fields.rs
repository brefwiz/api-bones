// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! Each When calls a shipped field validator; every Then reads its decision.
use api_bones_fields::{
    CalendarDate, DataClassification, DefinitionChange, EnumConstraints, EnumOption,
    FieldDefinition, FieldType, IntegerConstraints, Label, MoneyAmount, MoneyConstraints,
    NumberConstraints, RecordLinkConstraints, TextConstraints, TextFormat, TypedValue,
    field_definition::Constraints, typed_value::Value,
};
use cucumber::{given, then, when};

use crate::fields_world::FieldsWorld;

fn label(text: &str) -> Label {
    Label {
        text: text.into(),
        ..Default::default()
    }
}
fn option(id: &str, archived: bool) -> EnumOption {
    EnumOption {
        id: id.into(),
        label: label(id).into(),
        archived,
        ..Default::default()
    }
}

#[given(expr = "a typed field for {string}")]
fn field(world: &mut FieldsWorld, kind: String) {
    let (kind, constraints) = match kind.as_str() {
        "boolean" => (FieldType::FIELD_TYPE_BOOLEAN, None),
        "number" => (
            FieldType::FIELD_TYPE_NUMBER,
            Some(Constraints::Number(
                NumberConstraints {
                    min: Some(0.0),
                    max: Some(100.0),
                    max_fraction_digits: Some(2),
                    ..Default::default()
                }
                .into(),
            )),
        ),
        "integer" => (
            FieldType::FIELD_TYPE_INTEGER,
            Some(Constraints::Integer(
                IntegerConstraints {
                    min: Some(0),
                    max: Some(100),
                    ..Default::default()
                }
                .into(),
            )),
        ),
        "enum" => (
            FieldType::FIELD_TYPE_ENUM,
            Some(Constraints::Enumeration(
                EnumConstraints {
                    options: vec![option("ready", false), option("retired", true)],
                    ..Default::default()
                }
                .into(),
            )),
        ),
        "text" | "email" | "uri" | "phone" => {
            let (max_length, format) = match kind.as_str() {
                "text" => (5, TextFormat::TEXT_FORMAT_PLAIN),
                "email" => (256, TextFormat::TEXT_FORMAT_EMAIL),
                "uri" => (256, TextFormat::TEXT_FORMAT_URI),
                _ => (16, TextFormat::TEXT_FORMAT_PHONE_E164),
            };
            (
                FieldType::FIELD_TYPE_TEXT,
                Some(Constraints::Text(
                    TextConstraints {
                        max_length,
                        format: format.into(),
                        ..Default::default()
                    }
                    .into(),
                )),
            )
        }
        "date" => (FieldType::FIELD_TYPE_DATE, None),
        "timestamp" => (FieldType::FIELD_TYPE_TIMESTAMP, None),
        "duration" => (FieldType::FIELD_TYPE_DURATION, None),
        "money" => (
            FieldType::FIELD_TYPE_MONEY,
            Some(Constraints::Money(
                MoneyConstraints {
                    currency: "USD".into(),
                    min_minor: Some(0),
                    max_minor: Some(10000),
                    ..Default::default()
                }
                .into(),
            )),
        ),
        "person_reference" => (FieldType::FIELD_TYPE_PERSON_REFERENCE, None),
        "record_link" => (
            FieldType::FIELD_TYPE_RECORD_LINK,
            Some(Constraints::RecordLink(
                RecordLinkConstraints {
                    target_record_type: "booking".into(),
                    ..Default::default()
                }
                .into(),
            )),
        ),
        other => panic!("unknown field type {other}"),
    };
    world.definition = FieldDefinition {
        id: "field-1".into(),
        key: "example".into(),
        label: label("Example").into(),
        r#type: kind.into(),
        classification: DataClassification::DATA_CLASSIFICATION_INTERNAL.into(),
        constraints,
        ..Default::default()
    };
}

fn setting(definition: &mut FieldDefinition, name: &str, value: &str) {
    match name {
        "id" => definition.id = value.into(),
        "key" => definition.key = value.into(),
        "label" => definition.label = label(value).into(),
        "classification" => {
            definition.classification =
                buffa::EnumValue::from(value.parse::<i32>().expect("classification number"))
        }
        "archived" => definition.archived = value.parse().expect("archived bool"),
        "constraints" => {
            assert_eq!(value, "none");
            definition.constraints = None;
        }
        "type" => {
            assert_eq!(value, "integer");
            definition.r#type = FieldType::FIELD_TYPE_INTEGER.into();
            definition.constraints = Some(Constraints::Integer(Box::default()));
        }
        _ => match definition.constraints.as_mut().expect("constraints") {
            Constraints::Number(c) => match name {
                "min" => c.min = Some(value.parse().expect("number")),
                "max" => c.max = Some(value.parse().expect("number")),
                "unit" => c.unit = Some(value.into()),
                "fraction" => c.max_fraction_digits = Some(value.parse().expect("fraction digits")),
                _ => panic!("unknown number setting {name}"),
            },
            Constraints::Text(c) => {
                assert_eq!(name, "max_length");
                c.max_length = value.parse().expect("max length");
            }
            Constraints::Money(c) => match name {
                "currency" => c.currency = value.into(),
                "max" => c.max_minor = Some(value.parse().expect("minor units")),
                _ => panic!("unknown money setting {name}"),
            },
            Constraints::RecordLink(c) => {
                assert_eq!(name, "target_type");
                c.target_record_type = value.into();
            }
            Constraints::Enumeration(c) => match name {
                "option_id" => c.options[1].id = value.into(),
                "option_label" => c.options[0].label = label(value).into(),
                "remove_option" => c.options.retain(|option| option.id != value),
                "archive_option" => {
                    c.options
                        .iter_mut()
                        .find(|option| option.id == value)
                        .expect("option")
                        .archived = true
                }
                "add_option" => c.options.push(option(value, false)),
                "option_count" => {
                    c.options = (0..value.parse().expect("option count"))
                        .map(|i| option(&format!("option-{i}"), false))
                        .collect()
                }
                _ => panic!("unknown enum setting {name}"),
            },
            _ => panic!("unknown field setting {name}"),
        },
    }
}

#[given(expr = "the field setting {string} is {string}")]
fn configure(world: &mut FieldsWorld, name: String, value: String) {
    setting(&mut world.definition, &name, &value);
}

fn typed_value(input: &str) -> TypedValue {
    let value = if input == "unset" {
        None
    } else {
        let (kind, text) = input.split_once(':').expect("typed input");
        Some(match kind {
            "boolean" => Value::Boolean(text.parse().expect("boolean")),
            "number" => Value::Number(text.parse().expect("number")),
            "integer" => Value::Integer(text.parse().expect("integer")),
            "enum" => Value::EnumOptionId(text.into()),
            "text" => Value::Text(text.into()),
            "date" => {
                let parts: Vec<_> = text.split('-').collect();
                Value::Date(
                    CalendarDate {
                        year: parts[0].parse().expect("year"),
                        month: parts[1].parse().expect("month"),
                        day: parts[2].parse().expect("day"),
                        ..Default::default()
                    }
                    .into(),
                )
            }
            "timestamp" => {
                let (s, n) = text.split_once(':').expect("timestamp");
                Value::Timestamp(
                    buffa_types::google::protobuf::Timestamp {
                        seconds: s.parse().expect("seconds"),
                        nanos: n.parse().expect("nanos"),
                        ..Default::default()
                    }
                    .into(),
                )
            }
            "duration" => {
                let (s, n) = text.split_once(':').expect("duration");
                Value::Duration(
                    buffa_types::google::protobuf::Duration {
                        seconds: s.parse().expect("seconds"),
                        nanos: n.parse().expect("nanos"),
                        ..Default::default()
                    }
                    .into(),
                )
            }
            "money" => {
                let (n, currency) = text.split_once(':').expect("money");
                Value::Money(
                    MoneyAmount {
                        minor_units: n.parse().expect("minor units"),
                        currency: currency.into(),
                        ..Default::default()
                    }
                    .into(),
                )
            }
            "person" => Value::PersonId(text.into()),
            "record" => Value::RecordId(text.into()),
            _ => panic!("unknown value type {kind}"),
        })
    };
    TypedValue {
        value,
        ..Default::default()
    }
}

#[when(expr = "the owner validates value {string}")]
fn validate_value(world: &mut FieldsWorld, input: String) {
    let value = typed_value(&input);
    let validation = api_bones_fields::validate_value(&world.definition, &value);
    world.violations = Some(validation.err().into_iter().collect());
    world.change = None;
}

#[when(expr = "the owner validates the definition")]
fn validate_definition(world: &mut FieldsWorld) {
    let validation = api_bones_fields::validate_definition(&world.definition);
    world.violations = Some(validation.err().unwrap_or_default());
    world.change = None;
}

#[when(expr = "the owner changes field setting {string} to {string}")]
fn change(world: &mut FieldsWorld, name: String, value: String) {
    let previous = world.definition.clone();
    setting(&mut world.definition, &name, &value);
    let validation = api_bones_fields::validate_definition_change(&previous, &world.definition);
    world.violations = Some(validation.clone().err().unwrap_or_default());
    world.change = validation.ok();
}

#[then(expr = "the SDK field result has rule {string}")]
fn rule(world: &mut FieldsWorld, expected: String) {
    let violations = world
        .violations
        .as_ref()
        .expect("validator returned a decision");
    assert_eq!(violations.is_empty(), expected.is_empty(), "{violations:?}");
    assert!(
        expected.is_empty()
            || violations
                .iter()
                .any(|violation| violation.rule.as_str() == expected)
    );
    assert!(
        violations
            .iter()
            .all(|violation| violation.field.starts_with('/') && !violation.message.is_empty())
    );
}

#[then(expr = "the SDK field change is {string}")]
fn change_result(world: &mut FieldsWorld, expected: String) {
    let change = match expected.as_str() {
        "" => None,
        "unchanged" => Some(DefinitionChange::Unchanged),
        "compatible" => Some(DefinitionChange::Compatible),
        "narrowing" => Some(DefinitionChange::Narrowing),
        other => panic!("unknown definition change {other}"),
    };
    assert_eq!(world.change, change);
}
