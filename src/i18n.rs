// SPDX-License-Identifier: MIT
//! Renders a refused call in the viewer's locale.
//!
//! Re-exports [`api_bones_i18n`] and makes every error type that reaches a
//! caller [`Localizable`]: a Connect error, a `problem+json` body
//! ([`ProblemJson`], or the [`ApiError`] it deserializes to) and one
//! validation failure. [`violations_of`] lists the validation failures of a
//! refused call, each of which is [`Localizable`] too.

use std::collections::BTreeMap;

pub use api_bones_i18n::*;
use api_bones_i18n::{Localizable as BaseLocalizable, localize as base_localize};

use crate::error::{
    ApiError, ErrorArg, ErrorCode as PlatformCode, ErrorInfoMember, ProblemJson, ValidationError,
};

/// Every `ErrorCode`, in declaration order, which is the order of
/// [`PLATFORM_CODES`].
const VARIANTS: [PlatformCode; 30] = [
    PlatformCode::BadRequest,
    PlatformCode::ValidationFailed,
    PlatformCode::Unauthorized,
    PlatformCode::InvalidCredentials,
    PlatformCode::TokenExpired,
    PlatformCode::TokenInvalid,
    PlatformCode::Forbidden,
    PlatformCode::InsufficientPermissions,
    PlatformCode::OrgOutsideSubtree,
    PlatformCode::AncestorRequired,
    PlatformCode::CrossSubtreeAccess,
    PlatformCode::ResourceNotFound,
    PlatformCode::MethodNotAllowed,
    PlatformCode::NotAcceptable,
    PlatformCode::RequestTimeout,
    PlatformCode::Conflict,
    PlatformCode::ResourceAlreadyExists,
    PlatformCode::Gone,
    PlatformCode::PreconditionFailed,
    PlatformCode::PayloadTooLarge,
    PlatformCode::UnsupportedMediaType,
    PlatformCode::UnprocessableEntity,
    PlatformCode::PreconditionRequired,
    PlatformCode::RateLimited,
    PlatformCode::RequestHeaderFieldsTooLarge,
    PlatformCode::InternalServerError,
    PlatformCode::NotImplemented,
    PlatformCode::BadGateway,
    PlatformCode::ServiceUnavailable,
    PlatformCode::GatewayTimeout,
];

fn platform_of(code: &PlatformCode) -> &'static str {
    VARIANTS
        .iter()
        .position(|variant| variant == code)
        .map_or("INTERNAL_SERVER_ERROR", |index| PLATFORM_CODES[index])
}

/// The platform code of a `problem+json` body: its `type` slug, else the
/// first code of its status, else `BAD_REQUEST` for a 4xx and
/// `INTERNAL_SERVER_ERROR` for anything else.
fn platform_of_problem(r#type: &str, status: u16) -> &'static str {
    let slug = r#type.rsplit([':', '/']).next().unwrap_or_default();
    VARIANTS
        .iter()
        .find(|code| code.urn_slug() == slug)
        .or_else(|| VARIANTS.iter().find(|code| code.status_code() == status))
        .map_or_else(
            || {
                if (400..500).contains(&status) {
                    "BAD_REQUEST"
                } else {
                    "INTERNAL_SERVER_ERROR"
                }
            },
            platform_of,
        )
}

fn arg_value(arg: &ErrorArg) -> ArgValue {
    match arg {
        ErrorArg::String(value) => ArgValue::String(value.clone()),
        ErrorArg::Int64(value) => ArgValue::Int64(*value),
        ErrorArg::Double(value) => ArgValue::Double(*value),
        ErrorArg::Timestamp { seconds, nanos } => ArgValue::Timestamp {
            seconds: *seconds,
            nanos: *nanos,
        },
    }
}

/// The arguments of an identity, bounds-checked as on every decode.
pub(crate) fn args_of(args: &BTreeMap<String, ErrorArg>) -> Result<Args, ArgError> {
    Args::from_entries(
        args.iter()
            .map(|(name, arg)| (name.clone(), arg_value(arg))),
    )
}

