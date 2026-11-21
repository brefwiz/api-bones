// SPDX-License-Identifier: MIT
//! `Localizable` and `HasViolations` for the Connect error types.

use connectrpc::ConnectError;

use crate::connect::code::ConnectCode;
use crate::connect::error_info::{ValidationFailureDetail, error_info, violations_unchecked};
use crate::i18n::{
    ArgError, ArgRule, ErrorIdentity, HasViolations, Localizable, Violation, args_of, identity_of,
    localizable,
};
use api_bones_i18n::Localizable as BaseLocalizable;

/// The platform code of a Connect code (core-ui `connectCodeToErrorCode`).
fn platform_of_connect(code: ConnectCode) -> &'static str {
    match code {
        ConnectCode::Unauthenticated => "UNAUTHORIZED",
        ConnectCode::PermissionDenied => "FORBIDDEN",
        ConnectCode::NotFound => "RESOURCE_NOT_FOUND",
        ConnectCode::AlreadyExists => "RESOURCE_ALREADY_EXISTS",
        ConnectCode::ResourceExhausted => "RATE_LIMITED",
        ConnectCode::FailedPrecondition => "PRECONDITION_FAILED",
        ConnectCode::InvalidArgument => "VALIDATION_FAILED",
        ConnectCode::Unavailable => "SERVICE_UNAVAILABLE",
        ConnectCode::DeadlineExceeded => "GATEWAY_TIMEOUT",
        ConnectCode::Unimplemented => "NOT_IMPLEMENTED",
        ConnectCode::Aborted => "CONFLICT",
        ConnectCode::OutOfRange => "UNPROCESSABLE_ENTITY",
        _ => "INTERNAL_SERVER_ERROR",
    }
}

impl Localizable for ConnectError {
    fn identity(&self) -> ErrorIdentity {
        let member = error_info(self).map(|info| (&info).into());
        identity_of(
            platform_of_connect(ConnectCode::from(self.code)),
            member.as_ref(),
        )
    }
}

impl HasViolations for ConnectError {
    fn violations(&self) -> Vec<Violation> {
        violations_unchecked(self)
            .into_iter()
            .map(|(detail, args_decoded)| {
                let args = if args_decoded {
                    args_of(&detail.args)
                } else {
                    Err(ArgError {
                        rule: ArgRule::Malformed,
                    })
                };
                Violation {
                    field: detail.field,
                    message: detail.message,
                    rule: Some(detail.rule).filter(|rule| !rule.is_empty()),
                    args,
                }
            })
            .collect()
    }
}

localizable!(ValidationFailureDetail, self => {
    ErrorIdentity::violation(Some(&self.rule), args_of(&self.args))
});

#[cfg(test)]
mod tests {
    use crate::connect::error_info::{ErrorInfo, ValidationFailureDetail, with_error_info};
    use crate::error::ErrorArg;
    use crate::i18n::test_support::{en, platform_text, register_pay};
    use crate::i18n::{ArgRule, violations_of};
    use connectrpc::{ConnectError, ErrorCode as Code, ErrorDetail};

    #[test]
    fn a_connect_code_maps_to_its_platform_code() {
        for (code, expected) in [
            (Code::Unauthenticated, "UNAUTHORIZED"),
            (Code::PermissionDenied, "FORBIDDEN"),
            (Code::NotFound, "RESOURCE_NOT_FOUND"),
            (Code::AlreadyExists, "RESOURCE_ALREADY_EXISTS"),
            (Code::ResourceExhausted, "RATE_LIMITED"),
            (Code::FailedPrecondition, "PRECONDITION_FAILED"),
            (Code::InvalidArgument, "VALIDATION_FAILED"),
            (Code::Unavailable, "SERVICE_UNAVAILABLE"),
            (Code::DeadlineExceeded, "GATEWAY_TIMEOUT"),
            (Code::Unimplemented, "NOT_IMPLEMENTED"),
            (Code::Aborted, "CONFLICT"),
            (Code::OutOfRange, "UNPROCESSABLE_ENTITY"),
            (Code::Internal, "INTERNAL_SERVER_ERROR"),
            (Code::Unknown, "INTERNAL_SERVER_ERROR"),
            (Code::DataLoss, "INTERNAL_SERVER_ERROR"),
            (Code::Canceled, "INTERNAL_SERVER_ERROR"),
        ] {
            let error = ConnectError::new(code, "SECRET operator message");
            let text = en(&error);
            assert_eq!(text.text, platform_text(expected), "{code:?}");
            assert!(!text.text.contains("SECRET"));
        }
    }

