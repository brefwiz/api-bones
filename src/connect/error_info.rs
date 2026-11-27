// SPDX-License-Identifier: MIT
//! The emitted error code, carried across a Connect call.
//!
//! A refused call reaches the caller as a Connect code (`not_found`,
//! `permission_denied`, ...), which is too coarse to say *why* the emitter
//! refused. The emitter names the reason with an error code, and attaches it
//! to the Connect error as a `bones.v1.ErrorInfo` detail. The emitting
//! deployable rides along so a reader knows whose code it is.
//!
//! The reading side has two parts:
//!
//! - [`error_info`] decodes the detail from a [`ConnectError`].
//! - [`CarriesErrorInfo`] is what an SDK error type implements. Its
//!   [`display_with_token`](CarriesErrorInfo::display_with_token) appends the
//!   canonical text token `[bones-error code=<CODE> emitter=<name>]`
//!   to the error text, so the code survives any reporter that keeps only
//!   text (a panic message, a failed assertion, a log line).
//!
//! The values arrive from the remote peer and must not be able to forge or
//! break the surrounding text, so a detail whose code or emitter breaks the
//! token grammar is treated as absent, and at most [`MAX_ERROR_INFOS`] details
//! of an error are examined.
//!
//! The message also carries the typed arguments of the code's catalog string
//! (see [`ErrorArg`]), which a client formats in the viewer's locale. Like the
//! code, they come from the peer, so they never enter the text token.

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use connectrpc::{ConnectError, ErrorDetail};
use std::collections::BTreeMap;

use crate::error::{ErrorArg, ErrorArgError, ErrorInfoMember, insert_arg, validate_args};

/// The fully-qualified protobuf name of the detail message.
pub const ERROR_INFO_TYPE: &str = "bones.v1.ErrorInfo";

const TYPE_URL_PREFIX: &str = "type.googleapis.com/";
const MAX_CODE_LEN: usize = 128;
const MAX_EMITTER_LEN: usize = 128;
/// The most `ErrorInfo` details of one error that are examined.
pub const MAX_ERROR_INFOS: usize = 4;

/// The wire shape of `bones.v1.ErrorInfo`.
///
/// `code` and `emitter` form the catalog key `<emitter>.errors.<CODE>`;
/// `args` are the typed values the message of that code formats. An argument
/// is declared with its code in the emitter's proto, and a field marked
/// `debug_redact` is never one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ErrorInfo {
    /// The emitted error code, e.g. `GRANT_MISSING`.
    pub code: String,
    /// The emitting deployable. Empty until a transport layer stamps it.
    pub emitter: String,
    /// Typed arguments of the message for `code`, keyed by placeholder name.
    pub args: BTreeMap<String, ErrorArg>,
}

impl ErrorInfo {
    /// An `ErrorInfo` carrying only a code; the emitter is stamped later.
    #[must_use]
    pub fn from_code(code: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            emitter: String::new(),
            args: BTreeMap::new(),
        }
    }

    /// Carry one typed argument of the message for the code, replacing any
    /// argument of the same name.
    ///
    /// # Errors
    ///
    /// [`ErrorArgError`] when the argument, or the set it joins, breaks the
    /// bounds: at most [`MAX_ERROR_ARGS`](crate::error::MAX_ERROR_ARGS)
    /// arguments, names matching `^[a-z][a-z0-9_]{0,63}$`, strings of at most
    /// 1 KiB, finite doubles, in-range timestamps. Nothing is truncated.
    pub fn with_arg(
        mut self,
        name: impl Into<String>,
        value: impl Into<ErrorArg>,
    ) -> Result<Self, ErrorArgError> {
        insert_arg(&mut self.args, name.into(), value.into())?;
        Ok(self)
    }

    /// Protobuf wire bytes. Empty fields are omitted, as proto3 specifies.
    /// Arguments are written in name order.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        put_string(&mut out, 1, &self.code);
        put_string(&mut out, 2, &self.emitter);
        put_args(&mut out, 4, &self.args);
        out
    }

    /// Decode protobuf wire bytes. Unknown fields are skipped; malformed
    /// input yields `None`.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let mut info = Self::default();
        for (field, value) in wire_fields(bytes)? {
            match (field, value) {
                (1, Wire::Len(payload)) => info.code = text(payload)?,
                (2, Wire::Len(payload)) => info.emitter = text(payload)?,
                (4, Wire::Len(payload)) => {
                    if let Some((name, arg)) = decode_arg_entry(payload).ok()? {
                        info.args.insert(name, arg);
                    }
                }
                _ => {}
            }
        }
        validate_args(&info.args).ok()?;
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

    /// Whether the code is grammar-valid and the emitter is grammar-valid or
    /// not yet stamped.
    #[must_use]
    pub fn is_well_formed(&self) -> bool {
        valid_code(&self.code) && (self.emitter.is_empty() || valid_emitter(&self.emitter))
    }

    /// The canonical text token, or `None` when the code or emitter is empty
    /// or does not match the token grammar.
    ///
    /// The token carries the identity only, never an argument.
    #[must_use]
    pub fn token(&self) -> Option<String> {
        if valid_code(&self.code) && valid_emitter(&self.emitter) {
            Some(format!(
                "[bones-error code={} emitter={}]",
                self.code, self.emitter
            ))
        } else {
            None
        }
    }
}