pub(crate) fn identity_of(platform: &str, info: Option<&ErrorInfoMember>) -> ErrorIdentity {
    match info {
        Some(info) => ErrorIdentity::error(
            platform,
            Some(&info.code),
            Some(&info.emitter),
            args_of(&info.args),
        ),
        None => ErrorIdentity::error(platform, None, None, Ok(Args::new())),
    }
}

fn violation_of(error: &ValidationError) -> Violation {
    Violation {
        field: error.field.clone(),
        message: error.message.clone(),
        rule: error.rule.clone(),
        args: args_of(&error.args),
    }
}

/// What a refusal's typed identity is read from.
///
/// This is [`api_bones_i18n::Localizable`] widened to the foreign Connect
/// error, which the orphan rule keeps `api-bones-i18n` from implementing:
/// [`localize`] here takes every type that implements this trait. A type of
/// this crate implements both traits, so [`api_bones_i18n::localize`] takes it
/// too.
pub trait Localizable {
    /// The identity, decoded and bounds-checked.
    fn identity(&self) -> ErrorIdentity;
}

impl Localizable for ErrorIdentity {
    fn identity(&self) -> ErrorIdentity {
        self.clone()
    }
}

impl Localizable for Violation {
    fn identity(&self) -> ErrorIdentity {
        BaseLocalizable::identity(self)
    }
}

/// Render `error` in the viewer's `locale`. See [`api_bones_i18n::localize`].
#[must_use]
pub fn localize(error: &impl Localizable, locale: &str, options: &LocalizeOptions) -> Localized {
    base_localize(&error.identity(), locale, options)
}

/// Implement both traits for a type of this crate from one identity.
macro_rules! localizable {
    ($ty:ty, $this:ident => $identity:expr) => {
        impl Localizable for $ty {
            fn identity(&$this) -> ErrorIdentity {
                $identity
            }
        }

        impl BaseLocalizable for $ty {
            fn identity(&$this) -> ErrorIdentity {
                $identity
            }
        }
    };
}
pub(crate) use localizable;

localizable!(ApiError, self => identity_of(platform_of(&self.code), self.error_info.as_ref()));

