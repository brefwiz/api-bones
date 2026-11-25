// SPDX-License-Identifier: MIT
//! The error of a Layer 2 call, in the wire form `api-bones.error/v1`.
//!
//! The Rust core sorts every failure into one [`Error`]; a host revives its
//! JSON into its own error class. The fields are `code`, `emitter`,
//! `platform`, `connectCode`, `message`, `args` and, for a list that failed
//! part-way, `page`.
//!
//! A refusal the Layer 2 core makes itself carries the emitter `bones` and no
//! catalog key of its own, so a host renders the text of its platform code.

use std::collections::BTreeMap;
use std::fmt;
use std::time::SystemTime;

use api_bones::connect::{ConnectCode, ErrorArg, error_info};
use api_bones_page::{PageError, PageErrorKind, reason};
use connectrpc::ConnectError;
use serde_json::{Map, Value, json};

use crate::wire::{BadValue, double_value, format_rfc3339, system_time_parts};

/// The emitter of a refusal the Layer 2 core makes itself.
pub const EMITTER: &str = "bones";
/// A handle's `next()` was called while another `next()` was still pending.
pub const HANDLE_BUSY: &str = "HANDLE_BUSY";
/// The caller's identity changed between the start of a call and its answer.
pub const IDENTITY_CHANGED: &str = "IDENTITY_CHANGED";
/// A call did not finish within its call budget.
pub const CALL_BUDGET_ELAPSED: &str = "CALL_BUDGET_ELAPSED";
/// The caller cancelled, aborted or dropped the call.
pub const CANCELED: &str = "CANCELED";

/// Every Connect code, the code that maps to `INTERNAL_SERVER_ERROR` first so
/// that mapping a platform code back lands on it.
const CODES: [ConnectCode; 16] = [
    ConnectCode::Internal,
    ConnectCode::Canceled,
    ConnectCode::Unknown,
    ConnectCode::InvalidArgument,
    ConnectCode::DeadlineExceeded,
    ConnectCode::NotFound,
    ConnectCode::AlreadyExists,
    ConnectCode::PermissionDenied,
    ConnectCode::ResourceExhausted,
    ConnectCode::FailedPrecondition,
    ConnectCode::Aborted,
    ConnectCode::OutOfRange,
    ConnectCode::Unimplemented,
    ConnectCode::Unavailable,
    ConnectCode::DataLoss,
    ConnectCode::Unauthenticated,
];

pub use api_bones::connect::platform_of_connect;

/// The Connect code an emitter's platform code stands for; a platform code no
/// Connect code maps to gives `unknown`.
fn connect_of_platform(platform: &str) -> ConnectCode {
    CODES
        .into_iter()
        .find(|code| platform_of_connect(*code) == platform)
        .unwrap_or(ConnectCode::Unknown)
}

/// One typed argument of an error message.
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    /// A text value.
    String(String),
    /// A whole number, which crosses as a decimal string.
    Int64(i64),
    /// A real number, which crosses as a number or as `"NaN"`, `"Infinity"`,
    /// `"-Infinity"`.
    Double(f64),
    /// An instant, as RFC 3339 UTC text.
    Timestamp(String),
}

impl Arg {
    /// The `{kind, value}` object of the error JSON.
    #[must_use]
    pub fn to_value(&self) -> Value {
        match self {
            Self::String(text) => json!({"kind": "string", "value": text}),
            Self::Int64(n) => json!({"kind": "int64", "value": n.to_string()}),
            Self::Double(x) => json!({"kind": "double", "value": double_value(*x)}),
            Self::Timestamp(text) => json!({"kind": "timestamp", "value": text}),
        }
    }
}

/// A timestamp argument; an instant RFC 3339 cannot write stays as text.
fn timestamp_arg(seconds: i64, nanos: u32) -> Arg {
    format_rfc3339(seconds, nanos).map_or_else(
        |_| Arg::String(format!("{seconds}.{nanos:09}")),
        Arg::Timestamp,
    )
}

impl From<String> for Arg {
    fn from(text: String) -> Self {
        Self::String(text)
    }
}

impl From<&str> for Arg {
    fn from(text: &str) -> Self {
        Self::String(text.to_owned())
    }
}

