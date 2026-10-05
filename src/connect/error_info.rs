// SPDX-License-Identifier: MIT
//! The emitted error code, carried across a Connect call.
//!
//! A refused call reaches the caller as a Connect code (`not_found`,
//! `permission_denied`, ...), which is too coarse to say *why* the emitter
//! refused. The emitter names the reason with an error code, and attaches it
//! to the Connect error as a `bones.v1.ErrorInfo` detail. The emitting
//! deployable and its build version ride along so a reader knows whose code
//! it is.
//!
//! The reading side has two parts:
//!
//! - [`error_info`] decodes the detail from a [`ConnectError`].
//! - [`CarriesErrorInfo`] is what an SDK error type implements. Its
//!   [`display_with_token`](CarriesErrorInfo::display_with_token) appends the
//!   canonical text token `[bones-error code=<CODE> emitter=<name>@<version>]`
//!   to the error text, so the code survives any reporter that keeps only
//!   text (a panic message, a failed assertion, a log line).
//!
//! The token is only rendered for values that match the token grammar, because
//! the values arrive from the remote peer and must not be able to forge or
//! break the surrounding text.

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use connectrpc::{ConnectError, ErrorDetail};

/// The fully-qualified protobuf name of the detail message.
pub const ERROR_INFO_TYPE: &str = "bones.v1.ErrorInfo";

const TYPE_URL_PREFIX: &str = "type.googleapis.com/";
const MAX_CODE_LEN: usize = 128;
const MAX_EMITTER_LEN: usize = 128;
const MAX_VERSION_LEN: usize = 64;

/// The wire shape of `bones.v1.ErrorInfo`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ErrorInfo {
    /// The emitted error code, e.g. `GRANT_MISSING`.
    pub code: String,
    /// The emitting deployable. Empty until a transport layer stamps it.
    pub emitter: String,
    /// The emitting deployable's build version. Empty until stamped.
    pub emitter_version: String,
}

impl ErrorInfo {
    /// An `ErrorInfo` carrying only a code; the emitter is stamped later.
    #[must_use]
    pub fn from_code(code: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            emitter: String::new(),
            emitter_version: String::new(),
        }
    }

    /// Protobuf wire bytes. Empty fields are omitted, as proto3 specifies.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        put_string(&mut out, 1, &self.code);
        put_string(&mut out, 2, &self.emitter);
        put_string(&mut out, 3, &self.emitter_version);
        out
    }

    /// Decode protobuf wire bytes. Unknown fields are skipped; malformed
    /// input yields `None`.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let mut info = Self::default();
        let mut rest = bytes;
        while !rest.is_empty() {
            let (tag, after_tag) = take_varint(rest)?;
            rest = after_tag;
            let field = tag >> 3;
            match tag & 0x7 {
                0 => {
                    let (_, after) = take_varint(rest)?;
                    rest = after;
                }
                1 => rest = rest.get(8..)?,
                5 => rest = rest.get(4..)?,
                2 => {
                    let (len, after) = take_varint(rest)?;
                    let len = usize::try_from(len).ok()?;
                    let payload = after.get(..len)?;
                    rest = after.get(len..)?;
                    let text = || String::from_utf8(payload.to_vec()).ok();
                    match field {
                        1 => info.code = text()?,
                        2 => info.emitter = text()?,
                        3 => info.emitter_version = text()?,
                        _ => {}
                    }
                }
                _ => return None,
            }
        }
        Some(info)
    }

    /// The Connect error detail carrying this message.
    #[must_use]
    pub fn to_detail(&self) -> ErrorDetail {
        ErrorDetail {
            type_url: ERROR_INFO_TYPE.to_owned(),
            value: Some(STANDARD_NO_PAD.encode(self.encode())),
            debug: None,
        }
    }

    /// The canonical text token, or `None` when the code, emitter or version
    /// is empty or does not match the token grammar.
    #[must_use]
    pub fn token(&self) -> Option<String> {
        if valid_code(&self.code)
            && valid_emitter(&self.emitter)
            && valid_version(&self.emitter_version)
        {
            Some(format!(
                "[bones-error code={} emitter={}@{}]",
                self.code, self.emitter, self.emitter_version
            ))
        } else {
            None
        }
    }
}

/// Decode the `bones.v1.ErrorInfo` detail of a Connect error, if it has one.
#[must_use]
pub fn error_info(err: &ConnectError) -> Option<ErrorInfo> {
    err.details.iter().find_map(|detail| {
        let name = detail
            .type_url
            .strip_prefix(TYPE_URL_PREFIX)
            .unwrap_or(&detail.type_url);
        if name != ERROR_INFO_TYPE {
            return None;
        }
        let encoded = detail.value.as_deref()?;
        let bytes = STANDARD_NO_PAD
            .decode(encoded)
            .or_else(|_| STANDARD.decode(encoded))
            .ok()?;
        ErrorInfo::decode(&bytes)
    })
}