localizable!(ProblemJson, self => {
    let info = self
        .extensions
        .get("error_info")
        .and_then(|value| serde_json::from_value::<ErrorInfoMember>(value.clone()).ok());
    identity_of(
        platform_of_problem(&self.r#type, self.status),
        info.as_ref(),
    )
});

/// A `problem+json` `errors[]` member. One whose arguments do not decode
/// keeps its place and carries no arguments of its own.
fn violation_of_member(member: &serde_json::Value) -> Violation {
    if let Ok(error) = serde_json::from_value::<ValidationError>(member.clone()) {
        return violation_of(&error);
    }
    let text = |name: &str| {
        member
            .get(name)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    Violation {
        field: text("field").unwrap_or_default(),
        message: text("message").unwrap_or_default(),
        rule: text("rule"),
        args: Err(ArgError {
            rule: ArgRule::Malformed,
        }),
    }
}

/// An error that carries the validation failures of a refused call.
pub trait HasViolations {
    /// The validation failures, in the order the error carries them.
    fn violations(&self) -> Vec<Violation>;
}

impl HasViolations for ApiError {
    fn violations(&self) -> Vec<Violation> {
        self.errors.iter().map(violation_of).collect()
    }
}

impl HasViolations for ProblemJson {
    fn violations(&self) -> Vec<Violation> {
        self.extensions
            .get("errors")
            .and_then(serde_json::Value::as_array)
            .map_or_else(Vec::new, |members| {
                members.iter().map(violation_of_member).collect()
            })
    }
}

/// The validation failures of a refused call.
///
/// Each is renderable with [`localize`]: a Connect error's `bones.v1.ValidationFailure` details in
/// detail order, a `problem+json` body's `errors[]` in array order. One whose
/// arguments break the bounds is kept and carries no arguments of its own.
///
/// The Connect-only `connect::violations_of` (behind the `connect` feature)
/// returns the wire messages unchanged.
#[must_use]
pub fn violations_of(error: &impl HasViolations) -> Vec<Violation> {
    error.violations()
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    pub fn en(error: &impl Localizable) -> Localized {
        localize(error, "en", &LocalizeOptions::default())
    }

    pub fn platform_text(code: &str) -> String {
        en(&ErrorIdentity::error(code, None, None, Ok(Args::new()))).text
    }

    pub fn register_pay() {
        register_catalog(
            "i18n-pay",
            "en",
            r#"{"i18n-pay.errors.GRANT_MISSING.title": "Grant {grant} missing"}"#,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{en, platform_text, register_pay};
    use super::*;
    use crate::error::ErrorCode;
    use serde_json::json;

    fn problem(body: serde_json::Value) -> ProblemJson {
        serde_json::from_value(body).unwrap()
    }

    #[test]
    fn variants_follow_the_platform_code_order() {
        for (variant, code) in VARIANTS.iter().zip(PLATFORM_CODES) {
            assert_eq!(variant.urn_slug().replace('-', "_").to_uppercase(), code);
            assert_eq!(platform_of(variant), code);
        }
    }

    #[test]
    fn a_problem_type_slug_wins_over_its_status_in_every_type_mode() {
        for r#type in [
            "urn:api-bones:error:resource-not-found",
            "https://errors.example.com/resource-not-found",
            "resource-not-found",
        ] {
            let body = problem(json!({"type": r#type, "title": "t", "status": 500, "detail": "d"}));
            assert_eq!(
                en(&body).text,
                platform_text("RESOURCE_NOT_FOUND"),
                "{type}"
            );
        }
    }

    #[test]
    fn a_problem_without_a_known_type_maps_its_status() {
        for (status, expected) in [
            (400, "BAD_REQUEST"),
            (404, "RESOURCE_NOT_FOUND"),
            (409, "CONFLICT"),
            (418, "BAD_REQUEST"),
            (503, "SERVICE_UNAVAILABLE"),
            (599, "INTERNAL_SERVER_ERROR"),
            (200, "INTERNAL_SERVER_ERROR"),
        ] {
            let body = problem(
                json!({"type": "about:blank", "title": "t", "status": status, "detail": "d"}),
            );
            assert_eq!(en(&body).text, platform_text(expected), "{status}");
        }
    }

    #[test]
    fn a_problem_never_renders_its_title_or_detail() {
        let body = problem(
            json!({"type": "about:blank", "title": "SECRET title", "status": 403, "detail": "SECRET detail"}),
        );
        let text = en(&body);
        assert_eq!(text.text, platform_text("FORBIDDEN"));
        assert!(!text.text.contains("SECRET"));
    }

    #[test]
    fn a_problem_error_info_member_renders_from_the_emitter_catalog() {
        register_pay();
        let body = problem(json!({
            "type": "about:blank", "title": "t", "status": 403, "detail": "d",
            "error_info": {
                "code": "GRANT_MISSING",
                "emitter": "i18n-pay",
                "args": {"grant": {"stringValue": "read"}}
            }
        }));
        assert_eq!(en(&body).text, "Grant read missing");
    }

    #[test]
    fn a_problem_with_malformed_error_info_args_renders_without_it() {
        register_pay();
        for args in [
            json!({"grant": {"int64Value": 7}}),
            json!({"grant": {"stringValue": "a", "int64Value": "1"}}),
            json!({"grant": {"doubleValue": "NaN"}}),
            json!("nope"),
        ] {
            let body = problem(json!({
                "type": "about:blank", "title": "t", "status": 403, "detail": "d",
                "error_info": {"code": "GRANT_MISSING", "emitter": "i18n-pay", "args": args}
            }));
            assert_eq!(en(&body).text, platform_text("FORBIDDEN"), "{args}");
        }
        let not_an_object = problem(json!({
            "type": "about:blank", "title": "t", "status": 403, "detail": "d",
            "error_info": 7
        }));
        assert_eq!(en(&not_an_object).text, platform_text("FORBIDDEN"));
    }

    #[test]
    fn a_problem_error_info_with_a_bones_code_uses_the_platform_catalog() {
        let body = problem(json!({
            "type": "about:blank", "title": "t", "status": 400, "detail": "d",
            "error_info": {"code": "GONE", "emitter": "i18n-unregistered"}
        }));
        assert_eq!(en(&body).text, platform_text("GONE"));
    }

    #[test]
    fn a_problem_lists_its_validation_errors_in_array_order() {
        let body = problem(json!({
            "type": "about:blank", "title": "t", "status": 400, "detail": "d",
            "errors": [
                {"field": "/a", "message": "SECRET", "rule": "max", "args": {"max": {"int64Value": "5"}}},
                {"field": "/b", "message": "m"},
                {"field": "/c", "message": "m", "rule": "max", "args": {"max": {"int64Value": 5}}},
                7
            ]
        }));
        let violations = violations_of(&body);
        assert_eq!(violations.len(), 4);
        assert_eq!(violations[0].field, "/a");
        assert_eq!(en(&violations[0]).text, "Must be at most 5.");
        assert_eq!(violations[1].rule, None);
        assert_eq!(en(&violations[1]).text, platform_text("VALIDATION_FAILED"));
        assert_eq!(violations[2].field, "/c");
        assert_eq!(
            violations[2].args,
            Err(ArgError {
                rule: ArgRule::Malformed
            })
        );
        assert_eq!(en(&violations[2]).text, platform_text("VALIDATION_FAILED"));
        assert_eq!(violations[3].field, "");
        let not_a_list = problem(
            json!({"type": "x", "title": "t", "status": 400, "detail": "d", "errors": "no"}),
        );
        assert_eq!(violations_of(&not_a_list).len(), 0);
    }

    #[test]
    fn an_api_error_renders_its_code_and_error_info() {
        register_pay();
        let plain = ApiError::new(ErrorCode::ResourceNotFound, "SECRET");
        assert_eq!(en(&plain).text, platform_text("RESOURCE_NOT_FOUND"));
        let mut info = ErrorInfoMember::new("GRANT_MISSING", "i18n-pay");
        info.args
            .insert("grant".into(), ErrorArg::String("write".into()));
        let with_info = ApiError::new(ErrorCode::Forbidden, "SECRET").with_error_info(info);
        assert_eq!(en(&with_info).text, "Grant write missing");
    }

    #[test]
    fn an_api_error_with_unbounded_arguments_renders_without_error_info() {
        register_pay();
        let mut info = ErrorInfoMember::new("GRANT_MISSING", "i18n-pay");
        info.args.insert("grant".into(), ErrorArg::Double(f64::NAN));
        let error = ApiError::new(ErrorCode::Forbidden, "d").with_error_info(info);
        assert_eq!(en(&error).text, platform_text("FORBIDDEN"));
    }

    #[test]
    fn an_api_error_lists_its_validation_errors() {
        let mut failure = ValidationError::new("/age", "SECRET");
        failure.rule = Some("max".into());
        failure.args.insert("max".into(), ErrorArg::Int64(9));
        let mut error = ApiError::new(ErrorCode::ValidationFailed, "d");
        error.errors.push(failure);
        let violations = violations_of(&error);
        assert_eq!(violations.len(), 1);
        assert_eq!(en(&violations[0]).text, "Must be at most 9.");
    }

    #[test]
    fn every_argument_kind_crosses_unchanged() {
        let mut map = BTreeMap::new();
        map.insert("a".to_owned(), ErrorArg::String("s".into()));
        map.insert("b".to_owned(), ErrorArg::Int64(-3));
        map.insert("c".to_owned(), ErrorArg::Double(1.5));
        map.insert(
            "d".to_owned(),
            ErrorArg::Timestamp {
                seconds: 7,
                nanos: 8,
            },
        );
        let args = args_of(&map).unwrap();
        assert_eq!(args.get("a"), Some(&ArgValue::String("s".into())));
        assert_eq!(args.get("b"), Some(&ArgValue::Int64(-3)));
        assert_eq!(args.get("c"), Some(&ArgValue::Double(1.5)));
        assert_eq!(
            args.get("d"),
            Some(&ArgValue::Timestamp {
                seconds: 7,
                nanos: 8
            })
        );
        map.insert("e".to_owned(), ErrorArg::Double(f64::INFINITY));
        assert_eq!(args_of(&map).unwrap_err().rule, ArgRule::NonFiniteDouble);
    }
}