impl From<i64> for Arg {
    fn from(n: i64) -> Self {
        Self::Int64(n)
    }
}

impl From<f64> for Arg {
    fn from(x: f64) -> Self {
        Self::Double(x)
    }
}

impl From<SystemTime> for Arg {
    fn from(time: SystemTime) -> Self {
        let (seconds, nanos) = system_time_parts(time);
        timestamp_arg(seconds, nanos)
    }
}

impl From<ErrorArg> for Arg {
    fn from(arg: ErrorArg) -> Self {
        match arg {
            ErrorArg::String(text) => Self::String(text),
            ErrorArg::Int64(n) => Self::Int64(n),
            ErrorArg::Double(x) => Self::Double(x),
            ErrorArg::Timestamp { seconds, nanos } => {
                timestamp_arg(seconds, u32::try_from(nanos).unwrap_or(u32::MAX))
            }
        }
    }
}

/// Why a page of a list could not be delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageKind {
    /// The server could not read the cursor.
    CursorInvalid,
    /// The cursor is authentic but older than the server accepts.
    CursorExpired,
    /// The cursor was issued for a different query.
    CursorQueryMismatch,
    /// Any other failure of the call.
    Rpc,
}

impl PageKind {
    /// The `kind` of the error JSON's `page` member.
    #[must_use]
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::CursorInvalid => "cursor_invalid",
            Self::CursorExpired => "cursor_expired",
            Self::CursorQueryMismatch => "cursor_query_mismatch",
            Self::Rpc => "rpc",
        }
    }
}

/// What a list had delivered when it failed, and why it stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageDetail {
    /// Why the page was not delivered.
    pub kind: PageKind,
    /// Pages handed over before the failure.
    pub delivered_pages: u32,
    /// Items handed over before the failure.
    pub delivered_items: u64,
}

/// A failed call.
#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    /// The emitted error code, such as `GRANT_MISSING`; the platform code when
    /// the server named none.
    pub code: String,
    /// The emitting deployable; empty when the server stamped none.
    pub emitter: String,
    /// The platform `ErrorCode`, such as `FORBIDDEN`.
    pub platform: String,
    /// The Connect code of the refusal.
    pub connect_code: ConnectCode,
    /// The diagnostic text, for logs.
    pub message: String,
    /// The message arguments, keyed by placeholder name. Boxed, like `page`,
    /// so a `Result` carrying an `Error` stays small.
    pub args: Box<BTreeMap<String, Arg>>,
    /// Present only when a list failed.
    pub page: Option<Box<PageDetail>>,
}