/// The wire shape of `bones.v1.ValidationFailure`.
///
/// `rule` and `args` form the message `bones.validation.<rule>` formats in
/// the viewer's locale. The arguments are bounded as `ErrorInfo.args` are, on
/// decode and when attached with [`ValidationFailureDetail::to_detail`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidationFailureDetail {
    /// JSON Pointer to the failing field.
    pub field: String,
    /// The English message, the fallback when the catalog has no string.
    pub message: String,
    /// The machine-readable rule that failed. Empty when it names no rule.
    pub rule: String,
    /// Typed arguments of the message for `rule`, keyed by placeholder name.
    pub args: BTreeMap<String, ErrorArg>,
}

/// The fully-qualified protobuf name of the validation detail message.
pub const VALIDATION_FAILURE_TYPE: &str = "bones.v1.ValidationFailure";

impl ValidationFailureDetail {
    /// Carry one typed argument of the message for the rule, replacing any
    /// argument of the same name.
    ///
    /// # Errors
    ///
    /// [`ErrorArgError`] when the argument, or the set it joins, breaks the
    /// bounds. Nothing is truncated.
    pub fn with_arg(
        mut self,
        name: impl Into<String>,
        value: impl Into<ErrorArg>,
    ) -> Result<Self, ErrorArgError> {
        insert_arg(&mut self.args, name.into(), value.into())?;
        Ok(self)
    }

    /// Protobuf wire bytes. Empty fields are omitted, as proto3 specifies.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        put_string(&mut out, 1, &self.field);
        put_string(&mut out, 2, &self.message);
        put_string(&mut out, 3, &self.rule);
        put_args(&mut out, 4, &self.args);
        out
    }

    /// Decode protobuf wire bytes. Unknown fields are skipped; malformed
    /// input, or arguments that break the bounds, yield `None`.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let (failure, args_decoded) = decode_failure_unchecked(bytes)?;
        if !args_decoded {
            return None;
        }
        validate_args(&failure.args).ok()?;
        Some(failure)
    }

    /// The Connect error detail carrying this message.
    ///
    /// # Errors
    ///
    /// [`ErrorArgError`] when the arguments break the bounds.
    pub fn to_detail(&self) -> Result<ErrorDetail, ErrorArgError> {
        validate_args(&self.args)?;
        Ok(ErrorDetail {
            type_url: VALIDATION_FAILURE_TYPE.to_owned(),
            value: Some(STANDARD_NO_PAD.encode(self.encode())),
            debug: None,
        })
    }
}

impl From<&ErrorInfo> for ErrorInfoMember {
    fn from(info: &ErrorInfo) -> Self {
        Self {
            code: info.code.clone(),
            emitter: info.emitter.clone(),
            args: info.args.clone(),
        }
    }
}

impl From<&ErrorInfoMember> for ErrorInfo {
    fn from(member: &ErrorInfoMember) -> Self {
        Self {
            code: member.code.clone(),
            emitter: member.emitter.clone(),
            args: member.args.clone(),
        }
    }
}

/// The well-formed `bones.v1.ErrorInfo` details of a Connect error, in order.
///
/// Only the first [`MAX_ERROR_INFOS`] `ErrorInfo` details are examined, and a
/// detail whose code or emitter breaks the token grammar is dropped.
#[must_use]
pub fn error_infos(err: &ConnectError) -> Vec<ErrorInfo> {
    err.details
        .iter()
        .filter(|detail| is_error_info(detail))
        .take(MAX_ERROR_INFOS)
        .filter_map(|detail| {
            let encoded = detail.value.as_deref()?;
            let bytes = STANDARD_NO_PAD
                .decode(encoded)
                .or_else(|_| STANDARD.decode(encoded))
                .ok()?;
            ErrorInfo::decode(&bytes)
        })
        .filter(ErrorInfo::is_well_formed)
        .collect()
}

/// The first well-formed `bones.v1.ErrorInfo` detail of a Connect error. A
/// detail that breaks the token grammar is treated as absent.
#[must_use]
pub fn error_info(err: &ConnectError) -> Option<ErrorInfo> {
    error_infos(err).into_iter().next()
}

