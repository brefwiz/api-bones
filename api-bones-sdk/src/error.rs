// SPDX-License-Identifier: MIT
//! The error of a Layer 2 call.
//!
//! The Rust core sorts every failure into one [`Error`]; a host revives the
//! `bones.sdk.v1.CallError` message it crosses as into its own error class.
//! The fields are `code`, `emitter`, `platform`, the Connect code, `message`,
//! `args` and, when present, the page detail (a list that failed part-way) and
//! the wait a refusal named.
//!
//! A refusal the Layer 2 core makes itself carries the emitter `bones` and no
//! catalog key of its own, so a host renders the text of its platform code.

use std::collections::BTreeMap;
use std::fmt;
use std::time::{Duration, SystemTime};

use api_bones::connect::{ConnectCode, ErrorArg, error_info};
use api_bones_page::{PageError, PageErrorKind, reason};
use buffa_types::google::protobuf::Timestamp;
use connectrpc::ConnectError;

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
/// A wrapper or runtime stamped for another boundary version tried to open a
/// client of this core.
pub const SDK_BOUNDARY_MISMATCH: &str = "SDK_BOUNDARY_MISMATCH";
/// An in-fleet call whose SPIFFE Workload API did not answer.
pub const WORKLOAD_IDENTITY_UNAVAILABLE: &str =
    api_bones_transport::identity::WORKLOAD_IDENTITY_UNAVAILABLE;
/// The caller's SVID names no `/svc/<workload>`, so no peer follows from it.
pub const UNUSABLE_PEER_IDENTITY: &str = api_bones_transport::identity::UNUSABLE_PEER_IDENTITY;
/// The server did not present the identity of the peer the call is for.
pub const PEER_IDENTITY_REFUSED: &str = api_bones_transport::identity::PEER_IDENTITY_REFUSED;
/// An in-fleet call over a scheme other than `https`.
pub const INSECURE_IN_FLEET: &str = api_bones_transport::identity::INSECURE_IN_FLEET;

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
    /// A whole number.
    Int64(i64),
    /// A real number.
    Double(f64),
    /// An instant, as `google.protobuf.Timestamp` carries it.
    Timestamp {
        /// Seconds since the Unix epoch.
        seconds: i64,
        /// Nanoseconds within the second, `0..1_000_000_000`.
        nanos: i32,
    },
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
        let Timestamp { seconds, nanos, .. } = Timestamp::from(time);
        Self::Timestamp { seconds, nanos }
    }
}