impl Error {
    /// A refusal the core makes itself: emitter `bones`, no arguments.
    #[must_use]
    pub fn local(
        connect_code: ConnectCode,
        code: &str,
        platform: &str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.to_owned(),
            emitter: EMITTER.to_owned(),
            platform: platform.to_owned(),
            connect_code,
            message: message.into(),
            args: Box::default(),
            page: None,
        }
    }

    /// An error an emitter declared: its `code`, its `platform` code, and the
    /// arguments of its message. The Connect code is the one `platform` maps
    /// from.
    #[must_use]
    pub fn emitted(
        emitter: &str,
        code: &str,
        platform: &str,
        message: impl Into<String>,
        args: BTreeMap<String, Arg>,
    ) -> Self {
        Self {
            code: code.to_owned(),
            emitter: emitter.to_owned(),
            platform: platform.to_owned(),
            connect_code: connect_of_platform(platform),
            message: message.into(),
            args: Box::new(args),
            page: None,
        }
    }

    /// A handle's `next()` overlapped another `next()`.
    #[must_use]
    pub fn handle_busy() -> Self {
        Self::local(
            ConnectCode::FailedPrecondition,
            HANDLE_BUSY,
            "CONFLICT",
            "a next() call is already pending on this handle",
        )
    }

    /// The call did not finish within its call budget.
    #[must_use]
    pub fn call_budget_elapsed() -> Self {
        Self::local_mapped(
            ConnectCode::DeadlineExceeded,
            CALL_BUDGET_ELAPSED,
            "the call did not finish within its call budget",
        )
    }

    /// The call was cancelled, aborted or dropped.
    #[must_use]
    pub fn canceled() -> Self {
        Self::local_mapped(ConnectCode::Canceled, CANCELED, "the call was canceled")
    }

    /// The caller's identity changed while the call was in flight.
    #[must_use]
    pub fn identity_changed() -> Self {
        Self::local_mapped(
            ConnectCode::Unauthenticated,
            IDENTITY_CHANGED,
            "the caller identity changed during the call",
        )
    }

    /// A value the caller passed does not fit its wire form.
    #[must_use]
    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::local_mapped(
            ConnectCode::InvalidArgument,
            platform_of_connect(ConnectCode::InvalidArgument),
            message,
        )
    }

    /// A failure of the core itself, such as a response it cannot encode.
    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::local_mapped(
            ConnectCode::Internal,
            platform_of_connect(ConnectCode::Internal),
            message,
        )
    }

    fn local_mapped(connect_code: ConnectCode, code: &str, message: impl Into<String>) -> Self {
        Self::local(
            connect_code,
            code,
            platform_of_connect(connect_code),
            message,
        )
    }

    /// The same error, as the failure of a list that had delivered `pages`
    /// pages and `items` items.
    #[must_use]
    pub fn on_page(mut self, kind: PageKind, pages: u32, items: u64) -> Self {
        self.page = Some(Box::new(PageDetail {
            kind,
            delivered_pages: pages,
            delivered_items: items,
        }));
        self
    }

    /// The `error` of a span that ended in this failure: `<connectCode> <code>`.
    #[must_use]
    pub fn span_error(&self) -> String {
        format!("{} {}", self.connect_code.as_wire(), self.code)
    }

    /// The `api-bones.error/v1` JSON value.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let args: Map<String, Value> = self
            .args
            .iter()
            .map(|(name, arg)| (name.clone(), arg.to_value()))
            .collect();
        let mut wire = json!({
            "code": self.code,
            "emitter": self.emitter,
            "platform": self.platform,
            "connectCode": self.connect_code.as_wire(),
            "message": self.message,
            "args": args,
        });
        if let Some(page) = &self.page {
            wire["page"] = json!({
                "kind": page.kind.wire_name(),
                "deliveredPages": page.delivered_pages,
                "deliveredItems": page.delivered_items,
            });
        }
        wire
    }

    /// The `api-bones.error/v1` JSON text.
    #[must_use]
    pub fn to_json(&self) -> String {
        self.to_value().to_string()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.span_error(), self.message)
    }
}

impl std::error::Error for Error {}

/// An argument the caller passed that does not fit its wire form.
impl From<BadValue> for Error {
    fn from(bad: BadValue) -> Self {
        Self::invalid_argument(bad.message())
    }
}

impl From<ConnectError> for Error {
    fn from(error: ConnectError) -> Self {
        let connect_code = ConnectCode::from(error.code);
        let platform = platform_of_connect(connect_code);
        let (code, emitter, args) = match error_info(&error) {
            Some(info) => (
                if info.code.is_empty() {
                    platform.to_owned()
                } else {
                    info.code
                },
                info.emitter,
                info.args
                    .into_iter()
                    .map(|(name, arg)| (name, Arg::from(arg)))
                    .collect(),
            ),
            None => (platform.to_owned(), String::new(), BTreeMap::new()),
        };
        Self {
            code,
            emitter,
            platform: platform.to_owned(),
            connect_code,
            message: error.message.unwrap_or_default(),
            args: Box::new(args),
            page: None,
        }
    }
}

