// SPDX-License-Identifier: MIT
//! Schema and value invariants exercised without IO.
use super::*;
use buffa::Message;
use field_definition::Constraints;
use typed_value::Value;

fn definition(kind: FieldType, constraints: Option<Constraints>) -> FieldDefinition {
    FieldDefinition {
        id: "field-1".into(),
        key: "example".into(),
        label: buffa::MessageField::some(Label {
            text: "Example".into(),
            ..Default::default()
        }),
        r#type: kind.into(),
        classification: DataClassification::DATA_CLASSIFICATION_INTERNAL.into(),
        constraints,
        ..Default::default()
    }
}

fn number() -> FieldDefinition {
    definition(
        FieldType::FIELD_TYPE_NUMBER,
        Some(Constraints::Number(Box::new(NumberConstraints {
            min: Some(0.0),
            max: Some(100.0),
            unit: Some("kg/m2".into()),
            max_fraction_digits: Some(2),
            ..Default::default()
        }))),
    )
}

fn value(value: Value) -> TypedValue {
    TypedValue {
        value: Some(value),
        ..Default::default()
    }
}

#[test]
fn violations_have_closed_wire_tokens_and_preserve_the_json_pointer() {
    let cases = [
        (Rule::TypeMismatch, "type_mismatch"),
        (Rule::Range, "range"),
        (Rule::NonFinite, "non_finite"),
        (Rule::FractionDigits, "fraction_digits"),
        (Rule::UnknownOption, "unknown_option"),
        (Rule::OptionArchived, "option_archived"),
        (Rule::MaxLength, "max_length"),
        (Rule::Format, "format"),
        (Rule::CurrencyMismatch, "currency_mismatch"),
        (Rule::DefinitionArchived, "definition_archived"),
    ];
    for (rule, token) in cases {
        let violation = FieldViolation {
            field: "/fields/3/value".into(),
            message: "invalid value".into(),
            rule,
        };
        let wire: ValidationFailure = violation.into();
        assert_eq!(wire.field, "/fields/3/value");
        assert_eq!(wire.rule, token);
        assert_eq!(wire.message, "invalid value");
    }
}

#[test]
fn boolean_false_is_present_and_wrong_or_absent_arms_are_refused() {
    let definition = definition(FieldType::FIELD_TYPE_BOOLEAN, None);
    assert!(validate_definition(&definition).is_ok());
    assert!(validate_value(&definition, &value(Value::Boolean(false))).is_ok());
    for wrong in [TypedValue::default(), value(Value::Text("false".into()))] {
        assert_eq!(
            validate_value(&definition, &wrong).unwrap_err().rule,
            Rule::TypeMismatch
        );
    }
}

#[test]
fn numbers_use_inclusive_finite_bounds_and_decimal_fraction_digits() {
    let definition = number();
    assert!(validate_definition(&definition).is_ok());
    for accepted in [0.0, 100.0, 0.29, 1.01, 12.25] {
        assert!(validate_value(&definition, &value(Value::Number(accepted))).is_ok());
    }
    for (rejected, rule) in [
        (f64::NAN, Rule::NonFinite),
        (f64::INFINITY, Rule::NonFinite),
        (-0.01, Rule::Range),
        (100.01, Rule::Range),
        (0.001, Rule::FractionDigits),
    ] {
        assert_eq!(
            validate_value(&definition, &value(Value::Number(rejected)))
                .unwrap_err()
                .rule,
            rule
        );
    }
}

#[test]
fn definition_reports_multiple_schema_errors_and_validates_ucum_and_iso_currency() {
    let mut invalid = number();
    invalid.key = "Bad Key".into();
    invalid.label = buffa::MessageField::none();
    invalid.classification = DataClassification::DATA_CLASSIFICATION_UNSPECIFIED.into();
    if let Some(Constraints::Number(constraints)) = &mut invalid.constraints {
        constraints.unit = Some("made_up_unit".into());
        constraints.min = Some(101.0);
    }
    let errors = validate_definition(&invalid).unwrap_err();
    assert!(errors.iter().any(|error| error.field == "/key"));
    assert!(errors.iter().any(|error| error.field == "/label/text"));
    assert!(errors.iter().any(|error| error.field == "/classification"));
    assert!(errors.iter().any(|error| error.field == "/number/unit"));
    assert!(errors.iter().any(|error| error.rule == Rule::Range));
    for unit in ["m", "kg", "h", "{seat}", "kg/m2"] {
        let mut valid = number();
        if let Some(Constraints::Number(constraints)) = &mut valid.constraints {
            constraints.unit = Some(unit.into());
        }
        assert!(validate_definition(&valid).is_ok(), "{unit}");
    }
    let money = definition(
        FieldType::FIELD_TYPE_MONEY,
        Some(Constraints::Money(Box::new(MoneyConstraints {
            currency: "ZZZ".into(),
            ..Default::default()
        }))),
    );
    assert!(
        validate_definition(&money)
            .unwrap_err()
            .iter()
            .any(|error| error.field == "/money/currency")
    );
}

