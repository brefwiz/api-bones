// SPDX-License-Identifier: MIT
//! `bones.v1.ItemError` as the `ConnectError` the single call raises, and the
//! errors the runtime raises itself.

use api_bones::connect::{ErrorInfo, ValidationFailureDetail, with_error_info};
use connectrpc::{ConnectError, ErrorCode};

/// `ErrorInfo.code` of a whole-call refusal for too many items.
pub const BATCH_TOO_LARGE: &str = "BATCH_TOO_LARGE";
/// `ErrorInfo.code` of an id whose request alone exceeds the size limit.
pub const BATCH_ITEM_TOO_LARGE: &str = "BATCH_ITEM_TOO_LARGE";
/// `ErrorInfo.code` of a response that does not answer the request's ids in order.
pub const BATCH_RESPONSE_MISMATCH: &str = "BATCH_RESPONSE_MISMATCH";
/// `ErrorInfo.code` of a call whose identity changed before it was sent or answered.
pub const BATCH_IDENTITY_CHANGED: &str = "BATCH_IDENTITY_CHANGED";

/// The fields of a generated `bones.v1.ItemError`, read without naming its type.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemErrorParts {
    /// The Connect code token.
    pub code: String,
    /// The message the single call raises for the same cause.
    pub message: String,
    /// The item's `ErrorInfo` with its typed arguments, when it carries one.
    /// An `ErrorInfo` whose arguments break the bounds is treated as absent.
    pub info: Option<ErrorInfo>,
    /// Each `ValidationFailure` with its typed arguments. One whose arguments
    /// break the bounds is dropped.
    pub violations: Vec<ValidationFailureDetail>,
}

/// The error the single call raises for the same cause.
///
/// The code is the Connect token, the message is kept, `info` becomes the
/// `bones.v1.ErrorInfo` detail and each violation a `bones.v1.ValidationFailure`
/// detail, typed arguments included. An unknown token maps to `unknown`,
/// failing closed. Arguments that break the bounds make the result an
/// `internal` error that names the bound.
#[must_use]
pub fn item_error_to_connect(parts: &ItemErrorParts) -> ConnectError {
    let code = parts
        .code
        .parse::<ErrorCode>()
        .unwrap_or(ErrorCode::Unknown);
    let mut error = ConnectError::new(code, parts.message.clone());
    if let Some(info) = &parts.info {
        error = with_error_info(error, info);
    }
    for violation in &parts.violations {
        match violation.to_detail() {
            Ok(detail) => error = error.with_detail(detail),
            Err(bound) => {
                return ConnectError::internal(format!("invalid ValidationFailure args: {bound}"));
            }
        }
    }
    error
}

fn coded(error: ConnectError, code: &str) -> ConnectError {
    with_error_info(error, &ErrorInfo::from_code(code))
}

pub(crate) fn item_too_large(item_bytes: usize) -> ConnectError {
    coded(
        ConnectError::invalid_argument(format!(
            "an item of {item_bytes} bytes does not fit one batch request"
        )),
        BATCH_ITEM_TOO_LARGE,
    )
}

pub(crate) fn response_mismatch() -> ConnectError {
    coded(
        ConnectError::internal("the batch response does not answer the requested ids in order"),
        BATCH_RESPONSE_MISMATCH,
    )
}

pub(crate) fn identity_changed() -> ConnectError {
    coded(
        ConnectError::unauthenticated("the caller identity changed before the batch was sent"),
        BATCH_IDENTITY_CHANGED,
    )
}

pub(crate) fn queue_full() -> ConnectError {
    ConnectError::resource_exhausted("too many batch requests are queued for this identity")
}

pub(crate) fn abandoned() -> ConnectError {
    ConnectError::new(ErrorCode::Canceled, "the batch request was abandoned")
}

pub(crate) fn is_identity_changed(error: &ConnectError) -> bool {
    api_bones::connect::error_info(error).is_some_and(|i| i.code == BATCH_IDENTITY_CHANGED)
}

