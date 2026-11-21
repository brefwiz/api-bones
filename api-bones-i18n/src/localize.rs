// SPDX-License-Identifier: MIT
//! Rendering a refused call's typed identity in the viewer's locale.
//!
//! Keys are tried in order, each in the negotiated locale and then in `en`:
//! `<emitter>.errors.<code>`, `bones.errors.<code>`, `bones.errors.<PLATFORM>`.
//! A violation tries `bones.validation.<rule>`, then
//! `bones.errors.VALIDATION_FAILED`. The last key of each chain always
//! resolves, so neither the raw code nor the operator's `message` is ever
//! rendered.

use core::fmt;

use jiff::tz::TimeZone;

use crate::args::{ArgError, Args};
use crate::catalog::{BONES, entry};
use crate::format::Message;
use crate::locale::{DEFAULT_LOCALE, negotiate_locale};

/// The platform error codes: the `SCREAMING_SNAKE` form of the `ErrorCode`
/// variants, in declaration order. Each has an English `title` without
/// placeholders.
pub const PLATFORM_CODES: [&str; 30] = [
    "BAD_REQUEST",
    "VALIDATION_FAILED",
    "UNAUTHORIZED",
    "INVALID_CREDENTIALS",
    "TOKEN_EXPIRED",
    "TOKEN_INVALID",
    "FORBIDDEN",
    "INSUFFICIENT_PERMISSIONS",
    "ORG_OUTSIDE_SUBTREE",
    "ANCESTOR_REQUIRED",
    "CROSS_SUBTREE_ACCESS",
    "RESOURCE_NOT_FOUND",
    "METHOD_NOT_ALLOWED",
    "NOT_ACCEPTABLE",
    "REQUEST_TIMEOUT",
    "CONFLICT",
    "RESOURCE_ALREADY_EXISTS",
    "GONE",
    "PRECONDITION_FAILED",
    "PAYLOAD_TOO_LARGE",
    "UNSUPPORTED_MEDIA_TYPE",
    "UNPROCESSABLE_ENTITY",
    "PRECONDITION_REQUIRED",
    "RATE_LIMITED",
    "REQUEST_HEADER_FIELDS_TOO_LARGE",
    "INTERNAL_SERVER_ERROR",
    "NOT_IMPLEMENTED",
    "BAD_GATEWAY",
    "SERVICE_UNAVAILABLE",
    "GATEWAY_TIMEOUT",
];

const FALLBACK_PLATFORM: &str = "INTERNAL_SERVER_ERROR";
const VALIDATION_FAILED: &str = "VALIDATION_FAILED";

/// The platform code spelled `code`, or `INTERNAL_SERVER_ERROR`.
fn platform_code(code: &str) -> &'static str {
    PLATFORM_CODES
        .iter()
        .find(|known| **known == code)
        .copied()
        .unwrap_or(FALLBACK_PLATFORM)
}

fn nonempty(value: Option<&str>) -> Option<String> {
    value.filter(|v| !v.is_empty()).map(str::to_owned)
}

#[derive(Debug, Clone)]
enum Target {
    Error {
        platform: &'static str,
        code: Option<String>,
        emitter: Option<String>,
    },
    Violation {
        rule: Option<String>,
    },
}

/// What a catalog string is chosen and formatted from: the typed identity of
/// an error or of one validation failure.
#[derive(Debug, Clone)]
pub struct ErrorIdentity {
    target: Target,
    args: Result<Args, ArgError>,
}

impl ErrorIdentity {
    /// The identity of an error: its platform code (any other value becomes
    /// `INTERNAL_SERVER_ERROR`), the emitter's code and emitter when the
    /// error carries them, and its arguments. Empty strings are absent.
    ///
    /// Arguments that failed their bounds on decode make the error render as
    /// if it carried no emitter code.
    #[must_use]
    pub fn error(
        platform: &str,
        code: Option<&str>,
        emitter: Option<&str>,
        args: Result<Args, ArgError>,
    ) -> Self {
        Self {
            target: Target::Error {
                platform: platform_code(platform),
                code: nonempty(code),
                emitter: nonempty(emitter),
            },
            args,
        }
    }

    /// The identity of one validation failure: its rule (empty is absent)
    /// and arguments.
    ///
    /// Arguments that failed their bounds on decode make the failure render
    /// as if it carried no rule string of its own.
    #[must_use]
    pub fn violation(rule: Option<&str>, args: Result<Args, ArgError>) -> Self {
        Self {
            target: Target::Violation {
                rule: nonempty(rule),
            },
            args,
        }
    }