#[test]
fn definition_changes_distinguish_compatible_narrowing_and_immutable_identity() {
    let original = number();
    assert_eq!(
        validate_definition_change(&original, &original),
        Ok(DefinitionChange::Unchanged)
    );
    let mut changed = original.clone();
    changed.label.as_option_mut().unwrap().text = "Renamed".into();
    assert_eq!(
        validate_definition_change(&original, &changed),
        Ok(DefinitionChange::Compatible)
    );
    if let Some(Constraints::Number(c)) = &mut changed.constraints {
        c.max = Some(50.0);
    }
    assert_eq!(
        validate_definition_change(&original, &changed),
        Ok(DefinitionChange::Narrowing)
    );
    changed = original.clone();
    if let Some(Constraints::Number(c)) = &mut changed.constraints {
        c.max = None;
    }
    assert_eq!(
        validate_definition_change(&original, &changed),
        Ok(DefinitionChange::Compatible)
    );
    changed.id = "field-2".into();
    assert!(
        validate_definition_change(&original, &changed)
            .unwrap_err()
            .iter()
            .any(|error| error.field == "/id")
    );
    changed = original.clone();
    changed.key = "other".into();
    assert!(validate_definition_change(&original, &changed).is_err());
}

#[test]
fn enum_ids_survive_label_changes_but_cannot_be_removed_or_reused() {
    let option = EnumOption {
        id: "ready".into(),
        label: buffa::MessageField::some(Label {
            text: "Ready".into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    let original = definition(
        FieldType::FIELD_TYPE_ENUM,
        Some(Constraints::Enumeration(Box::new(EnumConstraints {
            options: vec![option.clone()],
            ..Default::default()
        }))),
    );
    assert!(validate_definition(&original).is_ok());
    let mut changed = original.clone();
    if let Some(Constraints::Enumeration(c)) = &mut changed.constraints {
        c.options[0].label.as_option_mut().unwrap().text = "Available".into();
    }
    assert_eq!(
        validate_definition_change(&original, &changed),
        Ok(DefinitionChange::Compatible)
    );
    assert!(validate_value(&changed, &value(Value::EnumOptionId("ready".into()))).is_ok());
    if let Some(Constraints::Enumeration(c)) = &mut changed.constraints {
        c.options[0].archived = true;
    }
    assert_eq!(
        validate_value(&changed, &value(Value::EnumOptionId("ready".into())))
            .unwrap_err()
            .rule,
        Rule::OptionArchived
    );
    if let Some(Constraints::Enumeration(c)) = &mut changed.constraints {
        c.options.push(option);
    }
    assert!(validate_definition(&changed).is_err());
    if let Some(Constraints::Enumeration(c)) = &mut changed.constraints {
        c.options = vec![EnumOption {
            id: "replacement".into(),
            label: buffa::MessageField::some(Label {
                text: "Replacement".into(),
                ..Default::default()
            }),
            ..Default::default()
        }];
    }
    assert!(validate_definition_change(&original, &changed).is_err());
}

#[test]
fn protobuf_values_preserve_definition_identity_through_round_trip() {
    let calendar = CalendarDate {
        year: 2024,
        month: 2,
        day: 29,
        ..Default::default()
    };
    let amount = MoneyAmount {
        minor_units: 123,
        currency: "USD".into(),
        ..Default::default()
    };
    let cases = [
        Value::Boolean(false),
        Value::Number(1.25),
        Value::Integer(i64::MAX),
        Value::EnumOptionId("ready".into()),
        Value::Text("é".into()),
        Value::Date(Box::new(calendar)),
        Value::Timestamp(Box::default()),
        Value::Duration(Box::default()),
        Value::Money(Box::new(amount)),
        Value::PersonId("person-1".into()),
        Value::RecordId("record-1".into()),
    ];
    for case in cases {
        let field = FieldValue {
            definition_id: "field-1".into(),
            value: buffa::MessageField::some(value(case)),
            ..Default::default()
        };
        let decoded = FieldValue::decode_from_slice(&field.encode_to_vec()).unwrap();
        assert_eq!(decoded, field);
    }
}

#[test]
fn integer_text_money_and_link_constraints_validate_actual_values() {
    let integer = definition(
        FieldType::FIELD_TYPE_INTEGER,
        Some(Constraints::Integer(Box::new(IntegerConstraints {
            min: Some(1),
            max: Some(2),
            unit: Some("{seat}".into()),
            ..Default::default()
        }))),
    );
    assert!(validate_definition(&integer).is_ok());
    assert!(validate_value(&integer, &value(Value::Integer(2))).is_ok());
    assert_eq!(
        validate_value(&integer, &value(Value::Integer(3)))
            .unwrap_err()
            .rule,
        Rule::Range
    );
    let text = definition(
        FieldType::FIELD_TYPE_TEXT,
        Some(Constraints::Text(Box::new(TextConstraints {
            max_length: Some(2),
            format: TextFormat::TEXT_FORMAT_PLAIN.into(),
            ..Default::default()
        }))),
    );
    assert!(validate_value(&text, &value(Value::Text("éé".into()))).is_ok());
    assert_eq!(
        validate_value(&text, &value(Value::Text("ééé".into())))
            .unwrap_err()
            .rule,
        Rule::MaxLength
    );
    let money = definition(
        FieldType::FIELD_TYPE_MONEY,
        Some(Constraints::Money(Box::new(MoneyConstraints {
            currency: "USD".into(),
            min_minor: Some(0),
            max_minor: Some(100),
            ..Default::default()
        }))),
    );
    assert!(validate_definition(&money).is_ok());
    assert!(
        validate_value(
            &money,
            &value(Value::Money(Box::new(MoneyAmount {
                minor_units: 100,
                currency: "USD".into(),
                ..Default::default()
            })))
        )
        .is_ok()
    );
    let link = definition(
        FieldType::FIELD_TYPE_RECORD_LINK,
        Some(Constraints::RecordLink(Box::new(RecordLinkConstraints {
            target_record_type: "booking".into(),
            ..Default::default()
        }))),
    );
    assert!(validate_definition(&link).is_ok());
    assert!(validate_value(&link, &value(Value::RecordId("record-1".into()))).is_ok());
    assert_eq!(
        validate_value(&link, &value(Value::RecordId(String::new())))
            .unwrap_err()
            .rule,
        Rule::Format
    );
}

#[test]
fn calendar_timestamp_duration_and_text_formats_are_validated() {
    let date = definition(FieldType::FIELD_TYPE_DATE, None);
    for (year, accepted) in [(2024, true), (2023, false)] {
        let actual = value(Value::Date(Box::new(CalendarDate {
            year,
            month: 2,
            day: 29,
            ..Default::default()
        })));
        assert_eq!(validate_value(&date, &actual).is_ok(), accepted);
    }
    let timestamp = definition(FieldType::FIELD_TYPE_TIMESTAMP, None);
    assert!(validate_value(&timestamp, &value(Value::Timestamp(Box::default()))).is_ok());
    let duration = definition(FieldType::FIELD_TYPE_DURATION, None);
    assert!(validate_value(&duration, &value(Value::Duration(Box::default()))).is_ok());
    for (format, accepted, refused) in [
        (
            TextFormat::TEXT_FORMAT_EMAIL,
            "ada@example.org",
            "not-email",
        ),
        (
            TextFormat::TEXT_FORMAT_URI,
            "https://example.org",
            "relative/path",
        ),
        (
            TextFormat::TEXT_FORMAT_PHONE_E164,
            "+14155552671",
            "4155552671",
        ),
    ] {
        let text = definition(
            FieldType::FIELD_TYPE_TEXT,
            Some(Constraints::Text(Box::new(TextConstraints {
                max_length: Some(256),
                format: format.into(),
                ..Default::default()
            }))),
        );
        assert!(validate_definition(&text).is_ok());
        assert!(validate_value(&text, &value(Value::Text(accepted.into()))).is_ok());
        assert_eq!(
            validate_value(&text, &value(Value::Text(refused.into())))
                .unwrap_err()
                .rule,
            Rule::Format
        );
    }
}