/// The well-formed `bones.v1.ValidationFailure` details of a Connect error, in
/// detail order, each with its typed arguments.
///
/// A detail that does not decode, or whose arguments break the bounds, is
/// treated as absent; nothing is truncated and no violation is dropped for
/// count.
#[must_use]
pub fn violations_of(err: &ConnectError) -> Vec<ValidationFailureDetail> {
    validation_failure_bytes(err)
        .filter_map(|bytes| ValidationFailureDetail::decode(&bytes))
        .collect()
}

/// The wire bytes of each `bones.v1.ValidationFailure` detail, in detail order.
fn validation_failure_bytes(err: &ConnectError) -> impl Iterator<Item = Vec<u8>> + '_ {
    err.details
        .iter()
        .filter(|detail| {
            detail
                .type_url
                .strip_prefix(TYPE_URL_PREFIX)
                .unwrap_or(&detail.type_url)
                == VALIDATION_FAILURE_TYPE
        })
        .filter_map(|detail| {
            let encoded = detail.value.as_deref()?;
            STANDARD_NO_PAD
                .decode(encoded)
                .or_else(|_| STANDARD.decode(encoded))
                .ok()
        })
}

/// Every `bones.v1.ValidationFailure` detail that parses as a message, in
/// detail order, with its arguments as sent. `false` marks an argument entry
/// that does not decode (the arguments are then empty). Bounds are the
/// caller's to check.
#[cfg(feature = "i18n")]
pub fn violations_unchecked(err: &ConnectError) -> Vec<(ValidationFailureDetail, bool)> {
    validation_failure_bytes(err)
        .filter_map(|bytes| decode_failure_unchecked(&bytes))
        .collect()
}

fn decode_failure_unchecked(bytes: &[u8]) -> Option<(ValidationFailureDetail, bool)> {
    let mut failure = ValidationFailureDetail::default();
    let mut args_decoded = true;
    for (field, value) in wire_fields(bytes)? {
        match (field, value) {
            (1, Wire::Len(payload)) => failure.field = text(payload)?,
            (2, Wire::Len(payload)) => failure.message = text(payload)?,
            (3, Wire::Len(payload)) => failure.rule = text(payload)?,
            (4, Wire::Len(payload)) => match decode_arg_entry(payload) {
                Ok(Some((name, arg))) => {
                    failure.args.insert(name, arg);
                }
                Ok(None) => {}
                Err(Malformed) => args_decoded = false,
            },
            _ => {}
        }
    }
    if !args_decoded {
        failure.args.clear();
    }
    Some((failure, args_decoded))
}

fn is_error_info(detail: &ErrorDetail) -> bool {
    detail
        .type_url
        .strip_prefix(TYPE_URL_PREFIX)
        .unwrap_or(&detail.type_url)
        == ERROR_INFO_TYPE
}

/// Attach `info` to `err`, replacing any `ErrorInfo` detail already there.
///
/// # Errors
///
/// [`ErrorArgError`] when the arguments of `info` break the bounds. The
/// arguments are never truncated to fit.
pub fn try_with_error_info(
    mut err: ConnectError,
    info: &ErrorInfo,
) -> Result<ConnectError, ErrorArgError> {
    validate_args(&info.args)?;
    err.details.retain(|detail| !is_error_info(detail));
    Ok(err.with_detail(info.to_detail()))
}