impl From<PageError> for Error {
    fn from(error: PageError) -> Self {
        let (pages, items) = (error.delivered_pages, error.delivered_items);
        let cursor = |code: &str, message: &str| {
            Self::local_mapped(ConnectCode::InvalidArgument, code, message)
        };
        let (failure, kind) = match error.kind {
            PageErrorKind::Rpc(rpc) => (Self::from(rpc), PageKind::Rpc),
            PageErrorKind::CursorExpired => (
                cursor(reason::CURSOR_EXPIRED, "the list cursor expired"),
                PageKind::CursorExpired,
            ),
            PageErrorKind::CursorQueryMismatch => (
                cursor(
                    reason::CURSOR_QUERY_MISMATCH,
                    "the list cursor belongs to a different query",
                ),
                PageKind::CursorQueryMismatch,
            ),
            _ => (
                cursor(reason::CURSOR_INVALID, "the list cursor is invalid"),
                PageKind::CursorInvalid,
            ),
        };
        failure.on_page(kind, pages, items)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use api_bones::connect::{ErrorInfo, with_error_info};
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn every_connect_code_maps_to_its_platform_code() {
        let rows = [
            (ConnectCode::Canceled, "INTERNAL_SERVER_ERROR"),
            (ConnectCode::Unknown, "INTERNAL_SERVER_ERROR"),
            (ConnectCode::InvalidArgument, "VALIDATION_FAILED"),
            (ConnectCode::DeadlineExceeded, "GATEWAY_TIMEOUT"),
            (ConnectCode::NotFound, "RESOURCE_NOT_FOUND"),
            (ConnectCode::AlreadyExists, "RESOURCE_ALREADY_EXISTS"),
            (ConnectCode::PermissionDenied, "FORBIDDEN"),
            (ConnectCode::ResourceExhausted, "RATE_LIMITED"),
            (ConnectCode::FailedPrecondition, "PRECONDITION_FAILED"),
            (ConnectCode::Aborted, "CONFLICT"),
            (ConnectCode::OutOfRange, "UNPROCESSABLE_ENTITY"),
            (ConnectCode::Unimplemented, "NOT_IMPLEMENTED"),
            (ConnectCode::Internal, "INTERNAL_SERVER_ERROR"),
            (ConnectCode::Unavailable, "SERVICE_UNAVAILABLE"),
            (ConnectCode::DataLoss, "INTERNAL_SERVER_ERROR"),
            (ConnectCode::Unauthenticated, "UNAUTHORIZED"),
        ];
        assert_eq!(rows.len(), CODES.len());
        for (code, platform) in rows {
            assert_eq!(platform_of_connect(code), platform, "{code:?}");
            assert!(CODES.contains(&code), "{code:?}");
        }
    }

    #[test]
    fn a_platform_code_maps_back_to_a_connect_code() {
        for (platform, code) in [
            ("UNAUTHORIZED", ConnectCode::Unauthenticated),
            ("FORBIDDEN", ConnectCode::PermissionDenied),
            ("RESOURCE_NOT_FOUND", ConnectCode::NotFound),
            ("RESOURCE_ALREADY_EXISTS", ConnectCode::AlreadyExists),
            ("RATE_LIMITED", ConnectCode::ResourceExhausted),
            ("PRECONDITION_FAILED", ConnectCode::FailedPrecondition),
            ("VALIDATION_FAILED", ConnectCode::InvalidArgument),
            ("SERVICE_UNAVAILABLE", ConnectCode::Unavailable),
            ("GATEWAY_TIMEOUT", ConnectCode::DeadlineExceeded),
            ("NOT_IMPLEMENTED", ConnectCode::Unimplemented),
            ("CONFLICT", ConnectCode::Aborted),
            ("UNPROCESSABLE_ENTITY", ConnectCode::OutOfRange),
            ("INTERNAL_SERVER_ERROR", ConnectCode::Internal),
            ("NOT_A_PLATFORM_CODE", ConnectCode::Unknown),
        ] {
            assert_eq!(connect_of_platform(platform), code, "{platform}");
        }
    }

    #[test]
    fn an_argument_crosses_tagged_by_its_kind() {
        assert_eq!(
            Arg::from("x").to_value(),
            json!({"kind": "string", "value": "x"})
        );
        assert_eq!(Arg::from("x".to_owned()), Arg::String("x".to_owned()));
        assert_eq!(
            Arg::from(i64::MAX).to_value(),
            json!({"kind": "int64", "value": "9223372036854775807"})
        );
        assert_eq!(
            Arg::from(1.5).to_value(),
            json!({"kind": "double", "value": 1.5})
        );
        for (x, word) in [
            (f64::NAN, "NaN"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
        ] {
            assert_eq!(
                Arg::Double(x).to_value(),
                json!({"kind": "double", "value": word})
            );
        }
        assert_eq!(
            Arg::from(UNIX_EPOCH + Duration::new(1_000_000_000, 500_000_000)).to_value(),
            json!({"kind": "timestamp", "value": "2001-09-09T01:46:40.5Z"})
        );
    }

    #[test]
    fn a_timestamp_outside_the_rfc_3339_range_stays_as_text() {
        let far = UNIX_EPOCH + Duration::from_secs(300_000_000_000);
        assert_eq!(
            Arg::from(far),
            Arg::String("300000000000.000000000".to_owned())
        );
        assert_eq!(
            Arg::from(ErrorArg::Timestamp {
                seconds: 0,
                nanos: -1
            }),
            Arg::String(format!("0.{:09}", u32::MAX))
        );
    }

    #[test]
    fn an_api_bones_argument_converts_to_its_kind() {
        assert_eq!(Arg::from(ErrorArg::from("a")), Arg::String("a".to_owned()));
        assert_eq!(Arg::from(ErrorArg::Int64(-4)), Arg::Int64(-4));
        assert_eq!(Arg::from(ErrorArg::Double(2.5)), Arg::Double(2.5));
        assert_eq!(
            Arg::from(ErrorArg::Timestamp {
                seconds: 86_400,
                nanos: 0
            }),
            Arg::Timestamp("1970-01-02T00:00:00Z".to_owned())
        );
    }

    #[test]
    fn a_local_refusal_names_the_bones_emitter_and_its_platform_code() {
        let rows = [
            (
                Error::handle_busy(),
                "failed_precondition",
                HANDLE_BUSY,
                "CONFLICT",
            ),
            (
                Error::call_budget_elapsed(),
                "deadline_exceeded",
                CALL_BUDGET_ELAPSED,
                "GATEWAY_TIMEOUT",
            ),
            (
                Error::canceled(),
                "canceled",
                CANCELED,
                "INTERNAL_SERVER_ERROR",
            ),
            (
                Error::identity_changed(),
                "unauthenticated",
                IDENTITY_CHANGED,
                "UNAUTHORIZED",
            ),
            (
                Error::invalid_argument("bad"),
                "invalid_argument",
                "VALIDATION_FAILED",
                "VALIDATION_FAILED",
            ),
            (
                Error::internal("bad"),
                "internal",
                "INTERNAL_SERVER_ERROR",
                "INTERNAL_SERVER_ERROR",
            ),
        ];
        for (error, connect, code, platform) in rows {
            let wire = error.to_value();
            assert_eq!(wire["connectCode"], connect);
            assert_eq!(wire["code"], code);
            assert_eq!(wire["platform"], platform);
            assert_eq!(wire["emitter"], "bones");
            assert_eq!(wire["args"], json!({}));
            assert!(wire.get("page").is_none());
        }
    }

    #[test]
    fn a_value_that_does_not_fit_its_wire_form_is_an_invalid_argument() {
        let error = Error::from(BadValue::new("id: expected a string, found null"));
        assert_eq!(error.connect_code, ConnectCode::InvalidArgument);
        assert_eq!(error.message, "id: expected a string, found null");
    }

    #[test]
    fn the_json_text_carries_the_wire_fields_and_the_diagnostic() {
        let error = Error::invalid_argument("id: expected a string, found null");
        let wire: Value = serde_json::from_str(&error.to_json()).unwrap();
        assert_eq!(wire["message"], "id: expected a string, found null");
        assert_eq!(
            error.to_string(),
            "invalid_argument VALIDATION_FAILED: id: expected a string, found null"
        );
        assert_eq!(error.span_error(), "invalid_argument VALIDATION_FAILED");
        let as_error: &dyn std::error::Error = &error;
        assert!(as_error.source().is_none());
    }

    #[test]
    fn an_emitted_error_carries_its_arguments_and_the_connect_code_of_its_platform() {
        let mut args = BTreeMap::new();
        args.insert("limit".to_owned(), Arg::from(3));
        args.insert("who".to_owned(), Arg::from("ann"));
        let error = Error::emitted("pay", "GRANT_MISSING", "FORBIDDEN", "no grant", args);
        assert_eq!(error.connect_code, ConnectCode::PermissionDenied);
        assert_eq!(
            error.to_value(),
            json!({
                "code": "GRANT_MISSING",
                "emitter": "pay",
                "platform": "FORBIDDEN",
                "connectCode": "permission_denied",
                "message": "no grant",
                "args": {
                    "limit": {"kind": "int64", "value": "3"},
                    "who": {"kind": "string", "value": "ann"},
                },
            })
        );
    }

    #[test]
    fn a_server_error_without_error_info_reports_its_platform_code() {
        let error = Error::from(ConnectError::permission_denied("no"));
        assert_eq!(error.code, "FORBIDDEN");
        assert_eq!(error.emitter, "");
        assert_eq!(error.platform, "FORBIDDEN");
        assert_eq!(error.connect_code, ConnectCode::PermissionDenied);
        assert_eq!(error.message, "no");
        assert!(error.args.is_empty());
        assert!(error.page.is_none());
        assert_eq!(
            Error::from(ConnectError::new(connectrpc::ErrorCode::Unavailable, "x")).message,
            "x"
        );
    }

    #[test]
    fn a_server_error_without_a_message_has_an_empty_one() {
        let mut bare = ConnectError::internal("x");
        bare.message = None;
        assert_eq!(Error::from(bare).message, "");
    }

    #[test]
    fn a_server_error_with_error_info_reports_its_reason_emitter_and_arguments() {
        let mut info = ErrorInfo::from_code("GRANT_MISSING");
        info.emitter = "pay".to_owned();
        info.args.insert("who".to_owned(), ErrorArg::from("ann"));
        info.args.insert("n".to_owned(), ErrorArg::Int64(2));
        let error = Error::from(with_error_info(
            ConnectError::permission_denied("no"),
            &info,
        ));
        assert_eq!(error.code, "GRANT_MISSING");
        assert_eq!(error.emitter, "pay");
        assert_eq!(error.platform, "FORBIDDEN");
        assert_eq!(error.connect_code, ConnectCode::PermissionDenied);
        assert_eq!(error.args["who"], Arg::from("ann"));
        assert_eq!(error.args["n"], Arg::Int64(2));
    }

    #[test]
    fn error_info_without_a_code_falls_back_to_the_platform_code() {
        let info = ErrorInfo::default();
        let error = Error::from(with_error_info(ConnectError::not_found("x"), &info));
        assert_eq!(error.code, "RESOURCE_NOT_FOUND");
    }

    fn page_error(kind: PageErrorKind) -> PageError {
        PageError {
            kind,
            delivered_pages: 2,
            delivered_items: 25,
        }
    }

    #[test]
    fn a_cursor_refusal_names_its_page_kind_and_what_was_delivered() {
        for (kind, page_kind, code, wire_kind) in [
            (
                PageErrorKind::CursorInvalid,
                PageKind::CursorInvalid,
                "CURSOR_INVALID",
                "cursor_invalid",
            ),
            (
                PageErrorKind::CursorExpired,
                PageKind::CursorExpired,
                "CURSOR_EXPIRED",
                "cursor_expired",
            ),
            (
                PageErrorKind::CursorQueryMismatch,
                PageKind::CursorQueryMismatch,
                "CURSOR_QUERY_MISMATCH",
                "cursor_query_mismatch",
            ),
        ] {
            let error = Error::from(page_error(kind));
            assert_eq!(error.code, code);
            assert_eq!(error.emitter, "bones");
            assert_eq!(error.connect_code, ConnectCode::InvalidArgument);
            assert_eq!(
                error.page,
                Some(Box::new(PageDetail {
                    kind: page_kind,
                    delivered_pages: 2,
                    delivered_items: 25
                }))
            );
            assert_eq!(
                error.to_value()["page"],
                json!({"kind": wire_kind, "deliveredPages": 2, "deliveredItems": 25})
            );
        }
    }

    #[test]
    fn an_rpc_failure_of_a_list_keeps_the_rpc_error_and_adds_the_page_detail() {
        let mut info = ErrorInfo::from_code("GRANT_MISSING");
        info.emitter = "pay".to_owned();
        let rpc = with_error_info(ConnectError::permission_denied("no"), &info);
        let error = Error::from(page_error(PageErrorKind::Rpc(rpc)));
        assert_eq!(error.code, "GRANT_MISSING");
        assert_eq!(error.platform, "FORBIDDEN");
        assert_eq!(error.page.as_ref().map(|p| p.kind), Some(PageKind::Rpc));
        assert_eq!(error.to_value()["page"]["kind"], "rpc");
    }
}