    #[test]
    fn a_connect_error_renders_from_its_error_info() {
        register_pay();
        let info = ErrorInfo::from_code("GRANT_MISSING")
            .with_arg("grant", "read".to_owned())
            .unwrap();
        let mut info = info;
        info.emitter = "i18n-pay".into();
        let error = with_error_info(ConnectError::new(Code::PermissionDenied, "m"), &info);
        assert_eq!(en(&error).text, "Grant read missing");
    }

    #[test]
    fn a_connect_error_whose_arguments_break_the_bounds_renders_without_error_info() {
        register_pay();
        let mut info = ErrorInfo::from_code("GRANT_MISSING");
        info.emitter = "i18n-pay".into();
        for n in 0..=32 {
            info.args
                .insert(format!("k{n:02}"), ErrorArg::Int64(i64::from(n)));
        }
        let error = ConnectError::new(Code::PermissionDenied, "m").with_detail(info.to_detail());
        assert_eq!(en(&error).text, platform_text("FORBIDDEN"));
    }

    fn failure(rule: &str, args: &[(&str, ErrorArg)]) -> ValidationFailureDetail {
        ValidationFailureDetail {
            field: format!("/{rule}"),
            message: "SECRET".into(),
            rule: rule.into(),
            args: args
                .iter()
                .map(|(name, arg)| ((*name).to_owned(), arg.clone()))
                .collect(),
        }
    }

    fn detail(failure: &ValidationFailureDetail) -> ErrorDetail {
        ErrorDetail {
            type_url: crate::connect::error_info::VALIDATION_FAILURE_TYPE.to_owned(),
            value: Some(base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD_NO_PAD,
                failure.encode(),
            )),
            debug: None,
        }
    }

    #[test]
    fn connect_violations_render_one_by_one_in_detail_order() {
        let max = failure("max", &[("max", ErrorArg::Int64(5))]);
        let bare = failure("", &[]);
        let mut too_many = failure("max", &[]);
        for n in 0..=32 {
            too_many
                .args
                .insert(format!("k{n:02}"), ErrorArg::Int64(i64::from(n)));
        }
        let error = ConnectError::new(Code::InvalidArgument, "m")
            .with_detail(detail(&max))
            .with_detail(detail(&bare))
            .with_detail(detail(&too_many));
        let violations = violations_of(&error);
        assert_eq!(violations.len(), 3);
        assert_eq!(violations[0].field, "/max");
        assert_eq!(en(&violations[0]).text, "Must be at most 5.");
        assert_eq!(violations[1].rule, None);
        assert_eq!(en(&violations[1]).text, platform_text("VALIDATION_FAILED"));
        assert_eq!(
            violations[2].args.as_ref().unwrap_err().rule,
            ArgRule::TooManyArgs
        );
        assert_eq!(en(&violations[2]).text, platform_text("VALIDATION_FAILED"));
        assert_eq!(
            violations_of(&ConnectError::new(Code::NotFound, "m")).len(),
            0
        );
    }

    #[test]
    fn a_bare_validation_failure_renders() {
        let max = failure("max", &[("max", ErrorArg::Int64(1500))]);
        assert_eq!(en(&max).text, "Must be at most 1,500.");
        let unknown = failure("nope", &[]);
        assert_eq!(en(&unknown).text, platform_text("VALIDATION_FAILED"));
    }

    #[test]
    fn a_validation_detail_with_a_malformed_argument_keeps_its_place() {
        let mut bytes = failure("max", &[]).encode();
        // args entry: field 4, length 4, key "k" and a value that is not a message.
        bytes.extend_from_slice(&[0x22, 0x04, 0x0a, 0x01, b'k', 0x12]);
        let malformed = ErrorDetail {
            type_url: crate::connect::error_info::VALIDATION_FAILURE_TYPE.to_owned(),
            value: Some(base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD_NO_PAD,
                bytes,
            )),
            debug: None,
        };
        let error = ConnectError::new(Code::InvalidArgument, "m").with_detail(malformed);
        let violations = violations_of(&error);
        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].args.as_ref().unwrap_err().rule,
            ArgRule::Malformed
        );
    }
}