impl From<ErrorArg> for Arg {
    fn from(arg: ErrorArg) -> Self {
        match arg {
            ErrorArg::String(text) => Self::String(text),
            ErrorArg::Int64(n) => Self::Int64(n),
            ErrorArg::Double(x) => Self::Double(x),
            ErrorArg::Timestamp { seconds, nanos } => Self::Timestamp { seconds, nanos },
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
    /// The message arguments, keyed by placeholder name. Boxed, like
    /// `details`, so a `Result` carrying an `Error` stays small.
    pub args: Box<BTreeMap<String, Arg>>,
    /// The rarely present members, read through [`Error::page`] and
    /// [`Error::retry_after`]; one box keeps a `Result` carrying an `Error`
    /// small.
    details: Option<Box<Details>>,
}

/// The members a failure carries only sometimes.
#[derive(Debug, Clone, Default, PartialEq)]
struct Details {
    /// Present only when a list failed.
    page: Option<PageDetail>,
    /// The wait a `resource_exhausted` refusal named in its `retry-after`.
    retry_after: Option<Duration>,
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
            details: None,
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
            details: None,
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

    /// A wrapper or runtime speaks another boundary version than the core: the
    /// client is refused before it is built. The arguments name the three
    /// versions, so a message can show which side is stale.
    #[must_use]
    pub fn boundary_mismatch(core: u32, wrapper: u32, runtime: u32) -> Self {
        let mut error = Self::local_mapped(
            ConnectCode::FailedPrecondition,
            SDK_BOUNDARY_MISMATCH,
            format!(
                "the SDK boundary versions do not match: core {core}, wrapper {wrapper}, runtime {runtime}"
            ),
        );
        for (name, version) in [("core", core), ("wrapper", wrapper), ("runtime", runtime)] {
            error
                .args
                .insert(name.to_owned(), Arg::Int64(i64::from(version)));
        }
        error
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
        self.details.get_or_insert_with(Box::default).page = Some(PageDetail {
            kind,
            delivered_pages: pages,
            delivered_items: items,
        });
        self
    }

    /// What a list had delivered when it failed; present only when a list
    /// failed.
    #[must_use]
    pub fn page(&self) -> Option<PageDetail> {
        self.details.as_ref().and_then(|details| details.page)
    }

    /// The wait a `resource_exhausted` refusal named in its `retry-after`,
    /// whether the call handed it back because the wait was too long to sit
    /// out or because its attempts were spent.
    #[must_use]
    pub fn retry_after(&self) -> Option<Duration> {
        self.details
            .as_ref()
            .and_then(|details| details.retry_after)
    }

    /// The `error` of a span that ended in this failure: `<connectCode> <code>`.
    #[must_use]
    pub fn span_error(&self) -> String {
        format!("{} {}", self.connect_code.as_wire(), self.code)
    }
}

/// The refusal of a call the host cannot make unpredictable: raised locally, so
/// it names no emitter, and not retryable.
pub(crate) fn no_random_source() -> ConnectError {
    ConnectError::failed_precondition(api_bones_rt::NoRandomSource.to_string())
}

/// The wait a throttle named, read by the shared `retry_after` reader; any
/// other refusal names none.
fn named_wait(error: &ConnectError) -> Option<Duration> {
    if error.code != connectrpc::ErrorCode::ResourceExhausted {
        return None;
    }
    api_bones::connect::retry_after(error)
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.span_error(), self.message)
    }
}

impl std::error::Error for Error {}