    /// The catalog keys to try in order, each with the emitter whose catalog
    /// holds it, and the arguments to format with.
    fn plan(&self) -> (Vec<(String, String)>, Args) {
        match &self.args {
            Ok(args) => (self.keys(true), args.clone()),
            Err(_) => (self.keys(false), Args::new()),
        }
    }

    fn keys(&self, args_ok: bool) -> Vec<(String, String)> {
        let bones = |key: String| (BONES.to_owned(), key);
        match &self.target {
            Target::Error {
                platform,
                code,
                emitter,
            } => {
                let mut keys = Vec::new();
                if let (true, Some(code)) = (args_ok, code) {
                    if let Some(emitter) = emitter {
                        keys.push((emitter.clone(), format!("{emitter}.errors.{code}")));
                    }
                    keys.push(bones(format!("bones.errors.{code}")));
                }
                keys.push(bones(format!("bones.errors.{platform}")));
                keys
            }
            Target::Violation { rule } => {
                let mut keys = Vec::new();
                if let (true, Some(rule)) = (args_ok, rule) {
                    keys.push(bones(format!("bones.validation.{rule}")));
                }
                keys.push(bones(format!("bones.errors.{VALIDATION_FAILED}")));
                keys
            }
        }
    }
}

/// Something that names the typed identity of a refusal.
pub trait Localizable {
    /// The identity, decoded and bounds-checked.
    fn identity(&self) -> ErrorIdentity;
}

impl Localizable for ErrorIdentity {
    fn identity(&self) -> ErrorIdentity {
        self.clone()
    }
}

/// One validation failure of a refused call.
#[derive(Debug, Clone, PartialEq)]
pub struct Violation {
    /// JSON Pointer to the failing field.
    pub field: String,
    /// The operator's English message. Never rendered by [`localize`].
    pub message: String,
    /// The machine-readable rule, when it names one.
    pub rule: Option<String>,
    /// The typed arguments of the rule's message, or the bound they broke.
    pub args: Result<Args, ArgError>,
}

impl Localizable for Violation {
    fn identity(&self) -> ErrorIdentity {
        ErrorIdentity::violation(self.rule.as_deref(), self.args.clone())
    }
}

/// The options of one [`localize`] call. The default renders times in UTC.
#[derive(Debug, Clone)]
pub struct LocalizeOptions {
    zone: TimeZone,
}

impl Default for LocalizeOptions {
    fn default() -> Self {
        Self {
            zone: TimeZone::UTC,
        }
    }
}

/// An option the call refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalizeError {
    /// The shared rule name: `unknown_time_zone`.
    pub rule: &'static str,
}

impl fmt::Display for LocalizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "option refused: {}", self.rule)
    }
}

impl core::error::Error for LocalizeError {}

impl LocalizeOptions {
    /// Render dates and times in the IANA zone `iana`.
    ///
    /// # Errors
    ///
    /// [`LocalizeError`] with rule `unknown_time_zone` for an empty or
    /// unknown zone, before any lookup.
    pub fn time_zone(mut self, iana: &str) -> Result<Self, LocalizeError> {
        let refused = LocalizeError {
            rule: "unknown_time_zone",
        };
        if iana.is_empty() {
            return Err(refused);
        }
        self.zone = TimeZone::get(iana).map_err(|_| refused)?;
        Ok(self)
    }
}

/// An error rendered for a viewer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Localized {
    /// The rendered `title` of the first key that resolves.
    pub text: String,
    /// The rendered `description` of the same key and locale, when it has one.
    pub hint: Option<String>,
}

fn render(
    emitter: &str,
    locale: &str,
    base: &str,
    args: &Args,
    options: &LocalizeOptions,
) -> Option<Localized> {
    let show = |suffix: &str| {
        let pattern = entry(emitter, locale, &format!("{base}.{suffix}"))?;
        Message::parse(&pattern)
            .ok()?
            .render(args, locale, &options.zone)
            .ok()
    };
    Some(Localized {
        text: show("title")?,
        hint: show("description"),
    })
}