/// Attach `info` to `err`, replacing any `ErrorInfo` detail already there.
#[must_use]
pub fn with_error_info(mut err: ConnectError, info: &ErrorInfo) -> ConnectError {
    err.details.retain(|detail| {
        detail
            .type_url
            .strip_prefix(TYPE_URL_PREFIX)
            .unwrap_or(&detail.type_url)
            != ERROR_INFO_TYPE
    });
    err.with_detail(info.to_detail())
}

/// Implemented by an SDK error type so a caller can read the emitted code and
/// emitter without parsing text.
pub trait CarriesErrorInfo {
    /// The decoded `ErrorInfo`, when the failure carried one.
    fn error_info(&self) -> Option<ErrorInfo>;

    /// The emitted error code.
    #[must_use]
    fn code(&self) -> Option<String> {
        self.error_info()
            .map(|info| info.code)
            .filter(|code| !code.is_empty())
    }

    /// The emitting deployable.
    #[must_use]
    fn emitter(&self) -> Option<String> {
        self.error_info()
            .map(|info| info.emitter)
            .filter(|emitter| !emitter.is_empty())
    }

    /// `base` followed by the canonical token, when there is a token to
    /// render. Use it as the body of the error's `Display`.
    #[must_use]
    fn display_with_token(&self, base: &str) -> String {
        match self.error_info().and_then(|info| info.token()) {
            Some(token) => format!("{base} {token}"),
            None => base.to_owned(),
        }
    }
}

impl CarriesErrorInfo for ConnectError {
    fn error_info(&self) -> Option<ErrorInfo> {
        error_info(self)
    }
}

fn valid_code(s: &str) -> bool {
    let mut chars = s.chars();
    s.len() <= MAX_CODE_LEN
        && chars.next().is_some_and(|c| c.is_ascii_uppercase())
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

fn valid_emitter(s: &str) -> bool {
    let mut chars = s.chars();
    s.len() <= MAX_EMITTER_LEN
        && chars
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_' || c == '-'
        })
}

fn valid_version(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_VERSION_LEN
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '+' || c == '-')
}