/// Attach `info` to `err`, replacing any `ErrorInfo` detail already there.
///
/// Arguments that break the bounds are a bug of the emitter: the call then
/// returns an `internal` error naming the broken bound, and the original
/// error and its arguments are not sent. Use [`try_with_error_info`] to
/// handle it.
#[must_use]
pub fn with_error_info(err: ConnectError, info: &ErrorInfo) -> ConnectError {
    try_with_error_info(err, info)
        .unwrap_or_else(|bound| ConnectError::internal(format!("invalid ErrorInfo args: {bound}")))
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

    /// The typed arguments of the message for the code, empty when the
    /// failure carried none.
    #[must_use]
    fn args(&self) -> BTreeMap<String, ErrorArg> {
        self.error_info().map(|info| info.args).unwrap_or_default()
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

pub(super) fn put_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = u8::try_from(value & 0x7f).unwrap_or_default();
        value >>= 7;
        if value == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
}

/// A length-delimited field, written even when empty.
pub(super) fn put_len(out: &mut Vec<u8>, field: u8, payload: &[u8]) {
    out.push((field << 3) | 2);
    put_varint(out, payload.len() as u64);
    out.extend_from_slice(payload);
}

/// A string field, omitted when empty.
fn put_string(out: &mut Vec<u8>, field: u8, value: &str) {
    if !value.is_empty() {
        put_len(out, field, value.as_bytes());
    }
}

/// A varint field, omitted when zero. A negative value is sign-extended to
/// ten bytes, as protobuf writes an `int32` or `int64`.
fn put_int(out: &mut Vec<u8>, field: u8, value: i64) {
    if value != 0 {
        out.push(field << 3);
        put_varint(out, value as u64);
    }
}

fn encode_arg(arg: &ErrorArg) -> Vec<u8> {
    let mut out = Vec::new();
    match arg {
        ErrorArg::String(value) => put_len(&mut out, 1, value.as_bytes()),
        ErrorArg::Int64(value) => {
            out.push(2 << 3);
            put_varint(&mut out, *value as u64);
        }
        ErrorArg::Double(value) => {
            out.push((3 << 3) | 1);
            out.extend_from_slice(&value.to_le_bytes());
        }
        ErrorArg::Timestamp { seconds, nanos } => {
            let mut timestamp = Vec::new();
            put_int(&mut timestamp, 1, *seconds);
            put_int(&mut timestamp, 2, i64::from(*nanos));
            put_len(&mut out, 4, &timestamp);
        }
    }
    out
}

fn put_args(out: &mut Vec<u8>, field: u8, args: &BTreeMap<String, ErrorArg>) {
    for (name, arg) in args {
        let mut entry = Vec::new();
        put_len(&mut entry, 1, name.as_bytes());
        put_len(&mut entry, 2, &encode_arg(arg));
        put_len(out, field, &entry);
    }
}

/// One decoded field value. Groups are not supported.
pub(super) enum Wire<'a> {
    Varint(u64),
    Fixed64([u8; 8]),
    Len(&'a [u8]),
    Skipped,
}

/// Every field of a message in order, or `None` when the bytes are malformed.
pub(super) fn wire_fields(bytes: &[u8]) -> Option<Vec<(u64, Wire<'_>)>> {
    let mut fields = Vec::new();
    let mut rest = bytes;
    while !rest.is_empty() {
        let (tag, after_tag) = take_varint(rest)?;
        rest = after_tag;
        let field = tag >> 3;
        let value = match tag & 0x7 {
            0 => {
                let (value, after) = take_varint(rest)?;
                rest = after;
                Wire::Varint(value)
            }
            1 => {
                let raw: [u8; 8] = rest.get(..8)?.try_into().ok()?;
                rest = rest.get(8..)?;
                Wire::Fixed64(raw)
            }
            5 => {
                rest = rest.get(4..)?;
                Wire::Skipped
            }
            2 => {
                let (len, after) = take_varint(rest)?;
                let len = usize::try_from(len).ok()?;
                let payload = after.get(..len)?;
                rest = after.get(len..)?;
                Wire::Len(payload)
            }
            _ => return None,
        };
        fields.push((field, value));
    }
    Some(fields)
}

fn text(payload: &[u8]) -> Option<String> {
    String::from_utf8(payload.to_vec()).ok()
}

/// A wire message that does not decode.
struct Malformed;

fn require<T>(value: Option<T>) -> Result<T, Malformed> {
    value.ok_or(Malformed)
}

/// One `args` map entry. `Ok(None)` is an entry that carries no value, which
/// is dropped.
fn decode_arg_entry(payload: &[u8]) -> Result<Option<(String, ErrorArg)>, Malformed> {
    let mut name = String::new();
    let mut arg = None;
    for (field, value) in require(wire_fields(payload))? {
        match (field, value) {
            (1, Wire::Len(key)) => name = require(text(key))?,
            (2, Wire::Len(body)) => arg = decode_arg(body)?,
            _ => {}
        }
    }
    Ok(arg.map(|arg| (name, arg)))
}

/// One `ErrorArg`. `Ok(None)` is a message with no oneof member set. The last
/// member set wins.
fn decode_arg(bytes: &[u8]) -> Result<Option<ErrorArg>, Malformed> {
    let mut arg = None;
    for (field, value) in require(wire_fields(bytes))? {
        match (field, value) {
            (1, Wire::Len(payload)) => arg = Some(ErrorArg::String(require(text(payload))?)),
            (2, Wire::Varint(value)) => arg = Some(ErrorArg::Int64(value as i64)),
            (3, Wire::Fixed64(raw)) => arg = Some(ErrorArg::Double(f64::from_le_bytes(raw))),
            (4, Wire::Len(payload)) => arg = Some(require(decode_timestamp(payload))?),
            _ => {}
        }
    }
    Ok(arg)
}

fn decode_timestamp(bytes: &[u8]) -> Option<ErrorArg> {
    let (mut seconds, mut nanos) = (0_i64, 0_i32);
    for (field, value) in wire_fields(bytes)? {
        match (field, value) {
            (1, Wire::Varint(value)) => seconds = value as i64,
            (2, Wire::Varint(value)) => nanos = value as i32,
            _ => {}
        }
    }
    Some(ErrorArg::Timestamp { seconds, nanos })
}

fn take_varint(mut bytes: &[u8]) -> Option<(u64, &[u8])> {
    let value = buffa::encoding::decode_varint(&mut bytes).ok()?;
    Some((value, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{MAX_ERROR_ARG_STRING_BYTES, MAX_ERROR_ARGS};
    use connectrpc::ErrorCode;

    fn full() -> ErrorInfo {
        ErrorInfo {
            code: "GRANT_MISSING".into(),
            emitter: "payments".into(),
            ..Default::default()
        }
    }

    #[test]
    fn wire_round_trips() {
        let info = full();
        assert_eq!(ErrorInfo::decode(&info.encode()), Some(info));
    }

    #[test]
    fn empty_fields_are_omitted_from_the_wire() {
        assert_eq!(ErrorInfo::default().encode(), [] as [u8; 0]);
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
            Some("[bones-error code=GRANT_MISSING emitter=payments]")
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
        assert_eq!(bad(|i| i.code = "lower".into()), None);
        assert_eq!(bad(|i| i.code = "1BAD".into()), None);
        assert_eq!(bad(|i| i.code = "A".repeat(129)), None);
        assert_eq!(bad(|i| i.emitter = "Upper".into()), None);
        assert_eq!(bad(|i| i.emitter = "a b".into()), None);
        assert_eq!(bad(|i| i.emitter = "a".repeat(129)), None);
        assert!(bad(|i| i.code = "A".repeat(128)).is_some());
    }

    #[test]
    fn a_hostile_peer_cannot_inject_text_through_the_token() {
        let err = with_error_info(
            ConnectError::new(ErrorCode::Internal, "x"),
            &ErrorInfo {
                code: "OK] fake [bones-error code=X".into(),
                emitter: "e".into(),
                ..Default::default()
            },
        );
        assert_eq!(err.display_with_token("failed"), "failed");
        assert_eq!(err.code(), None, "a malformed detail is treated as absent");
        assert_eq!(error_info(&err), None);
    }

    #[test]
    fn a_malformed_emitter_makes_the_detail_absent() {
        let err = with_error_info(
            ConnectError::new(ErrorCode::Internal, "x"),
            &ErrorInfo {
                code: "OK".into(),
                emitter: "Not Valid".into(),
                ..Default::default()
            },
        );
        assert_eq!(error_info(&err), None);
    }

    #[test]
    fn only_a_bounded_number_of_details_is_examined() {
        let bad = ErrorInfo::from_code("lower").to_detail();
        let mut err = ConnectError::new(ErrorCode::Internal, "x");
        for _ in 0..MAX_ERROR_INFOS {
            err = err.with_detail(bad.clone());
        }
        err = err.with_detail(full().to_detail());
        assert_eq!(error_info(&err), None, "the valid detail is past the cap");

        let mut err = ConnectError::new(ErrorCode::Internal, "x");
        for _ in 0..=MAX_ERROR_INFOS {
            err = err.with_detail(full().to_detail());
        }
        assert_eq!(error_infos(&err).len(), MAX_ERROR_INFOS);
    }

    #[test]
    fn the_removed_version_field_is_ignored_on_the_wire() {
        // field 3 is reserved: an old writer's value is skipped, not read.
        let mut bytes = ErrorInfo::from_code("A").encode();
        bytes.extend([0x1a, 0x05, b'1', b'.', b'4', b'.', b'2']);
        assert_eq!(ErrorInfo::decode(&bytes), Some(ErrorInfo::from_code("A")));
    }

    #[test]
    fn connect_error_carries_info() {
        let err = with_error_info(ConnectError::new(ErrorCode::NotFound, "gone"), &full());
        assert_eq!(err.code().as_deref(), Some("GRANT_MISSING"));
        assert_eq!(err.emitter().as_deref(), Some("payments"));
        assert_eq!(
            err.display_with_token("not found"),
            "not found [bones-error code=GRANT_MISSING emitter=payments]"
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

    fn with_args() -> ErrorInfo {
        ErrorInfo::from_code("SEAT_TAKEN")
            .with_arg("seat", "12A")
            .unwrap()
            .with_arg("row", -4_i64)
            .unwrap()
            .with_arg("price", 12.5_f64)
            .unwrap()
            .with_arg(
                "held_until",
                ErrorArg::Timestamp {
                    seconds: -86_400,
                    nanos: 250,
                },
            )
            .unwrap()
    }

    #[test]
    fn golden_wire_of_an_int64_arg() {
        let info = ErrorInfo::from_code("A").with_arg("n", 5_i64).unwrap();
        let bytes = [
            0x0a, 0x01, 0x41, 0x22, 0x07, 0x0a, 0x01, 0x6e, 0x12, 0x02, 0x10, 0x05,
        ];
        assert_eq!(info.encode(), bytes);
        assert_eq!(STANDARD.encode(bytes), "CgFBIgcKAW4SAhAF");
        assert_eq!(ErrorInfo::decode(&bytes), Some(info));
    }

    #[test]
    fn all_four_arg_types_round_trip_including_negative_values() {
        let info = with_args();
        assert_eq!(ErrorInfo::decode(&info.encode()), Some(info));
        let boundary = ErrorInfo::from_code("B")
            .with_arg("low", i64::MIN)
            .unwrap()
            .with_arg("high", i64::MAX)
            .unwrap()
            .with_arg("tiny", f64::MIN_POSITIVE)
            .unwrap()
            .with_arg("empty", "")
            .unwrap();
        assert_eq!(ErrorInfo::decode(&boundary.encode()), Some(boundary));
    }

    #[test]
    fn an_empty_string_arg_keeps_its_value_on_the_wire() {
        let info = ErrorInfo::from_code("A").with_arg("s", "").unwrap();
        assert_eq!(
            ErrorInfo::decode(&info.encode()).map(|i| i.args["s"].clone()),
            Some(ErrorArg::String(String::new()))
        );
    }

    #[test]
    fn a_valueless_entry_is_dropped() {
        // entry with a key and no value member
        let mut bytes = ErrorInfo::from_code("A").encode();
        bytes.extend([0x22, 0x03, 0x0a, 0x01, b'n']);
        // entry whose value message sets no oneof member
        bytes.extend([0x22, 0x05, 0x0a, 0x01, b'm', 0x12, 0x00]);
        let decoded = ErrorInfo::decode(&bytes).expect("decodes");
        assert!(decoded.args.is_empty());
    }

    #[test]
    fn the_last_duplicate_key_wins() {
        let mut bytes = ErrorInfo::from_code("A").encode();
        bytes.extend([0x22, 0x07, 0x0a, 0x01, b'n', 0x12, 0x02, 0x10, 0x01]);
        bytes.extend([0x22, 0x07, 0x0a, 0x01, b'n', 0x12, 0x02, 0x10, 0x09]);
        let decoded = ErrorInfo::decode(&bytes).expect("decodes");
        assert_eq!(decoded.args.len(), 1);
        assert_eq!(decoded.args["n"], ErrorArg::Int64(9));
    }

    #[test]
    fn a_malformed_entry_makes_the_message_none() {
        // entry length runs past the end
        assert_eq!(ErrorInfo::decode(&[0x22, 0x09, 0x0a, 0x01, b'n']), None);
        // entry key is not UTF-8
        assert_eq!(ErrorInfo::decode(&[0x22, 0x03, 0x0a, 0x01, 0xff]), None);
        // arg value truncated inside the oneof member
        assert_eq!(
            ErrorInfo::decode(&[0x22, 0x07, 0x0a, 0x01, b'n', 0x12, 0x02, 0x1a, 0x09]),
            None
        );
    }

    #[test]
    fn args_never_enter_the_token() {
        let plain = ErrorInfo {
            code: "SEAT_TAKEN".into(),
            emitter: "bookings".into(),
            ..Default::default()
        };
        let with = ErrorInfo {
            emitter: "bookings".into(),
            ..with_args()
        };
        assert_eq!(with.token(), plain.token());
        assert_eq!(
            with.token().as_deref(),
            Some("[bones-error code=SEAT_TAKEN emitter=bookings]")
        );
        let err = with_error_info(ConnectError::new(ErrorCode::Aborted, "taken"), &with);
        assert_eq!(
            err.display_with_token("taken"),
            "taken [bones-error code=SEAT_TAKEN emitter=bookings]"
        );
        assert_eq!(err.args(), with.args);
    }

    #[test]
    fn connect_error_carries_args_to_the_reader() {
        let err = with_error_info(ConnectError::new(ErrorCode::Aborted, "taken"), &with_args());
        let read = error_info(&err).expect("info");
        assert_eq!(read.args, with_args().args);
        assert!(ConnectError::new(ErrorCode::Aborted, "x").args().is_empty());
    }

    #[test]
    fn more_than_the_most_args_is_refused_on_construction() {
        let mut info = ErrorInfo::from_code("A");
        for i in 0..MAX_ERROR_ARGS {
            info = info
                .with_arg(format!("k{i}"), i64::try_from(i).unwrap())
                .unwrap();
        }
        assert_eq!(
            info.clone().with_arg("one_more", 1_i64),
            Err(ErrorArgError::TooMany {
                max: MAX_ERROR_ARGS
            })
        );
        // replacing an existing name is not growth
        assert!(info.with_arg("k0", 7_i64).is_ok());
    }

    #[test]
    fn key_grammar_is_enforced_on_construction() {
        let long = "a".repeat(65);
        for bad in [
            "",
            "Seat",
            "1seat",
            "_seat",
            "se-at",
            "se at",
            "séat",
            long.as_str(),
        ] {
            assert_eq!(
                ErrorInfo::from_code("A").with_arg(bad, 1_i64),
                Err(ErrorArgError::InvalidKey {
                    key: bad.to_owned()
                }),
                "{bad:?}"
            );
        }
        let longest = "a".repeat(64);
        for good in ["a", "seat_2", longest.as_str()] {
            assert!(
                ErrorInfo::from_code("A").with_arg(good, 1_i64).is_ok(),
                "{good}"
            );
        }
    }

    #[test]
    fn string_values_are_bounded_to_one_kib() {
        let ok = "x".repeat(MAX_ERROR_ARG_STRING_BYTES);
        assert!(ErrorInfo::from_code("A").with_arg("s", ok).is_ok());
        let too_long = "x".repeat(MAX_ERROR_ARG_STRING_BYTES + 1);
        assert_eq!(
            ErrorInfo::from_code("A").with_arg("s", too_long),
            Err(ErrorArgError::StringTooLong {
                key: "s".into(),
                len: MAX_ERROR_ARG_STRING_BYTES + 1,
            })
        );
    }

    #[test]
    fn non_finite_doubles_and_bad_timestamps_are_refused_on_construction() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                ErrorInfo::from_code("A").with_arg("d", value),
                Err(ErrorArgError::NonFiniteDouble { key: "d".into() })
            );
        }
        for (seconds, nanos) in [
            (0, -1),
            (0, 1_000_000_000),
            (253_402_300_800, 0),
            (-62_135_596_801, 0),
        ] {
            assert_eq!(
                ErrorInfo::from_code("A").with_arg("t", ErrorArg::Timestamp { seconds, nanos }),
                Err(ErrorArgError::InvalidTimestamp { key: "t".into() })
            );
        }
    }

    /// The wire bytes of an `ErrorInfo` holding one `args` entry, with `value`
    /// as the already-encoded `ErrorArg`.
    fn one_arg(key: &[u8], value: &[u8]) -> Vec<u8> {
        let mut entry = Vec::new();
        put_len(&mut entry, 1, key);
        put_len(&mut entry, 2, value);
        let mut bytes = Vec::new();
        put_len(&mut bytes, 4, &entry);
        bytes
    }

    #[test]
    fn decode_refuses_args_out_of_bounds_instead_of_truncating() {
        // 33 well-formed entries
        let mut over = ErrorInfo::from_code("A").encode();
        for i in 0..=MAX_ERROR_ARGS {
            let key = format!("k{i:02}");
            let mut entry = Vec::new();
            put_len(&mut entry, 1, key.as_bytes());
            put_len(&mut entry, 2, &[0x10, 0x01]);
            put_len(&mut over, 4, &entry);
        }
        assert_eq!(ErrorInfo::decode(&over), None);
        // bad key
        assert_eq!(ErrorInfo::decode(&one_arg(b"Bad", &[0x10, 0x01])), None);
        // 1 KiB + 1 string
        let mut value = Vec::new();
        put_len(&mut value, 1, "x".repeat(1025).as_bytes());
        assert_eq!(ErrorInfo::decode(&one_arg(b"s", &value)), None);
        // NaN and infinity on the wire
        for non_finite in [f64::NAN, f64::INFINITY] {
            let mut value = vec![(3 << 3) | 1];
            value.extend(non_finite.to_le_bytes());
            assert_eq!(ErrorInfo::decode(&one_arg(b"d", &value)), None);
        }
    }

    #[test]
    fn a_detail_with_out_of_bounds_args_is_treated_as_absent() {
        let mut info = ErrorInfo::from_code("A");
        info.args.insert("Bad Key".into(), ErrorArg::Int64(1));
        let bytes = info.encode();
        let detail = ErrorDetail {
            type_url: ERROR_INFO_TYPE.into(),
            value: Some(STANDARD_NO_PAD.encode(bytes)),
            debug: None,
        };
        let err = ConnectError::new(ErrorCode::Aborted, "x").with_detail(detail);
        assert_eq!(error_info(&err), None);
    }

    #[test]
    fn emitting_out_of_bounds_args_fails_loudly_and_try_form_is_typed() {
        let mut info = ErrorInfo::from_code("A");
        info.args.insert("Bad Key".into(), ErrorArg::Int64(1));
        let base = ConnectError::new(ErrorCode::Aborted, "x");
        assert_eq!(
            try_with_error_info(base.clone(), &info).map(|_| ()),
            Err(ErrorArgError::InvalidKey {
                key: "Bad Key".into()
            })
        );
        let loud = with_error_info(base, &info);
        assert_eq!(loud.code, ErrorCode::Internal);
        assert!(
            loud.message
                .as_deref()
                .is_some_and(|m| m.contains("Bad Key"))
        );
        assert_eq!(error_info(&loud), None);
    }

    #[test]
    fn member_conversion_carries_args_both_ways() {
        let info = ErrorInfo {
            emitter: "bookings".into(),
            ..with_args()
        };
        let member = ErrorInfoMember::from(&info);
        assert_eq!(member.args, info.args);
        assert_eq!(ErrorInfo::from(&member), info);
    }

    fn failure() -> ValidationFailureDetail {
        ValidationFailureDetail {
            field: "/guests".into(),
            message: "must be at most 4".into(),
            rule: "max".into(),
            ..Default::default()
        }
        .with_arg("limit", 4_i64)
        .unwrap()
    }

    #[test]
    fn validation_failure_round_trips_with_args() {
        let failure = failure();
        assert_eq!(
            ValidationFailureDetail::decode(&failure.encode()),
            Some(failure.clone())
        );
        let detail = failure.to_detail().expect("in bounds");
        assert_eq!(detail.type_url, VALIDATION_FAILURE_TYPE);
        let bytes = STANDARD_NO_PAD
            .decode(detail.value.expect("value"))
            .unwrap();
        assert_eq!(ValidationFailureDetail::decode(&bytes), Some(failure));
    }

    #[test]
    fn validation_failure_golden_wire() {
        let failure = ValidationFailureDetail {
            rule: "r".into(),
            ..Default::default()
        }
        .with_arg("n", 5_i64)
        .unwrap();
        assert_eq!(
            failure.encode(),
            [
                0x1a, 0x01, b'r', 0x22, 0x07, 0x0a, 0x01, 0x6e, 0x12, 0x02, 0x10, 0x05
            ]
        );
    }

    #[test]
    fn violations_of_reads_every_violation_in_order() {
        let first = failure();
        let second = ValidationFailureDetail {
            field: "/name".into(),
            rule: "required".into(),
            ..Default::default()
        };
        let mut err = ConnectError::new(ErrorCode::InvalidArgument, "bad");
        err.details.push(first.to_detail().unwrap());
        err.details
            .push(ErrorInfo::from_code("GRANT_MISSING").to_detail());
        err.details.push(second.to_detail().unwrap());
        assert_eq!(violations_of(&err), vec![first, second]);
    }

    #[test]
    fn violations_of_is_empty_without_violation_details() {
        let err = ConnectError::new(ErrorCode::InvalidArgument, "bad");
        assert_eq!(violations_of(&err).len(), 0);
        let info_only = with_error_info(err, &ErrorInfo::from_code("GRANT_MISSING"));
        assert_eq!(violations_of(&info_only).len(), 0);
    }

    #[test]
    fn violations_of_never_truncates() {
        let mut err = ConnectError::new(ErrorCode::InvalidArgument, "bad");
        for _ in 0..(MAX_ERROR_INFOS * 4) {
            err.details.push(failure().to_detail().unwrap());
        }
        assert_eq!(violations_of(&err).len(), MAX_ERROR_INFOS * 4);
    }

    #[test]
    fn violations_of_skips_a_detail_that_does_not_decode() {
        let mut broken = failure();
        broken.args.insert("Bad".into(), ErrorArg::Int64(1));
        let mut err = ConnectError::new(ErrorCode::InvalidArgument, "bad");
        err.details.push(ErrorDetail {
            type_url: VALIDATION_FAILURE_TYPE.to_owned(),
            value: Some(STANDARD_NO_PAD.encode(broken.encode())),
            debug: None,
        });
        err.details.push(failure().to_detail().unwrap());
        assert_eq!(violations_of(&err), vec![failure()]);
    }

    #[test]
    fn validation_failure_bounds_apply_on_decode_and_attach() {
        let mut broken = failure();
        broken.args.insert("Bad".into(), ErrorArg::Int64(1));
        assert!(broken.to_detail().is_err());
        assert_eq!(ValidationFailureDetail::decode(&broken.encode()), None);
        assert_eq!(
            ValidationFailureDetail::default().with_arg("A", 1_i64),
            Err(ErrorArgError::InvalidKey { key: "A".into() })
        );
        assert_eq!(ValidationFailureDetail::decode(&[0x22, 0x09]), None);
    }
}