/// Render `error` in the viewer's `locale` (a BCP 47 tag or an
/// Accept-Language value, negotiated over the platform locales).
///
/// Pure and synchronous. It never panics and never renders the raw code or
/// the operator's message: the last key of the chain always resolves.
#[must_use]
pub fn localize(error: &impl Localizable, locale: &str, options: &LocalizeOptions) -> Localized {
    let viewer = negotiate_locale(locale);
    let (keys, args) = error.identity().plan();
    let locales: &[&str] = if viewer == DEFAULT_LOCALE {
        &[DEFAULT_LOCALE]
    } else {
        &[viewer, DEFAULT_LOCALE]
    };
    keys.iter()
        .flat_map(|(emitter, base)| locales.iter().map(move |loc| (emitter, base, *loc)))
        .find_map(|(emitter, base, loc)| render(emitter, loc, base, &args, options))
        .unwrap_or(Localized {
            text: String::new(),
            hint: None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::{ArgRule, ArgValue};
    use crate::catalog::register_catalog;

    fn opts() -> LocalizeOptions {
        LocalizeOptions::default()
    }

    fn platform(code: &str) -> ErrorIdentity {
        ErrorIdentity::error(code, None, None, Ok(Args::new()))
    }

    fn args(entries: &[(&str, ArgValue)]) -> Args {
        Args::from_entries(entries.iter().map(|(k, v)| ((*k).to_owned(), v.clone()))).unwrap()
    }

    fn en_title(code: &str) -> String {
        entry(BONES, "en", &format!("bones.errors.{code}.title")).unwrap()
    }

    #[test]
    fn every_platform_code_has_a_placeholder_free_english_title() {
        for code in PLATFORM_CODES {
            let title = en_title(code);
            assert!(!title.contains('{'), "{code}");
            assert_eq!(localize(&platform(code), "en", &opts()).text, title);
        }
    }

    #[test]
    fn an_unknown_platform_code_renders_the_internal_error_text() {
        assert_eq!(
            localize(&platform("NOT_A_CODE"), "en", &opts()).text,
            en_title("INTERNAL_SERVER_ERROR")
        );
    }

    #[test]
    fn the_viewer_locale_is_negotiated_and_english_is_the_fallback() {
        let id = platform("BAD_REQUEST");
        assert_eq!(localize(&id, "fr-CA", &opts()).text, "Requête invalide");
        assert_eq!(
            localize(&id, "fr;q=0.5, en;q=0.9", &opts()).text,
            "Invalid request"
        );
        assert_eq!(localize(&id, "xx", &opts()).text, "Invalid request");
    }

    #[test]
    fn a_hint_comes_from_the_same_key_and_locale() {
        let hinted = localize(&platform("VALIDATION_FAILED"), "fr", &opts());
        assert_eq!(
            hinted.hint.as_deref(),
            Some("Veuillez vérifier les champs indiqués et réessayer.")
        );
        assert_eq!(localize(&platform("BAD_REQUEST"), "en", &opts()).hint, None);
    }

    #[test]
    fn an_emitter_key_wins_over_the_platform_keys_and_falls_back_to_english() {
        register_catalog(
            "lz-pay",
            "en",
            r#"{"lz-pay.errors.GRANT_MISSING.title": "Grant {grant} missing", "lz-pay.errors.GRANT_MISSING.description": "Ask the owner of {grant}"}"#,
        );
        register_catalog("lz-pay", "fr", r#"{"lz-pay.errors.OTHER.title": "Autre"}"#);
        let id = ErrorIdentity::error(
            "FORBIDDEN",
            Some("GRANT_MISSING"),
            Some("lz-pay"),
            Ok(args(&[("grant", ArgValue::String("read".into()))])),
        );
        let en = localize(&id, "en", &opts());
        assert_eq!(en.text, "Grant read missing");
        assert_eq!(en.hint.as_deref(), Some("Ask the owner of read"));
        let fr = localize(&id, "fr", &opts());
        assert_eq!(
            fr.text, "Grant read missing",
            "key-major: en emitter key beats fr bones"
        );
    }

    #[test]
    fn an_entry_that_cannot_render_falls_through_to_the_next_key() {
        register_catalog(
            "lz-miss",
            "en",
            r#"{"lz-miss.errors.X.title": "Need {n, number}"}"#,
        );
        let id = ErrorIdentity::error("FORBIDDEN", Some("X"), Some("lz-miss"), Ok(Args::new()));
        assert_eq!(localize(&id, "en", &opts()).text, en_title("FORBIDDEN"));
        let wrong = ErrorIdentity::error(
            "FORBIDDEN",
            Some("X"),
            Some("lz-miss"),
            Ok(args(&[("n", ArgValue::String("3".into()))])),
        );
        assert_eq!(localize(&wrong, "en", &opts()).text, en_title("FORBIDDEN"));
        let fits = ErrorIdentity::error(
            "FORBIDDEN",
            Some("X"),
            Some("lz-miss"),
            Ok(args(&[("n", ArgValue::Int64(1234))])),
        );
        assert_eq!(localize(&fits, "en", &opts()).text, "Need 1,234");
    }

    #[test]
    fn an_unstamped_error_skips_the_emitter_key_and_uses_a_bones_code() {
        let id = ErrorIdentity::error("BAD_REQUEST", Some("CONFLICT"), None, Ok(Args::new()));
        assert_eq!(localize(&id, "en", &opts()).text, en_title("CONFLICT"));
        let empty = ErrorIdentity::error("BAD_REQUEST", Some(""), Some(""), Ok(Args::new()));
        assert_eq!(
            localize(&empty, "en", &opts()).text,
            en_title("BAD_REQUEST")
        );
    }

    #[test]
    fn arguments_out_of_bounds_render_as_if_no_emitter_info() {
        register_catalog("lz-bad", "en", r#"{"lz-bad.errors.X.title": "emitter"}"#);
        let id = ErrorIdentity::error(
            "FORBIDDEN",
            Some("X"),
            Some("lz-bad"),
            Err(ArgError {
                rule: ArgRule::StringTooLong,
            }),
        );
        assert_eq!(localize(&id, "en", &opts()).text, en_title("FORBIDDEN"));
    }

    #[test]
    fn a_violation_renders_its_rule_then_the_platform_validation_text() {
        let max = Violation {
            field: "/age".into(),
            message: "SECRET operator text".into(),
            rule: Some("max".into()),
            args: Ok(args(&[("max", ArgValue::Int64(5))])),
        };
        assert_eq!(localize(&max, "en", &opts()).text, "Must be at most 5.");
        assert_eq!(localize(&max, "fr", &opts()).text, "Doit être au plus 5.");
        let missing_arg = Violation {
            args: Ok(Args::new()),
            ..max.clone()
        };
        let text = localize(&missing_arg, "en", &opts());
        assert_eq!(text.text, en_title("VALIDATION_FAILED"));
        assert!(text.hint.is_some());
        assert!(!text.text.contains("SECRET"));
        let no_rule = Violation {
            rule: None,
            ..max.clone()
        };
        assert_eq!(
            localize(&no_rule, "en", &opts()).text,
            en_title("VALIDATION_FAILED")
        );
        let unknown = Violation {
            rule: Some("no_such_rule".into()),
            ..max.clone()
        };
        assert_eq!(
            localize(&unknown, "en", &opts()).text,
            en_title("VALIDATION_FAILED")
        );
        let bad_args = Violation {
            args: Err(ArgError {
                rule: ArgRule::Malformed,
            }),
            ..max
        };
        assert_eq!(
            localize(&bad_args, "en", &opts()).text,
            en_title("VALIDATION_FAILED")
        );
    }

    #[test]
    fn rules_match_byte_exactly() {
        let upper = Violation {
            field: String::new(),
            message: String::new(),
            rule: Some("MAX".into()),
            args: Ok(args(&[("max", ArgValue::Int64(5))])),
        };
        assert_eq!(
            localize(&upper, "en", &opts()).text,
            en_title("VALIDATION_FAILED")
        );
    }

    #[test]
    fn a_timestamp_renders_in_the_requested_zone_and_utc_by_default() {
        register_catalog(
            "lz-time",
            "en",
            r#"{"lz-time.errors.AT.title": "{at, time, medium}"}"#,
        );
        let id = ErrorIdentity::error(
            "CONFLICT",
            Some("AT"),
            Some("lz-time"),
            Ok(args(&[(
                "at",
                ArgValue::Timestamp {
                    seconds: 0,
                    nanos: 0,
                },
            )])),
        );
        assert!(localize(&id, "en", &opts()).text.starts_with("12:00:00"));
        let tokyo = opts().time_zone("Asia/Tokyo").unwrap();
        assert!(localize(&id, "en", &tokyo).text.starts_with("9:00:00"));
    }

    #[test]
    fn an_unknown_or_empty_time_zone_is_refused() {
        for zone in ["", "Mars/Olympus", "not a zone"] {
            let refused = opts().time_zone(zone).unwrap_err();
            assert_eq!(refused.rule, "unknown_time_zone", "{zone:?}");
            assert_eq!(refused.to_string(), "option refused: unknown_time_zone");
        }
    }

    #[test]
    fn an_identity_is_its_own_localizable() {
        let id = platform("GONE");
        assert_eq!(localize(&id, "en", &opts()).text, en_title("GONE"));
    }
}