impl From<ConnectError> for Error {
    fn from(error: ConnectError) -> Self {
        let connect_code = ConnectCode::from(error.code);
        let platform = platform_of_connect(connect_code);
        let retry_after = named_wait(&error);
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
            details: retry_after.map(|wait| {
                Box::new(Details {
                    page: None,
                    retry_after: Some(wait),
                })
            }),
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

    fn throttle(retry_after: &str) -> ConnectError {
        let mut headers = http::HeaderMap::new();
        headers.insert(
            http::header::RETRY_AFTER,
            http::HeaderValue::from_str(retry_after).unwrap(),
        );
        ConnectError::resource_exhausted("limit reached").with_headers(headers)
    }

    #[test]
    fn a_host_without_randomness_is_refused_as_a_local_failed_precondition() {
        let error = Error::from(no_random_source());
        assert_eq!(error.platform, "PRECONDITION_FAILED");
        assert_eq!(error.connect_code, ConnectCode::FailedPrecondition);
        assert_eq!(error.emitter, "");
        assert_eq!(
            error.message,
            "this host has no secure random source (crypto.getRandomValues)"
        );
        assert_eq!(error.retry_after(), None);
    }

    #[test]
    fn a_throttle_with_a_seconds_retry_after_carries_the_wait_it_names() {
        let error = Error::from(throttle("7"));
        assert_eq!(error.retry_after(), Some(Duration::from_secs(7)));
    }

    #[tokio::test]
    async fn a_throttle_with_an_http_date_retry_after_carries_the_wait_it_names() {
        use api_bones::connect::fetch_with_retry;
        // Far enough ahead to be handed back unwaited; the loop rewrites the
        // date as whole seconds.
        let handed_back = fetch_with_retry(
            || async { Err::<(), _>(throttle("Fri, 01 Jan 2100 00:00:00 GMT")) },
            0,
            None,
        )
        .await
        .unwrap_err();
        let error = Error::from(handed_back);
        let wait = error.retry_after().expect("the wait the date names");
        assert!(wait > Duration::from_secs(2_000_000_000), "{wait:?}");
    }

    #[test]
    fn a_refusal_that_is_no_throttle_has_no_retry_after() {
        let mut headers = http::HeaderMap::new();
        headers.insert(
            http::header::RETRY_AFTER,
            http::HeaderValue::from_static("7"),
        );
        // A retry-after on another code is not a throttle.
        let other = ConnectError::unavailable("down").with_headers(headers);
        assert_eq!(Error::from(other).retry_after(), None);
        // A throttle that names no wait, or an unreadable one.
        let bare = Error::from(ConnectError::resource_exhausted("limit reached"));
        assert_eq!(bare.retry_after(), None);
        assert_eq!(Error::from(throttle("soon")).retry_after(), None);
    }

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
    fn an_argument_is_made_from_its_value() {
        assert_eq!(Arg::from("x"), Arg::String("x".to_owned()));
        assert_eq!(Arg::from("x".to_owned()), Arg::String("x".to_owned()));
        assert_eq!(Arg::from(i64::MAX), Arg::Int64(i64::MAX));
        assert_eq!(Arg::from(1.5), Arg::Double(1.5));
        assert_eq!(
            Arg::from(UNIX_EPOCH + Duration::new(1_000_000_000, 500_000_000)),
            Arg::Timestamp {
                seconds: 1_000_000_000,
                nanos: 500_000_000
            }
        );
    }

    #[test]
    fn an_instant_before_the_epoch_has_a_negative_second_and_a_positive_fraction() {
        assert_eq!(
            Arg::from(UNIX_EPOCH - Duration::new(1, 500_000_000)),
            Arg::Timestamp {
                seconds: -2,
                nanos: 500_000_000
            }
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
            Arg::Timestamp {
                seconds: 86_400,
                nanos: 0
            }
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
                Error::boundary_mismatch(1, 2, 3),
                "failed_precondition",
                SDK_BOUNDARY_MISMATCH,
                "PRECONDITION_FAILED",
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
            assert_eq!(error.connect_code.as_wire(), connect);
            assert_eq!(error.code, code);
            assert_eq!(error.platform, platform);
            assert_eq!(error.emitter, "bones");
            assert!(error.page().is_none());
            assert!(error.retry_after().is_none());
        }
    }

    #[test]
    fn a_boundary_mismatch_names_the_three_versions() {
        let error = Error::boundary_mismatch(1, 2, 3);
        assert_eq!(error.args.get("core"), Some(&Arg::Int64(1)));
        assert_eq!(error.args.get("wrapper"), Some(&Arg::Int64(2)));
        assert_eq!(error.args.get("runtime"), Some(&Arg::Int64(3)));
        assert!(error.message.contains("core 1, wrapper 2, runtime 3"));
    }

    #[test]
    fn the_text_of_an_error_names_its_connect_code_its_code_and_the_diagnostic() {
        let error = Error::invalid_argument("id: expected a string, found null");
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
        assert_eq!(error.code, "GRANT_MISSING");
        assert_eq!(error.emitter, "pay");
        assert_eq!(error.platform, "FORBIDDEN");
        assert_eq!(error.message, "no grant");
        assert_eq!(error.args.len(), 2);
        assert_eq!(error.args["limit"], Arg::Int64(3));
        assert_eq!(error.args["who"], Arg::from("ann"));
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
        assert!(error.page().is_none());
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
        for (kind, page_kind, code) in [
            (
                PageErrorKind::CursorInvalid,
                PageKind::CursorInvalid,
                "CURSOR_INVALID",
            ),
            (
                PageErrorKind::CursorExpired,
                PageKind::CursorExpired,
                "CURSOR_EXPIRED",
            ),
            (
                PageErrorKind::CursorQueryMismatch,
                PageKind::CursorQueryMismatch,
                "CURSOR_QUERY_MISMATCH",
            ),
        ] {
            let error = Error::from(page_error(kind));
            assert_eq!(error.code, code);
            assert_eq!(error.emitter, "bones");
            assert_eq!(error.connect_code, ConnectCode::InvalidArgument);
            assert_eq!(
                error.page(),
                Some(PageDetail {
                    kind: page_kind,
                    delivered_pages: 2,
                    delivered_items: 25
                })
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
        assert_eq!(error.page().map(|p| p.kind), Some(PageKind::Rpc));
    }
}
