// SPDX-License-Identifier: MIT
//! An error's message in the viewer's language.

use api_bones_i18n::{ArgValue, Args, ErrorIdentity, LocalizeOptions, localize};
use api_bones_sdk::__private::{
    decode_call_error, encode_build_info, encode_format_refused, encode_format_text,
};
use api_bones_sdk::{Arg, Error};

/// The `bones.sdk.v1.BuildInfo` bytes of this core, stamped with the boundary
/// version it speaks. A host reads it to refuse a core built for another one.
#[uniffi::export]
#[must_use]
pub fn api_bones_sdk_build_info() -> Vec<u8> {
    encode_build_info(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
}

/// The message of a failed call for a person, as `bones.sdk.v1.FormatResult`
/// bytes: `error` is the `CallError` bytes the call failed with, `locale` a
/// BCP 47 tag or an `Accept-Language` value, `time_zone` an IANA zone for the
/// instants the message names (UTC when absent). The text comes from the
/// error's catalog entry, never from its diagnostic message. A zone or error
/// that cannot be read is answered as a refused rule.
#[uniffi::export]
#[must_use]
pub fn format_error(error: Vec<u8>, locale: &str, time_zone: Option<String>) -> Vec<u8> {
    let decoded = decode_call_error(&error);
    // The exported signature owns the bytes the host handed over.
    drop(error);
    let Ok(error) = decoded else {
        return encode_format_refused("malformed_error");
    };
    let options = match time_zone {
        None => LocalizeOptions::default(),
        Some(zone) => match LocalizeOptions::default().time_zone(&zone) {
            Ok(options) => options,
            Err(refused) => return encode_format_refused(refused.rule),
        },
    };
    let localized = localize(&identity(&error), locale, &options);
    encode_format_text(&localized.text, localized.hint.as_deref())
}

/// The typed identity a message is rendered from: arguments out of their
/// bounds render the message without them.
fn identity(error: &Error) -> ErrorIdentity {
    let args = Args::from_entries(error.args.iter().map(|(name, arg)| {
        let value = match arg {
            Arg::String(text) => ArgValue::String(text.clone()),
            Arg::Int64(n) => ArgValue::Int64(*n),
            Arg::Double(x) => ArgValue::Double(*x),
            Arg::Timestamp { seconds, nanos } => ArgValue::Timestamp {
                seconds: *seconds,
                nanos: *nanos,
            },
        };
        (name.clone(), value)
    }));
    ErrorIdentity::error(
        &error.platform,
        Some(&error.code),
        Some(&error.emitter),
        args,
    )
}

#[cfg(test)]
mod tests {
    use api_bones_i18n::{PLATFORM_LOCALES, register_catalog};
    use api_bones_sdk::__private::encode_call_error;

    use super::*;

    /// What the catalog renders for `error` in `locale` and `zone`, encoded as
    /// the result.
    fn rendered_in(error: &Error, locale: &str, zone: Option<&str>) -> Vec<u8> {
        let options = match zone {
            None => LocalizeOptions::default(),
            Some(zone) => LocalizeOptions::default().time_zone(zone).expect("a zone"),
        };
        let shown = localize(&identity(error), locale, &options);
        assert!(
            !shown.text.is_empty(),
            "the platform catalog renders every error"
        );
        encode_format_text(&shown.text, shown.hint.as_deref())
    }

    /// What the catalog renders for `error` in `locale`, encoded as the result.
    fn rendered(error: &Error, locale: &str) -> Vec<u8> {
        rendered_in(error, locale, None)
    }

    /// An error whose catalog message names the instant `at`.
    fn naming_an_instant(emitter: &str, at: i64) -> Error {
        register_catalog(
            emitter,
            "en",
            &format!(r#"{{"{emitter}.errors.AT.title": "{{at, time, medium}}"}}"#),
        );
        let mut error = Error::canceled();
        error.platform = "CONFLICT".to_owned();
        error.code = "AT".to_owned();
        error.emitter = emitter.to_owned();
        error.args.insert(
            "at".to_owned(),
            Arg::Timestamp {
                seconds: at,
                nanos: 0,
            },
        );
        error
    }

    #[test]
    fn the_build_info_names_this_core_and_the_boundary() {
        assert_eq!(
            api_bones_sdk_build_info(),
            encode_build_info("api-bones-sdk-ffi", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn an_error_renders_its_catalog_text_never_its_diagnostic_message() {
        let mut error = Error::canceled();
        error.message = "sentinel-diagnostic".to_owned();
        let result = format_error(encode_call_error(&error), "en", None);
        assert_eq!(result, rendered(&error, "en"));
        assert!(!String::from_utf8_lossy(&result).contains("sentinel-diagnostic"));
    }

    #[test]
    fn a_locale_that_is_unknown_falls_back_to_the_default_text() {
        let error = Error::canceled();
        let bytes = encode_call_error(&error);
        assert_eq!(
            format_error(bytes, "xx-YY", None),
            format_error(encode_call_error(&error), "en", None)
        );
    }

    #[test]
    fn a_time_zone_is_checked_and_an_unknown_one_is_a_refused_rule() {
        let bytes = encode_call_error(&Error::canceled());
        assert_eq!(
            format_error(bytes.clone(), "en", Some("Mars/Base".to_owned())),
            encode_format_refused("unknown_time_zone")
        );
        assert_eq!(
            format_error(bytes, "en", Some("Europe/Paris".to_owned())),
            rendered(&Error::canceled(), "en")
        );
    }

    #[test]
    fn every_platform_locale_renders_what_localize_renders_with_its_hint_and_zone() {
        let errors = [
            Error::canceled(),
            Error::invalid_argument("diagnostic"),
            Error::boundary_mismatch(2, 1, 3),
            naming_an_instant("ffi-parity", 86_400),
        ];
        assert_eq!(PLATFORM_LOCALES.len(), 29);
        for locale in PLATFORM_LOCALES {
            for error in &errors {
                for zone in [None, Some("Asia/Tokyo"), Some("America/New_York")] {
                    assert_eq!(
                        format_error(encode_call_error(error), locale, zone.map(str::to_owned)),
                        rendered_in(error, locale, zone),
                        "{locale} {} {zone:?}",
                        error.code
                    );
                }
            }
        }
    }

    #[test]
    fn a_zone_that_is_ignored_would_render_another_instant() {
        let error = naming_an_instant("ffi-zone", 0);
        let utc = format_error(encode_call_error(&error), "en", None);
        let tokyo = format_error(
            encode_call_error(&error),
            "en",
            Some("Asia/Tokyo".to_owned()),
        );
        assert_ne!(utc, tokyo, "the zone moves the instant the text names");
        assert!(String::from_utf8_lossy(&utc).contains("12:00:00"));
        assert!(String::from_utf8_lossy(&tokyo).contains("9:00:00"));
    }

    #[test]
    fn bytes_that_are_no_call_error_are_a_refused_rule() {
        assert_eq!(
            format_error(vec![0x0a], "en", None),
            encode_format_refused("malformed_error")
        );
    }

    #[test]
    fn every_kind_of_argument_is_carried_into_the_identity() {
        let mut error = Error::canceled();
        let args = [
            ("name", Arg::from("a")),
            ("n", Arg::from(7_i64)),
            ("x", Arg::from(1.5)),
            (
                "at",
                Arg::Timestamp {
                    seconds: 1,
                    nanos: 0,
                },
            ),
        ];
        error
            .args
            .extend(args.into_iter().map(|(name, arg)| (name.to_owned(), arg)));
        let result = format_error(encode_call_error(&error), "fr", None);
        assert_eq!(result, rendered(&error, "fr"));
    }
}