/// Whether a failed chunk may be retried by a retry policy: `unauthenticated`
/// and an identity change never are.
#[must_use]
pub fn is_never_retried(error: &ConnectError) -> bool {
    error.code == ErrorCode::Unauthenticated || is_identity_changed(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use api_bones::connect::error_info;

    use api_bones::connect::{ErrorArg, VALIDATION_FAILURE_TYPE};

    fn parts(code: &str) -> ItemErrorParts {
        ItemErrorParts {
            code: code.to_owned(),
            message: "no such widget".to_owned(),
            info: Some(
                ErrorInfo {
                    code: "WIDGET_MISSING".to_owned(),
                    emitter: "widgets".to_owned(),
                    ..Default::default()
                }
                .with_arg("widget_id", "w-1")
                .expect("valid arg")
                .with_arg("attempts", 3_i64)
                .expect("valid arg"),
            ),
            violations: vec![
                ValidationFailureDetail {
                    field: "/id".to_owned(),
                    message: "must be a uuid".to_owned(),
                    rule: "format".to_owned(),
                    ..Default::default()
                }
                .with_arg("expected", "uuid")
                .expect("valid arg"),
            ],
        }
    }

    #[test]
    fn code_message_and_info_survive() {
        let error = item_error_to_connect(&parts("not_found"));
        assert_eq!(error.code, ErrorCode::NotFound);
        assert_eq!(error.message.as_deref(), Some("no such widget"));
        let info = error_info(&error).expect("info");
        assert_eq!(
            (info.code.as_str(), info.emitter.as_str()),
            ("WIDGET_MISSING", "widgets")
        );
    }

    #[test]
    fn info_args_survive() {
        let info = error_info(&item_error_to_connect(&parts("not_found"))).expect("info");
        assert_eq!(info.args.len(), 2);
        assert_eq!(info.args["widget_id"], ErrorArg::from("w-1"));
        assert_eq!(info.args["attempts"], ErrorArg::Int64(3));
    }

    #[test]
    fn violations_become_details_with_their_args() {
        let error = item_error_to_connect(&parts("invalid_argument"));
        let failures: Vec<_> = error
            .details
            .iter()
            .filter(|d| d.type_url == VALIDATION_FAILURE_TYPE)
            .collect();
        assert_eq!(failures.len(), 1);
        let bytes = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD_NO_PAD,
            failures[0].value.as_deref().expect("value"),
        )
        .expect("base64");
        let failure = ValidationFailureDetail::decode(&bytes).expect("decodes");
        assert_eq!(failure.rule, "format");
        assert_eq!(failure.args["expected"], ErrorArg::from("uuid"));
    }

    #[test]
    fn out_of_bounds_args_fail_loudly() {
        let mut broken = parts("invalid_argument");
        broken.violations[0]
            .args
            .insert("Bad Key".to_owned(), ErrorArg::Int64(1));
        let error = item_error_to_connect(&broken);
        assert_eq!(error.code, ErrorCode::Internal);
        assert!(
            error
                .message
                .as_deref()
                .is_some_and(|m| m.contains("Bad Key"))
        );

        let mut broken = parts("not_found");
        if let Some(info) = &mut broken.info {
            info.args.insert("Bad Key".to_owned(), ErrorArg::Int64(1));
        }
        assert_eq!(item_error_to_connect(&broken).code, ErrorCode::Internal);
    }

    #[test]
    fn an_unknown_token_fails_closed_as_unknown_keeping_the_message() {
        let error = item_error_to_connect(&parts("teapot"));
        assert_eq!(error.code, ErrorCode::Unknown);
        assert_eq!(error.message.as_deref(), Some("no such widget"));
    }

    #[test]
    fn identity_changes_and_unauthenticated_are_never_retried() {
        assert!(is_never_retried(&identity_changed()));
        assert!(is_never_retried(&ConnectError::unauthenticated("no")));
        assert!(!is_never_retried(&ConnectError::unavailable("down")));
    }

    #[test]
    fn runtime_errors_carry_their_codes() {
        let cases = [
            (item_too_large(5), BATCH_ITEM_TOO_LARGE),
            (response_mismatch(), BATCH_RESPONSE_MISMATCH),
            (identity_changed(), BATCH_IDENTITY_CHANGED),
        ];
        for (error, code) in cases {
            assert_eq!(error_info(&error).expect("info").code, code);
        }
        assert_eq!(queue_full().code, ErrorCode::ResourceExhausted);
    }
}
