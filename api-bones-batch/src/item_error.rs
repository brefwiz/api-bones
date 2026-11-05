// SPDX-License-Identifier: MIT
//! `bones.v1.ItemError` as the `ConnectError` the single call raises, and the
//! errors the runtime raises itself.

use api_bones::connect::{ErrorInfo, with_error_info};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use connectrpc::{ConnectError, ErrorCode, ErrorDetail};

/// `ErrorInfo.code` of a whole-call refusal for too many items.
pub const BATCH_TOO_LARGE: &str = "BATCH_TOO_LARGE";
/// `ErrorInfo.code` of an id whose request alone exceeds the size limit.
pub const BATCH_ITEM_TOO_LARGE: &str = "BATCH_ITEM_TOO_LARGE";
/// `ErrorInfo.code` of a response that does not answer the request's ids in order.
pub const BATCH_RESPONSE_MISMATCH: &str = "BATCH_RESPONSE_MISMATCH";
/// `ErrorInfo.code` of a call whose identity changed before it was sent or answered.
pub const BATCH_IDENTITY_CHANGED: &str = "BATCH_IDENTITY_CHANGED";

const VALIDATION_FAILURE_TYPE: &str = "bones.v1.ValidationFailure";

/// The fields of a generated `bones.v1.ItemError`, read without naming its type.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemErrorParts {
    /// The Connect code token.
    pub code: String,
    /// The message the single call raises for the same cause.
    pub message: String,
    /// `ErrorInfo.code` and `ErrorInfo.emitter`, when the item carries one.
    pub info: Option<(String, String)>,
    /// `ValidationFailure` field, message and rule.
    pub violations: Vec<(String, String, String)>,
}

/// The error the single call raises for the same cause.
///
/// The code is the Connect token, the message is kept, `info` becomes the
/// `bones.v1.ErrorInfo` detail and each violation a `bones.v1.ValidationFailure`
/// detail. An unknown token maps to `unknown`, failing closed.
#[must_use]
pub fn item_error_to_connect(parts: &ItemErrorParts) -> ConnectError {
    let code = parts
        .code
        .parse::<ErrorCode>()
        .unwrap_or(ErrorCode::Unknown);
    let mut error = ConnectError::new(code, parts.message.clone());
    if let Some((code, emitter)) = &parts.info {
        error = with_error_info(
            error,
            &ErrorInfo {
                code: code.clone(),
                emitter: emitter.clone(),
            },
        );
    }
    for (field, message, rule) in &parts.violations {
        let mut bytes = Vec::new();
        put_string(&mut bytes, 1, field);
        put_string(&mut bytes, 2, message);
        put_string(&mut bytes, 3, rule);
        error = error.with_detail(ErrorDetail {
            type_url: VALIDATION_FAILURE_TYPE.to_owned(),
            value: Some(STANDARD_NO_PAD.encode(bytes)),
            debug: None,
        });
    }
    error
}

fn put_string(out: &mut Vec<u8>, field: u8, value: &str) {
    if value.is_empty() {
        return;
    }
    out.push((field << 3) | 2);
    let mut len = value.len();
    while len >= 0x80 {
        out.push(u8::try_from(len & 0x7f).unwrap_or(0) | 0x80);
        len >>= 7;
    }
    out.push(u8::try_from(len).unwrap_or(0));
    out.extend_from_slice(value.as_bytes());
}

fn coded(error: ConnectError, code: &str) -> ConnectError {
    with_error_info(error, &ErrorInfo::from_code(code))
}

pub(crate) fn item_too_large(id_bytes: usize) -> ConnectError {
    coded(
        ConnectError::invalid_argument(format!(
            "an id of {id_bytes} bytes does not fit one batch request"
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

    fn parts(code: &str) -> ItemErrorParts {
        ItemErrorParts {
            code: code.to_owned(),
            message: "no such widget".to_owned(),
            info: Some(("WIDGET_MISSING".to_owned(), "widgets".to_owned())),
            violations: vec![(
                "/id".to_owned(),
                "must be a uuid".to_owned(),
                "format".to_owned(),
            )],
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
    fn violations_become_details() {
        let error = item_error_to_connect(&parts("invalid_argument"));
        let failures = error
            .details
            .iter()
            .filter(|d| d.type_url == VALIDATION_FAILURE_TYPE)
            .count();
        assert_eq!(failures, 1);
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