fn put_string(out: &mut Vec<u8>, field: u8, value: &str) {
    if value.is_empty() {
        return;
    }
    out.push((field << 3) | 2);
    let mut len = value.len() as u64;
    loop {
        let byte = u8::try_from(len & 0x7f).unwrap_or_default();
        len >>= 7;
        if len == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
    out.extend_from_slice(value.as_bytes());
}

fn take_varint(bytes: &[u8]) -> Option<(u64, &[u8])> {
    let mut value = 0u64;
    for (index, byte) in bytes.iter().enumerate().take(10) {
        value |= u64::from(byte & 0x7f) << (7 * index);
        if byte & 0x80 == 0 {
            return Some((value, &bytes[index + 1..]));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use connectrpc::ErrorCode;

    fn full() -> ErrorInfo {
        ErrorInfo {
            code: "GRANT_MISSING".into(),
            emitter: "payments".into(),
            emitter_version: "1.4.2".into(),
        }
    }

    #[test]
    fn wire_round_trips() {
        let info = full();
        assert_eq!(ErrorInfo::decode(&info.encode()), Some(info));
    }

    #[test]
    fn empty_fields_are_omitted_from_the_wire() {
        assert!(ErrorInfo::default().encode().is_empty());
        assert_eq!(
            ErrorInfo::from_code("X").encode(),
            vec![0x0a, 0x01, b'X'],
            "only field 1 is present"
        );
    }

    #[test]
    fn long_values_use_multi_byte_lengths() {
        let info = ErrorInfo::from_code("A".repeat(300));
        assert_eq!(ErrorInfo::decode(&info.encode()), Some(info));
    }

    #[test]
    fn decode_skips_unknown_fields() {
        // field 9 varint = 5, field 1 = "A", field 10 fixed32, field 11 fixed64.
        let mut bytes = vec![0x48, 0x05, 0x0a, 0x01, b'A'];
        bytes.extend([0x55, 1, 2, 3, 4]);
        bytes.extend([0x59, 1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(ErrorInfo::decode(&bytes), Some(ErrorInfo::from_code("A")));
    }

    #[test]
    fn decode_rejects_malformed_input() {
        assert_eq!(
            ErrorInfo::decode(&[0x0a, 0x05, b'A']),
            None,
            "short payload"
        );
        assert_eq!(ErrorInfo::decode(&[0x0a]), None, "missing length");
        assert_eq!(ErrorInfo::decode(&[0x0b]), None, "unsupported wire type");
        assert_eq!(
            ErrorInfo::decode(&[0x0a, 0x01, 0xff]),
            None,
            "invalid utf-8"
        );
        assert_eq!(ErrorInfo::decode(&[0x80; 11]), None, "unterminated varint");
    }

    #[test]
    fn detail_round_trips_through_a_connect_error() {
        let err = with_error_info(ConnectError::new(ErrorCode::NotFound, "gone"), &full());
        assert_eq!(err.details.len(), 1);
        assert_eq!(err.details[0].type_url, ERROR_INFO_TYPE);
        assert_eq!(error_info(&err), Some(full()));
    }

    #[test]
    fn error_info_accepts_a_prefixed_type_url_and_padded_base64() {
        let mut detail = full().to_detail();
        detail.type_url = format!("{TYPE_URL_PREFIX}{ERROR_INFO_TYPE}");
        detail.value = Some(STANDARD.encode(full().encode()));
        let err = ConnectError::new(ErrorCode::Internal, "x").with_detail(detail);
        assert_eq!(error_info(&err), Some(full()));
    }

    #[test]
    fn error_info_ignores_other_details_and_undecodable_values() {
        let other = ErrorDetail {
            type_url: "bones.v1.ValidationFailure".into(),
            value: Some(STANDARD_NO_PAD.encode(full().encode())),
            debug: None,
        };
        let bad = ErrorDetail {
            type_url: ERROR_INFO_TYPE.into(),
            value: Some("!!!".into()),
            debug: None,
        };
        let none = ErrorDetail {
            type_url: ERROR_INFO_TYPE.into(),
            value: None,
            debug: None,
        };
        let err = ConnectError::new(ErrorCode::Internal, "x")
            .with_detail(other)
            .with_detail(bad)
            .with_detail(none);
        assert_eq!(error_info(&err), None);
    }

    #[test]
    fn with_error_info_replaces_an_existing_detail() {
        let err = with_error_info(
            ConnectError::new(ErrorCode::Internal, "x"),
            &ErrorInfo::from_code("FIRST"),
        );
        let err = with_error_info(err, &full());
        assert_eq!(err.details.len(), 1);
        assert_eq!(error_info(&err), Some(full()));
    }

    #[test]
    fn token_renders_the_canonical_text() {
        assert_eq!(
            full().token().as_deref(),
            Some("[bones-error code=GRANT_MISSING emitter=payments@1.4.2]")
        );
    }

    #[test]
    fn token_requires_every_part_to_match_the_grammar() {
        let bad = |edit: fn(&mut ErrorInfo)| {
            let mut info = full();
            edit(&mut info);
            info.token()
        };
        assert_eq!(bad(|i| i.code.clear()), None);
        assert_eq!(bad(|i| i.emitter.clear()), None);
        assert_eq!(bad(|i| i.emitter_version.clear()), None);
        assert_eq!(bad(|i| i.code = "lower".into()), None);
        assert_eq!(bad(|i| i.code = "1BAD".into()), None);
        assert_eq!(bad(|i| i.code = "A".repeat(129)), None);
        assert_eq!(bad(|i| i.emitter = "Upper".into()), None);
        assert_eq!(bad(|i| i.emitter = "a b".into()), None);
        assert_eq!(bad(|i| i.emitter = "a".repeat(129)), None);
        assert_eq!(bad(|i| i.emitter_version = "1 2".into()), None);
        assert_eq!(bad(|i| i.emitter_version = "1]".into()), None);
        assert_eq!(bad(|i| i.emitter_version = "1".repeat(65)), None);
        assert!(bad(|i| i.code = "A".repeat(128)).is_some());
        assert!(bad(|i| i.emitter_version = "1.0.0-rc.1+build".into()).is_some());
    }

    #[test]
    fn a_hostile_peer_cannot_inject_text_through_the_token() {
        let err = with_error_info(
            ConnectError::new(ErrorCode::Internal, "x"),
            &ErrorInfo {
                code: "OK] fake [bones-error code=X".into(),
                emitter: "e".into(),
                emitter_version: "1".into(),
            },
        );
        assert_eq!(err.display_with_token("failed"), "failed");
    }

    #[test]
    fn connect_error_carries_info() {
        let err = with_error_info(ConnectError::new(ErrorCode::NotFound, "gone"), &full());
        assert_eq!(err.code().as_deref(), Some("GRANT_MISSING"));
        assert_eq!(err.emitter().as_deref(), Some("payments"));
        assert_eq!(
            err.display_with_token("not found"),
            "not found [bones-error code=GRANT_MISSING emitter=payments@1.4.2]"
        );
    }

    #[test]
    fn code_and_emitter_are_none_when_absent_or_empty() {
        let plain = ConnectError::new(ErrorCode::NotFound, "gone");
        assert_eq!(plain.code(), None);
        assert_eq!(plain.emitter(), None);
        assert_eq!(plain.display_with_token("gone"), "gone");
        let code_only = with_error_info(plain, &ErrorInfo::from_code("X"));
        assert_eq!(code_only.code().as_deref(), Some("X"));
        assert_eq!(code_only.emitter(), None);
        assert_eq!(code_only.display_with_token("gone"), "gone");
    }
}
