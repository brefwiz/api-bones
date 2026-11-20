// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! The hand-written `ErrorInfo` / `ValidationFailureDetail` codecs against the
//! generated `bones.v1` messages, byte for byte in both directions.

use api_bones::connect::{ErrorArg, ErrorInfo, ValidationFailureDetail};
use buffa::Message as _;
use buffa_types::google::protobuf::Timestamp;

use crate::field_wire::{self, error_arg::Value};

fn wire_arg(arg: &ErrorArg) -> field_wire::ErrorArg {
    let value = match arg {
        ErrorArg::String(v) => Value::String(v.clone()),
        ErrorArg::Int64(v) => Value::Int64(*v),
        ErrorArg::Double(v) => Value::Double(*v),
        ErrorArg::Timestamp { seconds, nanos } => Value::Timestamp(Box::new(Timestamp {
            seconds: *seconds,
            nanos: *nanos,
            ..Default::default()
        })),
    };
    field_wire::ErrorArg {
        value: Some(value),
        ..Default::default()
    }
}

fn hand_arg(arg: &field_wire::ErrorArg) -> ErrorArg {
    match arg.value.as_ref().expect("a value is set") {
        Value::String(v) => ErrorArg::String(v.clone()),
        Value::Int64(v) => ErrorArg::Int64(*v),
        Value::Double(v) => ErrorArg::Double(*v),
        Value::Timestamp(t) => ErrorArg::Timestamp {
            seconds: t.seconds,
            nanos: t.nanos,
        },
    }
}

fn args() -> std::collections::BTreeMap<String, ErrorArg> {
    [
        ("seat", ErrorArg::from("12A")),
        ("empty", ErrorArg::from("")),
        ("row", ErrorArg::Int64(-4)),
        ("big", ErrorArg::Int64(i64::MAX)),
        ("price", ErrorArg::Double(-12.5)),
        (
            "before_epoch",
            ErrorArg::Timestamp {
                seconds: -86_400,
                nanos: 250,
            },
        ),
        (
            "after_epoch",
            ErrorArg::Timestamp {
                seconds: 1_700_000_000,
                nanos: 999_999_999,
            },
        ),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect()
}

fn wire_args() -> Vec<(String, field_wire::ErrorArg)> {
    args()
        .iter()
        .map(|(k, v)| (k.clone(), wire_arg(v)))
        .collect()
}

#[test]
fn hand_written_error_info_decodes_what_the_generated_message_encodes() {
    let wire = field_wire::ErrorInfo {
        code: "SEAT_TAKEN".into(),
        emitter: "bookings".into(),
        args: wire_args().into_iter().collect(),
        ..Default::default()
    };
    let decoded = ErrorInfo::decode(&wire.encode_to_vec()).expect("decodes");
    assert_eq!(decoded.code, "SEAT_TAKEN");
    assert_eq!(decoded.emitter, "bookings");
    assert_eq!(decoded.args, args());
}

#[test]
fn generated_error_info_decodes_what_the_hand_written_codec_encodes() {
    let info = ErrorInfo {
        code: "SEAT_TAKEN".into(),
        emitter: "bookings".into(),
        args: args(),
    };
    let wire = field_wire::ErrorInfo::decode_from_slice(&info.encode()).expect("decodes");
    assert_eq!(wire.code, "SEAT_TAKEN");
    assert_eq!(wire.emitter, "bookings");
    assert_eq!(wire.args.len(), args().len());
    for (name, arg) in &wire.args {
        assert_eq!(hand_arg(arg), args()[name], "{name}");
    }
}

#[test]
fn error_info_bytes_are_identical_apart_from_map_order() {
    let info = ErrorInfo::from_code("A").with_arg("n", 5_i64).unwrap();
    let wire = field_wire::ErrorInfo {
        code: "A".into(),
        args: [("n".to_owned(), wire_arg(&ErrorArg::Int64(5)))]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    assert_eq!(info.encode(), wire.encode_to_vec());
}

#[test]
fn hand_written_validation_failure_decodes_what_the_generated_message_encodes() {
    let wire = field_wire::ValidationFailure {
        field: "/guests".into(),
        message: "must be at most 4".into(),
        rule: "max".into(),
        args: wire_args().into_iter().collect(),
        ..Default::default()
    };
    let decoded = ValidationFailureDetail::decode(&wire.encode_to_vec()).expect("decodes");
    assert_eq!(decoded.field, "/guests");
    assert_eq!(decoded.message, "must be at most 4");
    assert_eq!(decoded.rule, "max");
    assert_eq!(decoded.args, args());
}

#[test]
fn generated_validation_failure_decodes_what_the_hand_written_codec_encodes() {
    let failure = ValidationFailureDetail {
        field: "/guests".into(),
        message: "must be at most 4".into(),
        rule: "max".into(),
        args: args(),
    };
    let wire =
        field_wire::ValidationFailure::decode_from_slice(&failure.encode()).expect("decodes");
    assert_eq!(wire.field, "/guests");
    assert_eq!(wire.rule, "max");
    assert_eq!(wire.args.len(), args().len());
    for (name, arg) in &wire.args {
        assert_eq!(hand_arg(arg), args()[name], "{name}");
    }
}

#[test]
fn a_generated_entry_with_no_value_member_is_dropped_by_the_hand_written_codec() {
    let wire = field_wire::ErrorInfo {
        code: "A".into(),
        args: [("n".to_owned(), field_wire::ErrorArg::default())]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    let decoded = ErrorInfo::decode(&wire.encode_to_vec()).expect("decodes");
    assert!(decoded.args.is_empty());
}

/// The member names carry no `_value` suffix, so the JSON names are pinned in
/// the proto; the generated message and the hand-written type must agree on
/// them.
#[test]
fn generated_json_names_match_the_hand_written_problem_json_members() {
    let expected = [
        ("seat", "stringValue"),
        ("row", "int64Value"),
        ("price", "doubleValue"),
        ("after_epoch", "timestampValue"),
    ];
    for (name, member) in expected {
        let hand = &args()[name];
        let wire = wire_arg(hand);
        let json = serde_json::to_value(&wire).expect("generated message serializes");
        assert_eq!(
            json,
            serde_json::to_value(hand).expect("hand-written type serializes"),
            "{name}"
        );
        assert!(
            json.get(member).is_some(),
            "{name} is keyed `{member}`: {json}"
        );
        let back: field_wire::ErrorArg = serde_json::from_value(json).expect("round-trips");
        assert_eq!(hand_arg(&back), *hand, "{name}");
    }
}
