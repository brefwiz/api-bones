//! Standard API error types for all api-bones services.
//!
//! All services serialize errors into [`ApiError`] before sending an HTTP
//! response. The wire format conforms to
//! [RFC 9457 — Problem Details for HTTP APIs](https://www.rfc-editor.org/rfc/rfc9457):
//!
//! ```json
//! {
//!   "type": "urn:api-bones:error:resource-not-found",
//!   "title": "Resource Not Found",
//!   "status": 404,
//!   "detail": "Booking 123 not found",
//!   "instance": "urn:uuid:01234567-89ab-cdef-0123-456789abcdef"
//! }
//! ```
//!
//! Content-Type: `application/problem+json`
//!
//! # `no_std` + `alloc` limitations
//!
//! [`ErrorTypeMode`] is available under the `alloc` feature (its fields use
//! `String`), but the global accessors [`error_type_mode`] and
//! [`set_error_type_mode`] require the `std` feature because they rely on
//! [`std::sync::RwLock`] and [`std::env::var`].
//!
//! In a `no_std + alloc` environment you can still construct an
//! [`ErrorTypeMode`] value and call [`ErrorTypeMode::render`] directly, but
//! the automatic environment-variable resolution is unavailable.

#[cfg(all(not(feature = "std"), feature = "alloc", feature = "serde"))]
use alloc::collections::BTreeMap;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{borrow::ToOwned, format, string::String, string::ToString, sync::Arc, vec::Vec};
use core::fmt;
#[cfg(feature = "std")]
use std::{collections::BTreeMap, sync::Arc};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Error code
// ---------------------------------------------------------------------------

/// Machine-readable error code included in every API error response.
///
/// Serializes as a URN per [RFC 9457 §3.1.1](https://www.rfc-editor.org/rfc/rfc9457#section-3.1.1),
/// which requires the `type` member to be a URI reference.
/// Format: `urn:api-bones:error:<slug>` (e.g. `urn:api-bones:error:resource-not-found`).
///
/// # Examples
///
/// ```rust
/// use api_bones::error::ErrorCode;
///
/// let code = ErrorCode::ResourceNotFound;
/// assert_eq!(code.status_code(), 404);
/// assert_eq!(code.title(), "Resource Not Found");
/// assert_eq!(code.urn_slug(), "resource-not-found");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "proptest", derive(proptest_derive::Arbitrary))]
pub enum ErrorCode {
    // 400
    BadRequest,
    ValidationFailed,
    // 401
    Unauthorized,
    InvalidCredentials,
    TokenExpired,
    TokenInvalid,
    // 403
    Forbidden,
    InsufficientPermissions,
    // 404
    ResourceNotFound,
    // 405
    MethodNotAllowed,
    // 406
    NotAcceptable,
    // 408
    RequestTimeout,
    // 409
    Conflict,
    ResourceAlreadyExists,
    // 410
    Gone,
    // 412
    PreconditionFailed,
    // 413
    PayloadTooLarge,
    // 415
    UnsupportedMediaType,
    // 422
    UnprocessableEntity,
    // 428
    PreconditionRequired,
    // 429
    RateLimited,
    // 431
    RequestHeaderFieldsTooLarge,
    // 500
    InternalServerError,
    // 501
    NotImplemented,
    // 502
    BadGateway,
    // 503
    ServiceUnavailable,
    // 504
    GatewayTimeout,
}

/// How the RFC 9457 `type` field is rendered for [`ErrorCode`].
///
/// RFC 9457 §3.1.1 requires `type` to be a URI reference and encourages using
/// resolvable URLs so consumers can look up documentation. This enum lets you
/// choose the format that fits your deployment.
///
/// Requires `std` or `alloc` (fields contain `String`).
///
/// # `no_std` note
///
/// This type is available with `alloc` alone, but the global accessors
/// [`error_type_mode`] and [`set_error_type_mode`] require the `std` feature
/// (`RwLock` + env-var access). In a `no_std + alloc` context, construct the
/// variant you need and call [`ErrorTypeMode::render`] directly.
///
/// # Configuration
///
/// Set the mode once at startup via [`set_error_type_mode`], or let it
/// auto-resolve from environment variables (see [`error_type_mode`]).
///
/// ## URL mode (recommended)
///
/// Produces `{base_url}/{slug}`, e.g.:
/// `https://docs.myapp.com/errors/resource-not-found`
///
/// Set via env: `SHARED_TYPES_ERROR_TYPE_BASE_URL=https://docs.myapp.com/errors`
///
/// ## URN mode (fallback)
///
/// Produces `urn:{namespace}:error:{slug}`, e.g.:
/// `urn:myapp:error:resource-not-found`
///
/// Set via env: `SHARED_TYPES_URN_NAMESPACE=myapp`
///
/// # Examples
///
/// ```rust
/// use api_bones::error::ErrorTypeMode;
///
/// let url_mode = ErrorTypeMode::Url { base_url: "https://docs.example.com/errors".into() };
/// assert_eq!(url_mode.render("not-found"), "https://docs.example.com/errors/not-found");
///
/// let urn_mode = ErrorTypeMode::Urn { namespace: "myapp".into() };
/// assert_eq!(urn_mode.render("not-found"), "urn:myapp:error:not-found");
/// ```
#[cfg(any(feature = "std", feature = "alloc"))]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "proptest", derive(proptest_derive::Arbitrary))]
pub enum ErrorTypeMode {
    /// Generate a resolvable URL per RFC 9457 §3.1.1 (recommended).
    /// Format: `{base_url}/{slug}` — trailing slash in `base_url` is trimmed automatically.
    Url {
        /// Base URL for error documentation, e.g. `https://docs.myapp.com/errors`.
        base_url: String,
    },
    /// Generate a URN per RFC 9457 §3.1.1 + RFC 8141.
    /// Format: `urn:{namespace}:error:{slug}`.
    Urn {
        /// URN namespace, e.g. `"myapp"`.
        namespace: String,
    },
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl ErrorTypeMode {
    /// Render the full `type` URI for a given error slug.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::error::ErrorTypeMode;
    ///
    /// let mode = ErrorTypeMode::Url { base_url: "https://example.com/errors/".into() };
    /// assert_eq!(mode.render("bad-request"), "https://example.com/errors/bad-request");
    ///
    /// let mode = ErrorTypeMode::Urn { namespace: "acme".into() };
    /// assert_eq!(mode.render("bad-request"), "urn:acme:error:bad-request");
    /// ```
    #[must_use]
    pub fn render(&self, slug: &str) -> String {
        match self {
            Self::Url { base_url } => format!("{}/{slug}", base_url.trim_end_matches('/')),
            Self::Urn { namespace } => format!("urn:{namespace}:error:{slug}"),
        }
    }
}

/// Returns the active [`ErrorTypeMode`].
///
/// Resolution order (first wins):
/// 1. Value set via [`set_error_type_mode`] (programmatic, highest priority)
/// 2. Compile-time `SHARED_TYPES_ERROR_TYPE_BASE_URL` → [`ErrorTypeMode::Url`]
/// 3. Runtime `SHARED_TYPES_ERROR_TYPE_BASE_URL` → [`ErrorTypeMode::Url`]
/// 4. Compile-time `SHARED_TYPES_URN_NAMESPACE` → [`ErrorTypeMode::Urn`]
/// 5. Runtime `SHARED_TYPES_URN_NAMESPACE` → [`ErrorTypeMode::Urn`]
/// 6. Default: `ErrorTypeMode::Urn { namespace: "api-bones".into() }`
///
/// Requires the `std` feature (`RwLock` + environment variable access).
#[cfg(feature = "std")]
static ERROR_TYPE_MODE: std::sync::RwLock<Option<ErrorTypeMode>> = std::sync::RwLock::new(None);

/// Resolve the mode from environment variables and compile-time settings.
#[cfg(feature = "std")]
fn resolve_error_type_mode() -> ErrorTypeMode {
    // 1. Compile-time base URL → URL mode (never set in CI/test; excluded from
    //    coverage instrumentation to avoid false "missed function" reports)
    #[cfg(not(coverage))]
    if let Some(url) = option_env!("SHARED_TYPES_ERROR_TYPE_BASE_URL")
        && !url.is_empty()
    {
        return ErrorTypeMode::Url {
            base_url: url.to_owned(),
        };
    }
    // 2. Runtime base URL → URL mode
    if let Ok(url) = std::env::var("SHARED_TYPES_ERROR_TYPE_BASE_URL")
        && !url.is_empty()
    {
        return ErrorTypeMode::Url { base_url: url };
    }
    // 3. Compile-time URN namespace → URN mode (same rationale as #1)
    #[cfg(not(coverage))]
    if let Some(ns) = option_env!("SHARED_TYPES_URN_NAMESPACE")
        && !ns.is_empty()
    {
        return ErrorTypeMode::Urn {
            namespace: ns.to_owned(),
        };
    }
    // 4. Runtime URN namespace → URN mode
    if let Ok(ns) = std::env::var("SHARED_TYPES_URN_NAMESPACE")
        && !ns.is_empty()
    {
        return ErrorTypeMode::Urn { namespace: ns };
    }
    // 5. Default
    ErrorTypeMode::Urn {
        namespace: "api-bones".to_owned(),
    }
}

/// Returns the active [`ErrorTypeMode`].
///
/// Resolution order:
///
/// 1. Value set via [`set_error_type_mode`] (programmatic, highest priority)
/// 2. Compile-time `SHARED_TYPES_ERROR_TYPE_BASE_URL` → [`ErrorTypeMode::Url`]
/// 3. Runtime `SHARED_TYPES_ERROR_TYPE_BASE_URL` → [`ErrorTypeMode::Url`]
/// 4. Compile-time `SHARED_TYPES_URN_NAMESPACE` → [`ErrorTypeMode::Urn`]
/// 5. Runtime `SHARED_TYPES_URN_NAMESPACE` → [`ErrorTypeMode::Urn`]
/// 6. Default: `ErrorTypeMode::Urn { namespace: "api-bones".into() }`
///
/// Requires the `std` feature.
///
/// # Examples
///
/// ```rust
/// use api_bones::error::{error_type_mode, set_error_type_mode, ErrorTypeMode};
///
/// set_error_type_mode(ErrorTypeMode::Urn { namespace: "demo".into() });
/// let mode = error_type_mode();
/// assert_eq!(mode, ErrorTypeMode::Urn { namespace: "demo".into() });
/// ```
#[cfg(feature = "std")]
#[must_use]
pub fn error_type_mode() -> ErrorTypeMode {
    {
        let guard = ERROR_TYPE_MODE
            .read()
            .expect("error type mode lock poisoned");
        if let Some(mode) = guard.as_ref() {
            return mode.clone();
        }
    }
    // Not yet initialised — resolve and store.
    let mut guard = ERROR_TYPE_MODE
        .write()
        .expect("error type mode lock poisoned");
    // Double-check after acquiring write lock.
    if let Some(mode) = guard.as_ref() {
        return mode.clone();
    }
    let mode = resolve_error_type_mode();
    *guard = Some(mode.clone());
    mode
}

/// Override the error type mode programmatically (call once at application startup).
///
/// Unlike the previous `OnceLock`-based implementation, this will overwrite any
/// previously set or auto-resolved mode.
///
/// Requires the `std` feature.
///
/// # Example
/// ```rust
/// use api_bones::error::{set_error_type_mode, ErrorTypeMode};
///
/// set_error_type_mode(ErrorTypeMode::Url {
///     base_url: "https://docs.myapp.com/errors".into(),
/// });
/// ```
#[cfg(feature = "std")]
pub fn set_error_type_mode(mode: ErrorTypeMode) {
    let mut guard = ERROR_TYPE_MODE
        .write()
        .expect("error type mode lock poisoned");
    *guard = Some(mode);
}

/// Reset the error type mode to uninitialized so the next call to
/// [`error_type_mode`] re-resolves from environment variables.
///
/// Only available in test builds.
#[cfg(all(test, feature = "std"))]
pub(crate) fn reset_error_type_mode() {
    let mut guard = ERROR_TYPE_MODE
        .write()
        .expect("error type mode lock poisoned");
    *guard = None;
}

/// Returns the active URN namespace (convenience wrapper around [`error_type_mode`]).
/// Only meaningful when in [`ErrorTypeMode::Urn`] mode.
///
/// Requires the `std` feature.
///
/// # Examples
///
/// ```rust
/// use api_bones::error::{urn_namespace, set_error_type_mode, ErrorTypeMode};
///
/// set_error_type_mode(ErrorTypeMode::Urn { namespace: "myapp".into() });
/// assert_eq!(urn_namespace(), "myapp");
/// ```
#[cfg(feature = "std")]
#[must_use]
pub fn urn_namespace() -> String {
    match error_type_mode() {
        ErrorTypeMode::Urn { namespace } => namespace,
        ErrorTypeMode::Url { .. } => "api-bones".to_owned(),
    }
}

impl ErrorCode {
    /// HTTP status code for this error code.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::error::ErrorCode;
    ///
    /// assert_eq!(ErrorCode::BadRequest.status_code(), 400);
    /// assert_eq!(ErrorCode::Unauthorized.status_code(), 401);
    /// assert_eq!(ErrorCode::InternalServerError.status_code(), 500);
    /// ```
    #[must_use]
    pub fn status_code(&self) -> u16 {
        match self {
            Self::BadRequest | Self::ValidationFailed => 400,
            Self::Unauthorized
            | Self::InvalidCredentials
            | Self::TokenExpired
            | Self::TokenInvalid => 401,
            Self::Forbidden | Self::InsufficientPermissions => 403,
            Self::ResourceNotFound => 404,
            Self::MethodNotAllowed => 405,
            Self::NotAcceptable => 406,
            Self::RequestTimeout => 408,
            Self::Conflict | Self::ResourceAlreadyExists => 409,
            Self::Gone => 410,
            Self::PreconditionFailed => 412,
            Self::PayloadTooLarge => 413,
            Self::UnsupportedMediaType => 415,
            Self::UnprocessableEntity => 422,
            Self::PreconditionRequired => 428,
            Self::RateLimited => 429,
            Self::RequestHeaderFieldsTooLarge => 431,
            Self::InternalServerError => 500,
            Self::NotImplemented => 501,
            Self::BadGateway => 502,
            Self::ServiceUnavailable => 503,
            Self::GatewayTimeout => 504,
        }
    }

    /// Human-friendly title for this error code (RFC 9457 `title` field).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::error::ErrorCode;
    ///
    /// assert_eq!(ErrorCode::ResourceNotFound.title(), "Resource Not Found");
    /// assert_eq!(ErrorCode::BadRequest.title(), "Bad Request");
    /// ```
    #[must_use]
    pub fn title(&self) -> &'static str {
        match self {
            Self::BadRequest => "Bad Request",
            Self::ValidationFailed => "Validation Failed",
            Self::Unauthorized => "Unauthorized",
            Self::InvalidCredentials => "Invalid Credentials",
            Self::TokenExpired => "Token Expired",
            Self::TokenInvalid => "Token Invalid",
            Self::Forbidden => "Forbidden",
            Self::InsufficientPermissions => "Insufficient Permissions",
            Self::ResourceNotFound => "Resource Not Found",
            Self::MethodNotAllowed => "Method Not Allowed",
            Self::NotAcceptable => "Not Acceptable",
            Self::RequestTimeout => "Request Timeout",
            Self::Conflict => "Conflict",
            Self::ResourceAlreadyExists => "Resource Already Exists",
            Self::Gone => "Gone",
            Self::PreconditionFailed => "Precondition Failed",
            Self::PayloadTooLarge => "Payload Too Large",
            Self::UnsupportedMediaType => "Unsupported Media Type",
            Self::UnprocessableEntity => "Unprocessable Entity",
            Self::PreconditionRequired => "Precondition Required",
            Self::RateLimited => "Rate Limited",
            Self::RequestHeaderFieldsTooLarge => "Request Header Fields Too Large",
            Self::InternalServerError => "Internal Server Error",
            Self::NotImplemented => "Not Implemented",
            Self::BadGateway => "Bad Gateway",
            Self::ServiceUnavailable => "Service Unavailable",
            Self::GatewayTimeout => "Gateway Timeout",
        }
    }

    /// The URN slug for this error code (the part after `urn:api-bones:error:`).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::error::ErrorCode;
    ///
    /// assert_eq!(ErrorCode::ResourceNotFound.urn_slug(), "resource-not-found");
    /// assert_eq!(ErrorCode::ValidationFailed.urn_slug(), "validation-failed");
    /// ```
    #[must_use]
    pub fn urn_slug(&self) -> &'static str {
        match self {
            Self::BadRequest => "bad-request",
            Self::ValidationFailed => "validation-failed",
            Self::Unauthorized => "unauthorized",
            Self::InvalidCredentials => "invalid-credentials",
            Self::TokenExpired => "token-expired",
            Self::TokenInvalid => "token-invalid",
            Self::Forbidden => "forbidden",
            Self::InsufficientPermissions => "insufficient-permissions",
            Self::ResourceNotFound => "resource-not-found",
            Self::MethodNotAllowed => "method-not-allowed",
            Self::NotAcceptable => "not-acceptable",
            Self::RequestTimeout => "request-timeout",
            Self::Conflict => "conflict",
            Self::ResourceAlreadyExists => "resource-already-exists",
            Self::Gone => "gone",
            Self::PreconditionFailed => "precondition-failed",
            Self::PayloadTooLarge => "payload-too-large",
            Self::UnsupportedMediaType => "unsupported-media-type",
            Self::UnprocessableEntity => "unprocessable-entity",
            Self::PreconditionRequired => "precondition-required",
            Self::RateLimited => "rate-limited",
            Self::RequestHeaderFieldsTooLarge => "request-header-fields-too-large",
            Self::InternalServerError => "internal-server-error",
            Self::NotImplemented => "not-implemented",
            Self::BadGateway => "bad-gateway",
            Self::ServiceUnavailable => "service-unavailable",
            Self::GatewayTimeout => "gateway-timeout",
        }
    }

    /// Full type URI for this error code per RFC 9457 §3.1.1.
    ///
    /// The format depends on the active [`ErrorTypeMode`] (see [`error_type_mode`]):
    /// - URL mode: `https://docs.myapp.com/errors/resource-not-found`
    /// - URN mode: `urn:myapp:error:resource-not-found`
    ///
    /// Requires the `std` feature (dynamic namespace resolution via [`error_type_mode`]).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::error::{ErrorCode, set_error_type_mode, ErrorTypeMode};
    ///
    /// set_error_type_mode(ErrorTypeMode::Urn { namespace: "test".into() });
    /// assert_eq!(ErrorCode::ResourceNotFound.urn(), "urn:test:error:resource-not-found");
    /// ```
    #[cfg(feature = "std")]
    #[must_use]
    pub fn urn(&self) -> String {
        error_type_mode().render(self.urn_slug())
    }

    /// Parse an `ErrorCode` from a type URI string (URL or URN format).
    ///
    /// Requires the `std` feature (dynamic namespace resolution via [`error_type_mode`]).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::error::{ErrorCode, set_error_type_mode, ErrorTypeMode};
    ///
    /// set_error_type_mode(ErrorTypeMode::Urn { namespace: "test".into() });
    /// let code = ErrorCode::ResourceNotFound;
    /// let uri = code.urn();
    /// assert_eq!(ErrorCode::from_type_uri(&uri), Some(ErrorCode::ResourceNotFound));
    /// ```
    #[cfg(feature = "std")]
    #[must_use]
    pub fn from_type_uri(s: &str) -> Option<Self> {
        // Try to extract slug from the active mode's format first, then fall back
        let slug = match error_type_mode() {
            ErrorTypeMode::Url { base_url } => {
                let prefix = format!("{}/", base_url.trim_end_matches('/'));
                s.strip_prefix(prefix.as_str()).or_else(|| {
                    // Also accept URN format as fallback
                    let urn_prefix = format!("urn:{}:error:", urn_namespace());
                    s.strip_prefix(urn_prefix.as_str())
                })?
            }
            ErrorTypeMode::Urn { namespace } => {
                let prefix = format!("urn:{namespace}:error:");
                s.strip_prefix(prefix.as_str())?
            }
        };
        Some(match slug {
            "bad-request" => Self::BadRequest,
            "validation-failed" => Self::ValidationFailed,
            "unauthorized" => Self::Unauthorized,
            "invalid-credentials" => Self::InvalidCredentials,
            "token-expired" => Self::TokenExpired,
            "token-invalid" => Self::TokenInvalid,
            "forbidden" => Self::Forbidden,
            "insufficient-permissions" => Self::InsufficientPermissions,
            "resource-not-found" => Self::ResourceNotFound,
            "method-not-allowed" => Self::MethodNotAllowed,
            "not-acceptable" => Self::NotAcceptable,
            "request-timeout" => Self::RequestTimeout,
            "conflict" => Self::Conflict,
            "resource-already-exists" => Self::ResourceAlreadyExists,
            "gone" => Self::Gone,
            "precondition-failed" => Self::PreconditionFailed,
            "payload-too-large" => Self::PayloadTooLarge,
            "unsupported-media-type" => Self::UnsupportedMediaType,
            "unprocessable-entity" => Self::UnprocessableEntity,
            "precondition-required" => Self::PreconditionRequired,
            "rate-limited" => Self::RateLimited,
            "request-header-fields-too-large" => Self::RequestHeaderFieldsTooLarge,
            "internal-server-error" => Self::InternalServerError,
            "not-implemented" => Self::NotImplemented,
            "bad-gateway" => Self::BadGateway,
            "service-unavailable" => Self::ServiceUnavailable,
            "gateway-timeout" => Self::GatewayTimeout,
            _ => return None,
        })
    }
}

/// In `std` mode the display resolves through the dynamic [`error_type_mode`].
#[cfg(feature = "std")]
impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.urn())
    }
}

/// In `no_std` mode the display falls back to a fixed `urn:api-bones:error:<slug>` format.
#[cfg(not(feature = "std"))]
impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "urn:api-bones:error:{}", self.urn_slug())
    }
}

#[cfg(all(feature = "serde", feature = "std"))]
impl Serialize for ErrorCode {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.urn())
    }
}

#[cfg(all(feature = "serde", feature = "std"))]
impl<'de> Deserialize<'de> for ErrorCode {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::from_type_uri(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown error type URI: {s}")))
    }
}

#[cfg(feature = "utoipa")]
impl utoipa::PartialSchema for ErrorCode {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        use utoipa::openapi::schema::{ObjectBuilder, SchemaType, Type};
        utoipa::openapi::RefOr::T(utoipa::openapi::schema::Schema::Object(
            ObjectBuilder::new()
                .schema_type(SchemaType::new(Type::String))
                .examples(["urn:api-bones:error:resource-not-found"])
                .build(),
        ))
    }
}

#[cfg(feature = "utoipa")]
impl utoipa::ToSchema for ErrorCode {
    fn name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("ErrorCode")
    }
}

// ---------------------------------------------------------------------------
// TryFrom<u16> / TryFrom<http::StatusCode> for ErrorCode
// ---------------------------------------------------------------------------

/// Attempt to convert an HTTP status code (as `u16`) to its canonical
/// [`ErrorCode`] variant.
///
/// Only 4xx and 5xx codes that have a direct mapping return `Ok`; all other
/// codes (1xx, 2xx, 3xx, or unmapped 4xx/5xx) return `Err(())`.
///
/// # Examples
///
/// ```rust
/// use api_bones::error::ErrorCode;
///
/// assert_eq!(ErrorCode::try_from(404_u16), Ok(ErrorCode::ResourceNotFound));
/// assert_eq!(ErrorCode::try_from(500_u16), Ok(ErrorCode::InternalServerError));
/// assert!(ErrorCode::try_from(200_u16).is_err());
/// assert!(ErrorCode::try_from(301_u16).is_err());
/// ```
impl TryFrom<u16> for ErrorCode {
    type Error = ();

    fn try_from(status: u16) -> Result<Self, Self::Error> {
        match status {
            400 => Ok(Self::BadRequest),
            401 => Ok(Self::Unauthorized),
            403 => Ok(Self::Forbidden),
            404 => Ok(Self::ResourceNotFound),
            405 => Ok(Self::MethodNotAllowed),
            406 => Ok(Self::NotAcceptable),
            408 => Ok(Self::RequestTimeout),
            409 => Ok(Self::Conflict),
            410 => Ok(Self::Gone),
            412 => Ok(Self::PreconditionFailed),
            413 => Ok(Self::PayloadTooLarge),
            415 => Ok(Self::UnsupportedMediaType),
            422 => Ok(Self::UnprocessableEntity),
            428 => Ok(Self::PreconditionRequired),
            429 => Ok(Self::RateLimited),
            431 => Ok(Self::RequestHeaderFieldsTooLarge),
            500 => Ok(Self::InternalServerError),
            501 => Ok(Self::NotImplemented),
            502 => Ok(Self::BadGateway),
            503 => Ok(Self::ServiceUnavailable),
            504 => Ok(Self::GatewayTimeout),
            _ => Err(()),
        }
    }
}

/// Attempt to convert an [`http::StatusCode`] to its canonical [`ErrorCode`]
/// variant.
///
/// Delegates to [`TryFrom<u16>`] for [`ErrorCode`]; see that impl for the
/// full mapping. Non-error status codes (1xx, 2xx, 3xx) and unmapped 4xx/5xx
/// codes return `Err(())`.
///
/// Requires the `http` feature.
///
/// # Examples
///
/// ```rust
/// use api_bones::error::ErrorCode;
/// use http::StatusCode;
///
/// assert_eq!(
///     ErrorCode::try_from(StatusCode::NOT_FOUND),
///     Ok(ErrorCode::ResourceNotFound),
/// );
/// assert!(ErrorCode::try_from(StatusCode::OK).is_err());
/// ```
#[cfg(feature = "http")]
impl TryFrom<http::StatusCode> for ErrorCode {
    type Error = ();

    fn try_from(status: http::StatusCode) -> Result<Self, Self::Error> {
        Self::try_from(status.as_u16())
    }
}

// ---------------------------------------------------------------------------
// Validation error
// ---------------------------------------------------------------------------

/// A single field-level validation error, used in [`ApiError::errors`].
///
/// Carried as a documented extension member alongside the standard
/// [RFC 9457](https://www.rfc-editor.org/rfc/rfc9457) fields.
///
/// Requires `std` or `alloc` (fields contain `String`).
///
/// # Examples
///
/// ```rust
/// use api_bones::error::ValidationError;
///
/// let err = ValidationError {
///     field: "/email".into(),
///     message: "must be a valid email".into(),
///     rule: Some("email".into()),
/// };
/// assert_eq!(err.to_string(), "/email: must be a valid email (rule: email)");
///
/// let err2 = ValidationError {
///     field: "/name".into(),
///     message: "is required".into(),
///     rule: None,
/// };
/// assert_eq!(err2.to_string(), "/name: is required");
/// ```
#[cfg(any(feature = "std", feature = "alloc"))]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "proptest", derive(proptest_derive::Arbitrary))]
pub struct ValidationError {
    /// JSON Pointer to the offending field (e.g. `"/email"`).
    pub field: String,
    /// Human-readable description of what went wrong.
    pub message: String,
    /// Optional machine-readable rule that failed (e.g. `"min_length"`).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub rule: Option<String>,
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.rule {
            Some(rule) => write!(f, "{}: {} (rule: {})", self.field, self.message, rule),
            None => write!(f, "{}: {}", self.field, self.message),
        }
    }
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl core::error::Error for ValidationError {}

// ---------------------------------------------------------------------------
// HttpError trait — blanket From<E: HttpError> for ApiError
// ---------------------------------------------------------------------------

/// Implement this trait on domain error types to get a blanket
/// [`From`] implementation into [`ApiError`] for free.
///
/// # Examples
///
/// ```rust
/// use api_bones::error::{ApiError, ErrorCode, HttpError};
///
/// #[derive(Debug)]
/// struct BookingNotFound(u64);
///
/// impl HttpError for BookingNotFound {
///     fn status_code(&self) -> u16 { 404 }
///     fn error_code(&self) -> ErrorCode { ErrorCode::ResourceNotFound }
///     fn detail(&self) -> String { format!("Booking {} not found", self.0) }
/// }
///
/// let err: ApiError = BookingNotFound(42).into();
/// assert_eq!(err.status, 404);
/// assert_eq!(err.detail, "Booking 42 not found");
/// ```
#[cfg(any(feature = "std", feature = "alloc"))]
pub trait HttpError: core::fmt::Debug {
    /// HTTP status code (e.g. `404`).
    fn status_code(&self) -> u16;
    /// Machine-readable [`ErrorCode`] for this error.
    fn error_code(&self) -> ErrorCode;
    /// Human-readable detail string (RFC 9457 §3.1.4 `detail`).
    fn detail(&self) -> String;
}

/// Blanket conversion: any `HttpError` implementor becomes an [`ApiError`].
///
/// This is a blanket impl over a sealed trait parameter so it does not
/// conflict with other `From` impls on `ApiError`.
#[cfg(any(feature = "std", feature = "alloc"))]
impl<E: HttpError> From<E> for ApiError {
    fn from(e: E) -> Self {
        Self::new(e.error_code(), e.detail())
    }
}

// ---------------------------------------------------------------------------
// API error — RFC 9457 Problem Details
// ---------------------------------------------------------------------------

/// [RFC 9457](https://www.rfc-editor.org/rfc/rfc9457) Problem Details error response.
///
/// Wire format field mapping:
///
/// - `code` → `"type"` — URN per RFC 9457 §3.1.1 (e.g. `urn:api-bones:error:resource-not-found`)
/// - `title` → `"title"` — RFC 9457 §3.1.2
/// - `status` → `"status"` — HTTP status code, RFC 9457 §3.1.3 (valid range: 100–599)
/// - `detail` → `"detail"` — RFC 9457 §3.1.4
/// - `request_id` → `"instance"` — URI per RFC 9457 §3.1.5, as `urn:uuid:<uuid>` per RFC 4122 §3
/// - `errors` → `"errors"` — documented extension for field-level validation errors
///
/// Content-Type must be set to `application/problem+json` by the HTTP layer.
///
/// Requires `std` or `alloc` (fields contain `String`/`Vec`).
///
/// # Examples
///
/// ```rust
/// use api_bones::error::{ApiError, ErrorCode};
///
/// let err = ApiError::new(ErrorCode::ResourceNotFound, "User 42 not found");
/// assert_eq!(err.status, 404);
/// assert_eq!(err.detail, "User 42 not found");
/// ```
#[cfg(any(feature = "std", feature = "alloc"))]
#[derive(Debug, Clone)]
#[cfg_attr(
    all(feature = "std", feature = "serde"),
    derive(Serialize, Deserialize)
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct ApiError {
    /// Machine-readable error URN (RFC 9457 §3.1.1 `type`).
    #[cfg_attr(all(feature = "std", feature = "serde"), serde(rename = "type"))]
    pub code: ErrorCode,
    /// Human-friendly error label (RFC 9457 §3.1.2 `title`).
    pub title: String,
    /// HTTP status code (RFC 9457 §3.1.3 `status`). Valid range: 100–599.
    pub status: u16,
    /// Human-readable error specifics (RFC 9457 §3.1.4 `detail`).
    pub detail: String,
    /// URI identifying this specific occurrence (RFC 9457 §3.1.5 `instance`).
    /// Serialized as `urn:uuid:<uuid>` per RFC 4122 §3.
    #[cfg(feature = "uuid")]
    #[cfg_attr(
        all(feature = "std", feature = "serde"),
        serde(
            rename = "instance",
            default,
            skip_serializing_if = "Option::is_none",
            with = "uuid_urn_option"
        )
    )]
    #[cfg_attr(feature = "schemars", schemars(with = "Option<String>"))]
    pub request_id: Option<uuid::Uuid>,
    /// Structured field-level validation errors (extension). Omitted when empty.
    #[cfg_attr(
        all(feature = "std", feature = "serde"),
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub errors: Vec<ValidationError>,
    /// Structured rate-limit metadata (extension). Present on 429 responses
    /// when built via [`ApiError::rate_limited_with`] or
    /// [`ApiError::with_rate_limit`]. Serialized as the top-level
    /// `rate_limit` member.
    #[cfg_attr(
        all(feature = "std", feature = "serde"),
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    #[cfg_attr(feature = "arbitrary", arbitrary(default))]
    pub rate_limit: Option<crate::ratelimit::RateLimitInfo>,
    /// Upstream error that caused this `ApiError`, if any.
    ///
    /// Not serialized — for in-process error chaining only. Exposed via
    /// [`core::error::Error::source`] so that `anyhow`, `eyre`, and tracing
    /// can walk the full error chain.
    ///
    /// Requires `std` or `alloc` (uses `Arc`).
    #[cfg(any(feature = "std", feature = "alloc"))]
    #[cfg_attr(all(feature = "std", feature = "serde"), serde(skip))]
    #[cfg_attr(feature = "arbitrary", arbitrary(default))]
    pub source: Option<Arc<dyn core::error::Error + Send + Sync + 'static>>,
    /// Nested cause chain serialized as RFC 9457 extension member `"causes"`.
    ///
    /// Each entry is a nested Problem Details object. Omitted when empty.
    /// Preserved through [`From`] conversions.
    #[cfg_attr(
        all(feature = "std", feature = "serde"),
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    #[cfg_attr(feature = "arbitrary", arbitrary(default))]
    pub causes: Vec<Self>,
    /// Arbitrary RFC 9457 extension members attached by the caller.
    ///
    /// Serialized **inline** at the top level of the JSON object (flattened).
    /// Keys must not collide with the standard Problem Details fields.
    ///
    /// Use [`ApiError::with_extension`] to attach values.
    #[cfg(all(any(feature = "std", feature = "alloc"), feature = "serde"))]
    #[cfg_attr(all(feature = "std", feature = "serde"), serde(flatten))]
    #[cfg_attr(feature = "arbitrary", arbitrary(default))]
    pub extensions: BTreeMap<String, serde_json::Value>,
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl PartialEq for ApiError {
    fn eq(&self, other: &Self) -> bool {
        // `source` is intentionally excluded: trait objects have no meaningful
        // equality and the field is not part of the wire format.
        self.code == other.code
            && self.title == other.title
            && self.status == other.status
            && self.detail == other.detail
            && self.errors == other.errors
            && self.rate_limit == other.rate_limit
            && self.causes == other.causes
            // extensions only exist when serde is on
            && {
                #[cfg(all(any(feature = "std", feature = "alloc"), feature = "serde"))]
                { self.extensions == other.extensions }
                #[cfg(not(all(any(feature = "std", feature = "alloc"), feature = "serde")))]
                true
            }
            // request_id only exists when the `uuid` feature is on
            && {
                #[cfg(feature = "uuid")]
                { self.request_id == other.request_id }
                #[cfg(not(feature = "uuid"))]
                true
            }
    }
}

/// Serde module: serialize/deserialize `Option<Uuid>` as `"urn:uuid:<uuid>"` strings.
/// Used for the RFC 9457 §3.1.5 `instance` field (RFC 4122 §3 `urn:uuid:` scheme).
#[cfg(all(
    feature = "serde",
    feature = "uuid",
    any(feature = "std", feature = "alloc")
))]
mod uuid_urn_option {
    use serde::{Deserialize, Deserializer, Serializer};

    #[allow(clippy::ref_option)] // serde `with` module requires &Option<T> — not caller-controlled
    pub fn serialize<S: Serializer>(uuid: &Option<uuid::Uuid>, s: S) -> Result<S::Ok, S::Error> {
        match uuid {
            Some(id) => s.serialize_str(&format!("urn:uuid:{id}")),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<uuid::Uuid>, D::Error> {
        let opt = Option::<String>::deserialize(d)?;
        match opt {
            None => Ok(None),
            Some(ref urn) => {
                let hex = urn.strip_prefix("urn:uuid:").ok_or_else(|| {
                    serde::de::Error::custom(format!("expected urn:uuid: prefix, got {urn}"))
                })?;
                hex.parse::<uuid::Uuid>()
                    .map(Some)
                    .map_err(serde::de::Error::custom)
            }
        }
    }
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl ApiError {
    /// Create a new `ApiError`. `title` and `status` are derived from `code`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::error::{ApiError, ErrorCode};
    ///
    /// let err = ApiError::new(ErrorCode::BadRequest, "missing field");
    /// assert_eq!(err.status, 400);
    /// assert_eq!(err.title, "Bad Request");
    /// assert_eq!(err.detail, "missing field");
    /// ```
    pub fn new(code: ErrorCode, detail: impl Into<String>) -> Self {
        let status = code.status_code();
        debug_assert!(
            (100..=599).contains(&status),
            "status {status} is not a valid HTTP status code (RFC 9457 §3.1.3 requires 100–599)"
        );
        Self {
            title: code.title().to_owned(),
            status,
            detail: detail.into(),
            code,
            #[cfg(feature = "uuid")]
            request_id: None,
            errors: Vec::new(),
            rate_limit: None,
            source: None,
            causes: Vec::new(),
            #[cfg(all(any(feature = "std", feature = "alloc"), feature = "serde"))]
            extensions: BTreeMap::new(),
        }
    }

    /// Attach a request ID (typically set by tracing middleware).
    /// Serializes as `"instance": "urn:uuid:<id>"` per RFC 9457 §3.1.5 + RFC 4122 §3.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::error::{ApiError, ErrorCode};
    /// use uuid::Uuid;
    ///
    /// let err = ApiError::new(ErrorCode::BadRequest, "oops")
    ///     .with_request_id(Uuid::nil());
    /// assert_eq!(err.request_id, Some(Uuid::nil()));
    /// ```
    #[cfg(feature = "uuid")]
    #[must_use]
    pub fn with_request_id(mut self, id: uuid::Uuid) -> Self {
        self.request_id = Some(id);
        self
    }

    /// Attach structured field-level validation errors.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::error::{ApiError, ErrorCode, ValidationError};
    ///
    /// let err = ApiError::new(ErrorCode::ValidationFailed, "invalid input")
    ///     .with_errors(vec![
    ///         ValidationError { field: "/email".into(), message: "invalid".into(), rule: None },
    ///     ]);
    /// assert_eq!(err.errors.len(), 1);
    /// ```
    #[must_use]
    pub fn with_errors(mut self, errors: Vec<ValidationError>) -> Self {
        self.errors = errors;
        self
    }

    /// Attach an upstream error as the `source()` for this `ApiError`.
    ///
    /// The source is exposed via [`core::error::Error::source`] for error-chain
    /// tools (`anyhow`, `eyre`, tracing) but is **not** serialized to the wire.
    ///
    /// Requires `std` or `alloc` (uses `Arc`).
    #[cfg(any(feature = "std", feature = "alloc"))]
    #[must_use]
    pub fn with_source(mut self, source: impl core::error::Error + Send + Sync + 'static) -> Self {
        self.source = Some(Arc::new(source));
        self
    }

    /// Attach a chain of nested cause errors, serialized as `"causes"` in
    /// Problem Details output.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::error::{ApiError, ErrorCode};
    ///
    /// let cause = ApiError::not_found("upstream resource missing");
    /// let err = ApiError::internal("pipeline failed")
    ///     .with_causes(vec![cause]);
    /// assert_eq!(err.causes.len(), 1);
    /// ```
    #[must_use]
    pub fn with_causes(mut self, causes: Vec<Self>) -> Self {
        self.causes = causes;
        self
    }

    /// Attach a single arbitrary RFC 9457 extension member.
    ///
    /// The value is serialized **inline** (flattened) in the Problem Details
    /// object. Requires the `serde` feature.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::error::ApiError;
    ///
    /// let err = ApiError::internal("boom")
    ///     .with_extension("trace_id", "abc-123");
    /// # #[cfg(feature = "serde")]
    /// assert_eq!(err.extensions["trace_id"], "abc-123");
    /// ```
    #[cfg(all(any(feature = "std", feature = "alloc"), feature = "serde"))]
    #[must_use]
    pub fn with_extension(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.extensions.insert(key.into(), value.into());
        self
    }

    /// HTTP status code.
    #[must_use]
    pub fn status_code(&self) -> u16 {
        self.status
    }

    /// Whether this is a client error (4xx).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::error::{ApiError, ErrorCode};
    ///
    /// assert!(ApiError::not_found("gone").is_client_error());
    /// assert!(!ApiError::internal("boom").is_client_error());
    /// ```
    #[must_use]
    pub fn is_client_error(&self) -> bool {
        self.status < 500
    }

    /// Whether this is a server error (5xx).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::error::{ApiError, ErrorCode};
    ///
    /// assert!(ApiError::internal("boom").is_server_error());
    /// assert!(!ApiError::not_found("gone").is_server_error());
    /// ```
    #[must_use]
    pub fn is_server_error(&self) -> bool {
        self.status >= 500
    }

    // -----------------------------------------------------------------------
    // Convenience constructors
    // -----------------------------------------------------------------------

    /// 400 Bad Request.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::error::ApiError;
    ///
    /// let err = ApiError::bad_request("missing param");
    /// assert_eq!(err.status, 400);
    /// assert_eq!(err.title, "Bad Request");
    /// ```
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::BadRequest, msg)
    }

    /// 400 Validation Failed.
    pub fn validation_failed(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::ValidationFailed, msg)
    }

    /// 401 Unauthorized.
    pub fn unauthorized(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Unauthorized, msg)
    }

    /// 401 Invalid Credentials.
    #[must_use]
    pub fn invalid_credentials() -> Self {
        Self::new(ErrorCode::InvalidCredentials, "Invalid credentials")
    }

    /// 401 Token Expired.
    #[must_use]
    pub fn token_expired() -> Self {
        Self::new(ErrorCode::TokenExpired, "Token has expired")
    }

    /// 403 Forbidden.
    pub fn forbidden(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Forbidden, msg)
    }

    /// 403 Insufficient Permissions.
    pub fn insufficient_permissions(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::InsufficientPermissions, msg)
    }

    /// 404 Resource Not Found.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::error::ApiError;
    ///
    /// let err = ApiError::not_found("user 42 not found");
    /// assert_eq!(err.status, 404);
    /// assert_eq!(err.title, "Resource Not Found");
    /// ```
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::ResourceNotFound, msg)
    }

    /// 409 Conflict.
    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Conflict, msg)
    }

    /// 409 Resource Already Exists.
    pub fn already_exists(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::ResourceAlreadyExists, msg)
    }

    /// 422 Unprocessable Entity.
    pub fn unprocessable(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::UnprocessableEntity, msg)
    }

    /// 429 Rate Limited.
    #[must_use]
    pub fn rate_limited(retry_after_seconds: u64) -> Self {
        Self::new(
            ErrorCode::RateLimited,
            format!("Rate limited, retry after {retry_after_seconds}s"),
        )
    }

    /// Attach structured [`RateLimitInfo`](crate::ratelimit::RateLimitInfo)
    /// metadata. Serialized as the top-level `rate_limit` member and
    /// propagated to [`ProblemJson`] as an extension of the same name.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::error::ApiError;
    /// use api_bones::ratelimit::RateLimitInfo;
    ///
    /// let info = RateLimitInfo::new(100, 0, 1_700_000_000).retry_after(60);
    /// let err = ApiError::rate_limited(60).with_rate_limit(info.clone());
    /// assert_eq!(err.rate_limit.as_ref(), Some(&info));
    /// ```
    #[must_use]
    pub fn with_rate_limit(mut self, info: crate::ratelimit::RateLimitInfo) -> Self {
        self.rate_limit = Some(info);
        self
    }

    /// 429 Rate Limited with structured quota metadata.
    ///
    /// Convenience over [`ApiError::rate_limited`] +
    /// [`ApiError::with_rate_limit`]. The `detail` string is derived from
    /// `info.retry_after` when set.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::error::ApiError;
    /// use api_bones::ratelimit::RateLimitInfo;
    ///
    /// let info = RateLimitInfo::new(100, 0, 1_700_000_000).retry_after(30);
    /// let err = ApiError::rate_limited_with(info);
    /// assert_eq!(err.status, 429);
    /// assert!(err.rate_limit.is_some());
    /// ```
    #[must_use]
    pub fn rate_limited_with(info: crate::ratelimit::RateLimitInfo) -> Self {
        let detail = match info.retry_after {
            Some(secs) => format!("Rate limited, retry after {secs}s"),
            None => "Rate limited".to_string(),
        };
        Self::new(ErrorCode::RateLimited, detail).with_rate_limit(info)
    }

    /// 500 Internal Server Error. **Never expose internal details in `msg`.**
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::InternalServerError, msg)
    }

    /// 503 Service Unavailable.
    pub fn unavailable(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::ServiceUnavailable, msg)
    }

    /// Return a typed builder for constructing an `ApiError`.
    ///
    /// Required fields (`code` and `detail`) must be set before calling
    /// [`ApiErrorBuilder::build`]; the compiler enforces this via typestate.
    ///
    /// # Example
    /// ```rust
    /// use api_bones::error::{ApiError, ErrorCode};
    ///
    /// let err = ApiError::builder()
    ///     .code(ErrorCode::ResourceNotFound)
    ///     .detail("Booking 123 not found")
    ///     .build();
    /// assert_eq!(err.status, 404);
    /// ```
    #[must_use]
    pub fn builder() -> ApiErrorBuilder<(), ()> {
        ApiErrorBuilder {
            code: (),
            detail: (),
            #[cfg(feature = "uuid")]
            request_id: None,
            errors: Vec::new(),
            causes: Vec::new(),
        }
    }

    #[cfg(feature = "uuid")]
    fn with_request_id_opt(mut self, id: Option<uuid::Uuid>) -> Self {
        self.request_id = id;
        self
    }

    #[cfg(not(feature = "uuid"))]
    fn with_request_id_opt(self, _id: Option<()>) -> Self {
        self
    }
}

// ---------------------------------------------------------------------------
// ApiError builder — typestate
// ---------------------------------------------------------------------------

/// Typestate builder for [`ApiError`].
///
/// Type parameters track whether required fields have been set:
/// - `C` — `ErrorCode` once `.code()` is called, `()` otherwise
/// - `D` — `String` once `.detail()` is called, `()` otherwise
///
/// [`ApiErrorBuilder::build`] is only available when both are set.
///
/// Requires `std` or `alloc`.
#[cfg(any(feature = "std", feature = "alloc"))]
pub struct ApiErrorBuilder<C, D> {
    code: C,
    detail: D,
    #[cfg(feature = "uuid")]
    request_id: Option<uuid::Uuid>,
    errors: Vec<ValidationError>,
    causes: Vec<ApiError>,
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl<D> ApiErrorBuilder<(), D> {
    /// Set the error code. `title` and `status` are derived from it automatically.
    pub fn code(self, code: ErrorCode) -> ApiErrorBuilder<ErrorCode, D> {
        ApiErrorBuilder {
            code,
            detail: self.detail,
            #[cfg(feature = "uuid")]
            request_id: self.request_id,
            errors: self.errors,
            causes: self.causes,
        }
    }
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl<C> ApiErrorBuilder<C, ()> {
    /// Set the human-readable error detail message.
    pub fn detail(self, detail: impl Into<String>) -> ApiErrorBuilder<C, String> {
        ApiErrorBuilder {
            code: self.code,
            detail: detail.into(),
            #[cfg(feature = "uuid")]
            request_id: self.request_id,
            errors: self.errors,
            causes: self.causes,
        }
    }
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl<C, D> ApiErrorBuilder<C, D> {
    /// Attach a request ID.
    #[cfg(feature = "uuid")]
    #[must_use]
    pub fn request_id(mut self, id: uuid::Uuid) -> Self {
        self.request_id = Some(id);
        self
    }

    /// Attach structured field-level validation errors.
    #[must_use]
    pub fn errors(mut self, errors: Vec<ValidationError>) -> Self {
        self.errors = errors;
        self
    }

    /// Attach a chain of nested cause errors (serialized as `"causes"`).
    #[must_use]
    pub fn causes(mut self, causes: Vec<ApiError>) -> Self {
        self.causes = causes;
        self
    }
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl ApiErrorBuilder<ErrorCode, String> {
    /// Build the [`ApiError`].
    ///
    /// Only available once both `code` and `detail` have been set.
    #[must_use]
    pub fn build(self) -> ApiError {
        #[cfg(feature = "uuid")]
        let built = ApiError::new(self.code, self.detail).with_request_id_opt(self.request_id);
        #[cfg(not(feature = "uuid"))]
        let built = ApiError::new(self.code, self.detail).with_request_id_opt(None::<()>);
        built.with_errors(self.errors).with_causes(self.causes)
    }
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.detail)
    }
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl core::error::Error for ApiError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|s| s as &(dyn core::error::Error + 'static))
    }
}

// ---------------------------------------------------------------------------
// proptest::arbitrary::Arbitrary for ApiError
// ---------------------------------------------------------------------------
// uuid::Uuid does not implement proptest::arbitrary::Arbitrary, so we write
// a manual Strategy that constructs a Uuid from a random u128 value.

#[cfg(all(
    feature = "proptest",
    feature = "uuid",
    any(feature = "std", feature = "alloc")
))]
impl proptest::arbitrary::Arbitrary for ApiError {
    type Parameters = ();
    type Strategy = proptest::strategy::BoxedStrategy<Self>;

    fn arbitrary_with((): ()) -> Self::Strategy {
        use proptest::prelude::*;
        (
            any::<ErrorCode>(),
            any::<String>(),
            any::<u16>(),
            any::<String>(),
            proptest::option::of(any::<u128>().prop_map(uuid::Uuid::from_u128)),
            any::<Vec<ValidationError>>(),
        )
            .prop_map(|(code, title, status, detail, request_id, errors)| Self {
                code,
                title,
                status,
                detail,
                #[cfg(feature = "uuid")]
                request_id,
                errors,
                rate_limit: None,
                source: None,
                causes: Vec::new(),
                #[cfg(all(any(feature = "std", feature = "alloc"), feature = "serde"))]
                extensions: BTreeMap::new(),
            })
            .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialises access to the global `ErrorTypeMode` and environment
    /// variables so that tests which mutate them cannot interfere with
    /// each other, even when `cargo test` runs them in parallel threads.
    static MODE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// RAII guard that resets `ErrorTypeMode` on drop so subsequent tests
    /// always start from a clean slate.
    struct ModeGuard(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

    impl Drop for ModeGuard {
        fn drop(&mut self) {
            reset_error_type_mode();
        }
    }

    /// Acquire `MODE_LOCK`, reset the cached mode, and return the guard.
    /// The mode is also reset when the guard is dropped.
    fn lock_and_reset_mode() -> ModeGuard {
        let guard = MODE_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset_error_type_mode();
        ModeGuard(guard)
    }

    // -----------------------------------------------------------------------
    // TryFrom<u16> for ErrorCode — issue #152
    // -----------------------------------------------------------------------

    #[test]
    fn error_code_try_from_u16_non_error_returns_err() {
        for code in [100_u16, 200, 204, 301, 302, 304] {
            assert!(
                ErrorCode::try_from(code).is_err(),
                "expected Err for status {code}"
            );
        }
    }

    #[test]
    fn error_code_try_from_u16_unmapped_4xx_returns_err() {
        // e.g. 418 I'm a Teapot — no canonical ErrorCode variant
        assert!(ErrorCode::try_from(418_u16).is_err());
    }

    #[test]
    fn error_code_try_from_u16_roundtrip() {
        // For every variant whose status_code() maps back uniquely, the
        // roundtrip ErrorCode -> u16 -> ErrorCode must succeed and match.
        // Variants that share a status code (e.g. BadRequest/ValidationFailed)
        // only roundtrip to the canonical (first-matched) variant.
        let canonical_variants = [
            ErrorCode::BadRequest,
            ErrorCode::Unauthorized,
            ErrorCode::Forbidden,
            ErrorCode::ResourceNotFound,
            ErrorCode::MethodNotAllowed,
            ErrorCode::NotAcceptable,
            ErrorCode::RequestTimeout,
            ErrorCode::Conflict,
            ErrorCode::Gone,
            ErrorCode::PreconditionFailed,
            ErrorCode::PayloadTooLarge,
            ErrorCode::UnsupportedMediaType,
            ErrorCode::UnprocessableEntity,
            ErrorCode::PreconditionRequired,
            ErrorCode::RateLimited,
            ErrorCode::RequestHeaderFieldsTooLarge,
            ErrorCode::InternalServerError,
            ErrorCode::NotImplemented,
            ErrorCode::BadGateway,
            ErrorCode::ServiceUnavailable,
            ErrorCode::GatewayTimeout,
        ];
        for variant in &canonical_variants {
            let status = variant.status_code();
            let roundtripped =
                ErrorCode::try_from(status).expect("canonical variant should round-trip");
            assert_eq!(
                roundtripped, *variant,
                "roundtrip failed for {variant:?} (status {status})"
            );
        }
    }

    #[cfg(feature = "http")]
    #[test]
    fn error_code_try_from_status_code_non_error_returns_err() {
        use http::StatusCode;
        assert!(ErrorCode::try_from(StatusCode::OK).is_err());
        assert!(ErrorCode::try_from(StatusCode::MOVED_PERMANENTLY).is_err());
    }

    #[cfg(feature = "http")]
    #[test]
    fn error_code_try_from_status_code_roundtrip() {
        use http::StatusCode;
        let pairs = [
            (StatusCode::NOT_FOUND, ErrorCode::ResourceNotFound),
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorCode::InternalServerError,
            ),
            (StatusCode::TOO_MANY_REQUESTS, ErrorCode::RateLimited),
            (StatusCode::UNAUTHORIZED, ErrorCode::Unauthorized),
        ];
        for (sc, expected) in &pairs {
            assert_eq!(
                ErrorCode::try_from(*sc),
                Ok(expected.clone()),
                "failed for {sc}"
            );
        }
    }

    #[test]
    fn status_codes() {
        assert_eq!(ApiError::bad_request("x").status_code(), 400);
        assert_eq!(ApiError::unauthorized("x").status_code(), 401);
        assert_eq!(ApiError::invalid_credentials().status_code(), 401);
        assert_eq!(ApiError::token_expired().status_code(), 401);
        assert_eq!(ApiError::forbidden("x").status_code(), 403);
        assert_eq!(ApiError::not_found("x").status_code(), 404);
        assert_eq!(ApiError::conflict("x").status_code(), 409);
        assert_eq!(ApiError::already_exists("x").status_code(), 409);
        assert_eq!(ApiError::unprocessable("x").status_code(), 422);
        assert_eq!(ApiError::rate_limited(30).status_code(), 429);
        assert_eq!(ApiError::internal("x").status_code(), 500);
        assert_eq!(ApiError::unavailable("x").status_code(), 503);
    }

    #[test]
    fn status_in_valid_http_range() {
        // RFC 9457 §3.1.3: status MUST be a valid HTTP status code (100–599)
        for err in [
            ApiError::bad_request("x"),
            ApiError::unauthorized("x"),
            ApiError::forbidden("x"),
            ApiError::not_found("x"),
            ApiError::conflict("x"),
            ApiError::unprocessable("x"),
            ApiError::rate_limited(30),
            ApiError::internal("x"),
            ApiError::unavailable("x"),
        ] {
            assert!(
                (100..=599).contains(&err.status),
                "status {} out of RFC 9457 §3.1.3 range",
                err.status
            );
        }
    }

    #[test]
    fn error_code_urn() {
        let _g = lock_and_reset_mode();
        assert_eq!(
            ErrorCode::ResourceNotFound.urn(),
            "urn:api-bones:error:resource-not-found"
        );
        assert_eq!(
            ErrorCode::ValidationFailed.urn(),
            "urn:api-bones:error:validation-failed"
        );
        assert_eq!(
            ErrorCode::InternalServerError.urn(),
            "urn:api-bones:error:internal-server-error"
        );
    }

    #[test]
    fn error_code_from_type_uri_roundtrip() {
        let _g = lock_and_reset_mode();
        let codes = [
            ErrorCode::BadRequest,
            ErrorCode::ValidationFailed,
            ErrorCode::Unauthorized,
            ErrorCode::ResourceNotFound,
            ErrorCode::InternalServerError,
            ErrorCode::ServiceUnavailable,
        ];
        for code in &codes {
            let urn = code.urn();
            assert_eq!(ErrorCode::from_type_uri(&urn).as_ref(), Some(code));
        }
    }

    #[test]
    fn error_code_from_type_uri_unknown() {
        let _g = lock_and_reset_mode();
        assert!(ErrorCode::from_type_uri("urn:api-bones:error:unknown-thing").is_none());
        assert!(ErrorCode::from_type_uri("RESOURCE_NOT_FOUND").is_none());
    }

    #[test]
    fn display_format() {
        let _g = lock_and_reset_mode();
        let e = ApiError::not_found("booking 123 not found");
        assert_eq!(
            e.to_string(),
            "[urn:api-bones:error:resource-not-found] booking 123 not found"
        );
    }

    #[test]
    fn title_populated() {
        let e = ApiError::not_found("x");
        assert_eq!(e.title, "Resource Not Found");
    }

    #[test]
    fn with_request_id() {
        let id = uuid::Uuid::new_v4();
        let e = ApiError::internal("oops").with_request_id(id);
        assert_eq!(e.request_id, Some(id));
    }

    #[test]
    fn with_errors() {
        let e = ApiError::validation_failed("invalid input").with_errors(vec![ValidationError {
            field: "/email".to_owned(),
            message: "invalid format".to_owned(),
            rule: Some("format".to_owned()),
        }]);
        assert!(!e.errors.is_empty());
        assert_eq!(e.errors[0].field, "/email");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn wire_format() {
        let _g = lock_and_reset_mode();
        let e = ApiError::not_found("booking 123 not found");
        let json = serde_json::to_value(&e).unwrap();
        // RFC 9457: no custom envelope wrapper
        assert!(json.get("error").is_none());
        // RFC 9457 §3.1.1: type MUST be a URI reference
        assert_eq!(json["type"], "urn:api-bones:error:resource-not-found");
        assert_eq!(json["title"], "Resource Not Found");
        assert_eq!(json["status"], 404);
        assert_eq!(json["detail"], "booking 123 not found");
        // Optional fields omitted when absent
        assert!(json.get("instance").is_none());
        assert!(json.get("errors").is_none());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn wire_format_instance_is_urn_uuid() {
        let _g = lock_and_reset_mode();
        // RFC 9457 §3.1.5: instance is a URI; RFC 4122 §3: urn:uuid: scheme
        let id = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let e = ApiError::internal("oops").with_request_id(id);
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(
            json["instance"],
            "urn:uuid:550e8400-e29b-41d4-a716-446655440000"
        );
        // Old field name must NOT appear
        assert!(json.get("request_id").is_none());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn wire_format_with_errors() {
        let _g = lock_and_reset_mode();
        let e = ApiError::validation_failed("bad input").with_errors(vec![ValidationError {
            field: "/name".to_owned(),
            message: "required".to_owned(),
            rule: None,
        }]);
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["type"], "urn:api-bones:error:validation-failed");
        assert_eq!(json["status"], 400);
        assert!(json["errors"].is_array());
        assert_eq!(json["errors"][0]["field"], "/name");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn snapshot_not_found() {
        let _g = lock_and_reset_mode();
        let e = ApiError::not_found("booking 123 not found");
        let json = serde_json::to_value(&e).unwrap();
        let expected = serde_json::json!({
            "type": "urn:api-bones:error:resource-not-found",
            "title": "Resource Not Found",
            "status": 404,
            "detail": "booking 123 not found"
        });
        assert_eq!(json, expected);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn snapshot_validation_failed_with_errors() {
        let _g = lock_and_reset_mode();
        let e = ApiError::validation_failed("invalid input").with_errors(vec![
            ValidationError {
                field: "/email".to_owned(),
                message: "invalid format".to_owned(),
                rule: Some("format".to_owned()),
            },
            ValidationError {
                field: "/name".to_owned(),
                message: "required".to_owned(),
                rule: None,
            },
        ]);
        let json = serde_json::to_value(&e).unwrap();
        let expected = serde_json::json!({
            "type": "urn:api-bones:error:validation-failed",
            "title": "Validation Failed",
            "status": 400,
            "detail": "invalid input",
            "errors": [
                {"field": "/email", "message": "invalid format", "rule": "format"},
                {"field": "/name", "message": "required"}
            ]
        });
        assert_eq!(json, expected);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn error_code_serde_roundtrip() {
        let _g = lock_and_reset_mode();
        let code = ErrorCode::ResourceNotFound;
        let json = serde_json::to_value(&code).unwrap();
        assert_eq!(json, "urn:api-bones:error:resource-not-found");
        let back: ErrorCode = serde_json::from_value(json).unwrap();
        assert_eq!(back, code);
    }

    #[test]
    fn client_vs_server() {
        assert!(ApiError::not_found("x").is_client_error());
        assert!(!ApiError::not_found("x").is_server_error());
        assert!(ApiError::internal("x").is_server_error());
    }

    // -----------------------------------------------------------------------
    // ErrorTypeMode::render — URL variant (line 105)
    // -----------------------------------------------------------------------

    #[test]
    fn error_type_mode_render_url() {
        let mode = ErrorTypeMode::Url {
            base_url: "https://docs.example.com/errors".into(),
        };
        assert_eq!(
            mode.render("resource-not-found"),
            "https://docs.example.com/errors/resource-not-found"
        );
        // trailing slash in base_url is trimmed
        let mode_slash = ErrorTypeMode::Url {
            base_url: "https://docs.example.com/errors/".into(),
        };
        assert_eq!(
            mode_slash.render("bad-request"),
            "https://docs.example.com/errors/bad-request"
        );
    }

    // -----------------------------------------------------------------------
    // set_error_type_mode + urn_namespace URL branch
    //
    // These tests mutate global state (ErrorTypeMode / env vars), so each
    // one resets the mode before and after via reset_error_type_mode().
    // -----------------------------------------------------------------------

    #[test]
    fn set_error_type_mode_url_and_urn_namespace_fallback() {
        let _g = lock_and_reset_mode();
        set_error_type_mode(ErrorTypeMode::Url {
            base_url: "https://docs.test.com/errors".into(),
        });
        assert_eq!(
            error_type_mode(),
            ErrorTypeMode::Url {
                base_url: "https://docs.test.com/errors".into()
            }
        );
        // urn_namespace() returns "api-bones" as a safe fallback in URL mode
        assert_eq!(urn_namespace(), "api-bones");
    }

    #[test]
    fn urn_namespace_urn_mode_returns_namespace() {
        let _g = lock_and_reset_mode();
        // Default mode is Urn { "api-bones" } — covers the Urn arm of urn_namespace()
        assert_eq!(urn_namespace(), "api-bones");
    }

    // -----------------------------------------------------------------------
    // error_type_mode() runtime env-var branches
    // -----------------------------------------------------------------------

    #[allow(unsafe_code)]
    #[test]
    fn error_type_mode_url_from_runtime_env() {
        let _g = lock_and_reset_mode();
        // Safety: single-threaded test; env var cleaned up after.
        unsafe {
            std::env::set_var(
                "SHARED_TYPES_ERROR_TYPE_BASE_URL",
                "https://env.example.com/errors",
            );
        }
        let mode = error_type_mode();
        assert!(
            matches!(mode, ErrorTypeMode::Url { base_url } if base_url == "https://env.example.com/errors")
        );
        unsafe {
            std::env::remove_var("SHARED_TYPES_ERROR_TYPE_BASE_URL");
        }
    }

    #[allow(unsafe_code)]
    #[test]
    fn error_type_mode_urn_from_runtime_env() {
        let _g = lock_and_reset_mode();
        // Safety: single-threaded test; env var cleaned up after.
        unsafe {
            std::env::set_var("SHARED_TYPES_URN_NAMESPACE", "testapp");
        }
        let mode = error_type_mode();
        assert!(matches!(mode, ErrorTypeMode::Urn { namespace } if namespace == "testapp"));
        unsafe {
            std::env::remove_var("SHARED_TYPES_URN_NAMESPACE");
        }
    }

    // -----------------------------------------------------------------------
    // from_type_uri — URL mode path
    // -----------------------------------------------------------------------

    #[test]
    fn from_type_uri_url_mode_paths() {
        let _g = lock_and_reset_mode();
        set_error_type_mode(ErrorTypeMode::Url {
            base_url: "https://docs.test.com/errors".into(),
        });
        // Primary: URL prefix match
        assert_eq!(
            ErrorCode::from_type_uri("https://docs.test.com/errors/resource-not-found"),
            Some(ErrorCode::ResourceNotFound)
        );
        // Fallback: URN format still accepted in URL mode
        assert_eq!(
            ErrorCode::from_type_uri("urn:api-bones:error:bad-request"),
            Some(ErrorCode::BadRequest)
        );
        // URL prefix matches but slug is unknown → None (via slug match wildcard)
        assert!(ErrorCode::from_type_uri("https://docs.test.com/errors/totally-unknown").is_none());
        // Neither prefix matches → ? operator fires on the or_else result
        assert!(ErrorCode::from_type_uri("not-a-url-or-urn").is_none());
    }

    // -----------------------------------------------------------------------
    // Complete coverage of all 27 ErrorCode variants:
    //   title(), urn_slug(), status_code(), from_type_uri() roundtrip
    // -----------------------------------------------------------------------

    #[test]
    #[allow(clippy::too_many_lines)]
    fn all_error_code_variants_title_slug_status() {
        let _g = lock_and_reset_mode();
        let cases: &[(ErrorCode, &str, &str, u16)] = &[
            (ErrorCode::BadRequest, "Bad Request", "bad-request", 400),
            (
                ErrorCode::ValidationFailed,
                "Validation Failed",
                "validation-failed",
                400,
            ),
            (ErrorCode::Unauthorized, "Unauthorized", "unauthorized", 401),
            (
                ErrorCode::InvalidCredentials,
                "Invalid Credentials",
                "invalid-credentials",
                401,
            ),
            (
                ErrorCode::TokenExpired,
                "Token Expired",
                "token-expired",
                401,
            ),
            (
                ErrorCode::TokenInvalid,
                "Token Invalid",
                "token-invalid",
                401,
            ),
            (ErrorCode::Forbidden, "Forbidden", "forbidden", 403),
            (
                ErrorCode::InsufficientPermissions,
                "Insufficient Permissions",
                "insufficient-permissions",
                403,
            ),
            (
                ErrorCode::ResourceNotFound,
                "Resource Not Found",
                "resource-not-found",
                404,
            ),
            (
                ErrorCode::MethodNotAllowed,
                "Method Not Allowed",
                "method-not-allowed",
                405,
            ),
            (
                ErrorCode::NotAcceptable,
                "Not Acceptable",
                "not-acceptable",
                406,
            ),
            (
                ErrorCode::RequestTimeout,
                "Request Timeout",
                "request-timeout",
                408,
            ),
            (ErrorCode::Conflict, "Conflict", "conflict", 409),
            (
                ErrorCode::ResourceAlreadyExists,
                "Resource Already Exists",
                "resource-already-exists",
                409,
            ),
            (ErrorCode::Gone, "Gone", "gone", 410),
            (
                ErrorCode::PreconditionFailed,
                "Precondition Failed",
                "precondition-failed",
                412,
            ),
            (
                ErrorCode::PayloadTooLarge,
                "Payload Too Large",
                "payload-too-large",
                413,
            ),
            (
                ErrorCode::UnsupportedMediaType,
                "Unsupported Media Type",
                "unsupported-media-type",
                415,
            ),
            (
                ErrorCode::UnprocessableEntity,
                "Unprocessable Entity",
                "unprocessable-entity",
                422,
            ),
            (
                ErrorCode::PreconditionRequired,
                "Precondition Required",
                "precondition-required",
                428,
            ),
            (ErrorCode::RateLimited, "Rate Limited", "rate-limited", 429),
            (
                ErrorCode::RequestHeaderFieldsTooLarge,
                "Request Header Fields Too Large",
                "request-header-fields-too-large",
                431,
            ),
            (
                ErrorCode::InternalServerError,
                "Internal Server Error",
                "internal-server-error",
                500,
            ),
            (
                ErrorCode::NotImplemented,
                "Not Implemented",
                "not-implemented",
                501,
            ),
            (ErrorCode::BadGateway, "Bad Gateway", "bad-gateway", 502),
            (
                ErrorCode::ServiceUnavailable,
                "Service Unavailable",
                "service-unavailable",
                503,
            ),
            (
                ErrorCode::GatewayTimeout,
                "Gateway Timeout",
                "gateway-timeout",
                504,
            ),
        ];
        for (code, title, slug, status) in cases {
            assert_eq!(code.title(), *title, "title mismatch for {slug}");
            assert_eq!(code.urn_slug(), *slug, "slug mismatch");
            assert_eq!(code.status_code(), *status, "status mismatch for {slug}");
            // urn() roundtrip via from_type_uri()
            let urn = code.urn();
            assert_eq!(
                ErrorCode::from_type_uri(&urn).as_ref(),
                Some(code),
                "from_type_uri roundtrip failed for {urn}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // insufficient_permissions() convenience constructor (lines 515–517)
    // -----------------------------------------------------------------------

    #[test]
    fn insufficient_permissions_constructor() {
        let e = ApiError::insufficient_permissions("missing admin role");
        assert_eq!(e.status_code(), 403);
        assert_eq!(e.title, "Insufficient Permissions");
        assert!(e.is_client_error());
    }

    // -----------------------------------------------------------------------
    // uuid_urn_option: serialize None branch + full deserializer coverage
    // (lines 407, 411–424)
    // -----------------------------------------------------------------------

    #[cfg(feature = "serde")]
    #[test]
    fn error_code_deserialize_non_string_is_error() {
        let _g = lock_and_reset_mode();
        // Covers the ? on String::deserialize in ErrorCode::deserialize (line 321)
        let result: Result<ErrorCode, _> = serde_json::from_value(serde_json::json!(42));
        assert!(result.is_err());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn error_code_deserialize_unknown_uri_is_error() {
        let _g = lock_and_reset_mode();
        // Covers ok_or_else closure in ErrorCode::deserialize (lines 322–323)
        let result: Result<ErrorCode, _> =
            serde_json::from_value(serde_json::json!("urn:api-bones:error:does-not-exist"));
        assert!(result.is_err());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn uuid_urn_option_serialize_none_produces_null() {
        // The None arm exists for the serde `with` protocol. Since
        // skip_serializing_if = "Option::is_none" is set on the field, serde
        // never calls this in practice — test it directly.
        use serde_json::Serializer as JsonSerializer;
        let mut buf = Vec::new();
        let mut s = JsonSerializer::new(&mut buf);
        uuid_urn_option::serialize(&None, &mut s).unwrap();
        assert_eq!(buf, b"null");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn uuid_urn_option_deserialize_non_string_is_error() {
        let _g = lock_and_reset_mode();
        // Covers the ? failure path in deserialize (line 415): Option<String>::deserialize
        // returns Err when the JSON value is not a string or null.
        let json = serde_json::json!({
            "type": "urn:api-bones:error:bad-request",
            "title": "Bad Request",
            "status": 400,
            "detail": "x",
            "instance": 42
        });
        let result: Result<ApiError, _> = serde_json::from_value(json);
        assert!(result.is_err());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn uuid_urn_option_deserialize_null_gives_none() {
        let _g = lock_and_reset_mode();
        // Triggers the None arm in deserialize (line 414).
        let json = serde_json::json!({
            "type": "urn:api-bones:error:bad-request",
            "title": "Bad Request",
            "status": 400,
            "detail": "x",
            "instance": null
        });
        let e: ApiError = serde_json::from_value(json).unwrap();
        assert!(e.request_id.is_none());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn uuid_urn_option_deserialize_valid_urn_uuid() {
        let _g = lock_and_reset_mode();
        // Triggers the happy-path Some arm in deserialize (lines 415–421).
        let id = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let json = serde_json::json!({
            "type": "urn:api-bones:error:bad-request",
            "title": "Bad Request",
            "status": 400,
            "detail": "x",
            "instance": "urn:uuid:550e8400-e29b-41d4-a716-446655440000"
        });
        let e: ApiError = serde_json::from_value(json).unwrap();
        assert_eq!(e.request_id, Some(id));
    }

    #[cfg(feature = "serde")]
    #[test]
    fn uuid_urn_option_deserialize_bad_prefix_is_error() {
        let _g = lock_and_reset_mode();
        // Triggers the ok_or_else error path (lines 416–418).
        let json = serde_json::json!({
            "type": "urn:api-bones:error:bad-request",
            "title": "Bad Request",
            "status": 400,
            "detail": "x",
            "instance": "uuid:550e8400-e29b-41d4-a716-446655440000"
        });
        let result: Result<ApiError, _> = serde_json::from_value(json);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // ApiError builder tests
    // -----------------------------------------------------------------------

    #[test]
    fn builder_basic() {
        let err = ApiError::builder()
            .code(ErrorCode::ResourceNotFound)
            .detail("Booking 123 not found")
            .build();
        assert_eq!(err.status, 404);
        assert_eq!(err.title, "Resource Not Found");
        assert_eq!(err.detail, "Booking 123 not found");
        assert!(err.request_id.is_none());
        assert!(err.errors.is_empty());
    }

    #[test]
    fn builder_equivalence_with_new() {
        let via_new = ApiError::new(ErrorCode::BadRequest, "bad");
        let via_builder = ApiError::builder()
            .code(ErrorCode::BadRequest)
            .detail("bad")
            .build();
        assert_eq!(via_new, via_builder);
    }

    #[test]
    fn builder_chaining_all_optionals() {
        let id = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let errs = vec![ValidationError {
            field: "/email".to_owned(),
            message: "invalid".to_owned(),
            rule: None,
        }];
        let err = ApiError::builder()
            .code(ErrorCode::ValidationFailed)
            .detail("invalid input")
            .request_id(id)
            .errors(errs.clone())
            .build();
        assert_eq!(err.request_id, Some(id));
        assert_eq!(err.errors, errs);
    }

    #[test]
    fn builder_detail_before_code() {
        // Typestate allows setting detail before code
        let err = ApiError::builder()
            .detail("forbidden action")
            .code(ErrorCode::Forbidden)
            .build();
        assert_eq!(err.status, 403);
        assert_eq!(err.detail, "forbidden action");
    }

    // -----------------------------------------------------------------------
    // Error source() chaining — issue #37
    // -----------------------------------------------------------------------

    #[test]
    fn api_error_source_none_by_default() {
        use std::error::Error;
        let err = ApiError::not_found("booking 42");
        assert!(err.source().is_none());
    }

    #[test]
    fn api_error_with_source_chain_is_walkable() {
        use std::error::Error;

        #[derive(Debug)]
        struct RootCause;
        impl std::fmt::Display for RootCause {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("database connection refused")
            }
        }
        impl Error for RootCause {}

        let err = ApiError::internal("upstream failure").with_source(RootCause);

        // source() returns the attached cause
        let source = err.source().expect("source should be set");
        assert_eq!(source.to_string(), "database connection refused");

        // chain ends after one hop
        assert!(source.source().is_none());
    }

    #[test]
    fn api_error_source_chain_two_levels() {
        use std::error::Error;

        #[derive(Debug)]
        struct Mid(std::io::Error);
        impl std::fmt::Display for Mid {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "mid: {}", self.0)
            }
        }
        impl Error for Mid {
            fn source(&self) -> Option<&(dyn Error + 'static)> {
                Some(&self.0)
            }
        }

        let io_err = std::io::Error::new(std::io::ErrorKind::TimedOut, "timed out");
        let mid = Mid(io_err);

        let err = ApiError::unavailable("service down").with_source(mid);

        let hop1 = err.source().expect("first source");
        assert!(hop1.to_string().starts_with("mid:"));

        let hop2 = hop1.source().expect("second source");
        assert_eq!(hop2.to_string(), "timed out");
    }

    #[test]
    fn api_error_partial_eq_ignores_source() {
        #[derive(Debug)]
        struct Cause;
        impl std::fmt::Display for Cause {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("cause")
            }
        }
        impl std::error::Error for Cause {}

        // Exercise the Display impl (required by std::error::Error) so coverage is satisfied.
        assert_eq!(Cause.to_string(), "cause");
        let a = ApiError::not_found("x");
        let b = ApiError::not_found("x").with_source(Cause);
        // source is intentionally excluded from PartialEq
        assert_eq!(a, b);
    }

    #[test]
    fn api_error_with_source_is_cloneable() {
        use std::error::Error;

        #[derive(Debug)]
        struct Cause;
        impl std::fmt::Display for Cause {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("cause")
            }
        }
        impl Error for Cause {}

        // Exercise Display (required by std::error::Error) for coverage.
        assert_eq!(Cause.to_string(), "cause");
        let a = ApiError::internal("oops").with_source(Cause);
        // Clone is derived; Arc clone shares the allocation.
        let b = a.clone();
        // Both a and b must have source set — verify both are still usable.
        assert!(a.source().is_some());
        assert!(b.source().is_some());
    }

    #[test]
    fn validation_error_display_with_rule() {
        let ve = ValidationError {
            field: "/email".to_owned(),
            message: "invalid format".to_owned(),
            rule: Some("format".to_owned()),
        };
        assert_eq!(ve.to_string(), "/email: invalid format (rule: format)");
    }

    #[test]
    fn validation_error_display_without_rule() {
        let ve = ValidationError {
            field: "/name".to_owned(),
            message: "required".to_owned(),
            rule: None,
        };
        assert_eq!(ve.to_string(), "/name: required");
    }

    #[test]
    fn validation_error_is_std_error() {
        use std::error::Error;
        let ve = ValidationError {
            field: "/age".to_owned(),
            message: "must be positive".to_owned(),
            rule: Some("min".to_owned()),
        };
        // source() is None — ValidationError has no inner cause
        assert!(ve.source().is_none());
        // usable as &dyn Error
        let _: &dyn Error = &ve;
    }

    #[test]
    fn api_error_source_downcast() {
        use std::error::Error;
        use std::sync::Arc;

        #[derive(Debug)]
        struct Typed(u32);
        impl std::fmt::Display for Typed {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "typed({})", self.0)
            }
        }
        impl Error for Typed {}

        // Exercise Display (required by std::error::Error) for coverage.
        assert_eq!(Typed(7).to_string(), "typed(7)");
        let err = ApiError::internal("oops").with_source(Typed(42));
        let source_arc: &Arc<dyn Error + Send + Sync> = err.source.as_ref().expect("source set");
        let downcasted = source_arc.downcast_ref::<Typed>();
        assert!(downcasted.is_some());
        assert_eq!(downcasted.unwrap().0, 42);
    }

    // -----------------------------------------------------------------------
    // schemars
    // -----------------------------------------------------------------------

    #[cfg(feature = "schemars")]
    #[test]
    fn error_code_schema_is_valid() {
        let schema = schemars::schema_for!(ErrorCode);
        let json = serde_json::to_value(&schema).expect("schema serializable");
        assert!(json.is_object(), "schema should be a JSON object");
    }

    #[cfg(all(feature = "schemars", any(feature = "std", feature = "alloc")))]
    #[test]
    fn api_error_schema_is_valid() {
        let schema = schemars::schema_for!(ApiError);
        let json = serde_json::to_value(&schema).expect("schema serializable");
        assert!(json.is_object());
        // Confirm top-level type property exists
        assert!(
            json.get("definitions").is_some()
                || json.get("$defs").is_some()
                || json.get("properties").is_some(),
            "schema should contain definitions or properties"
        );
    }

    #[cfg(all(feature = "schemars", any(feature = "std", feature = "alloc")))]
    #[test]
    fn validation_error_schema_is_valid() {
        let schema = schemars::schema_for!(ValidationError);
        let json = serde_json::to_value(&schema).expect("schema serializable");
        assert!(json.is_object());
    }

    // -----------------------------------------------------------------------
    // #108 — HttpError trait
    // -----------------------------------------------------------------------

    #[test]
    fn http_error_blanket_from() {
        #[derive(Debug)]
        struct NotFound(u64);

        impl HttpError for NotFound {
            fn status_code(&self) -> u16 {
                404
            }
            fn error_code(&self) -> ErrorCode {
                ErrorCode::ResourceNotFound
            }
            fn detail(&self) -> String {
                format!("item {} not found", self.0)
            }
        }

        assert_eq!(NotFound(99).status_code(), 404);
        let err: ApiError = NotFound(99).into();
        assert_eq!(err.status, 404);
        assert_eq!(err.code, ErrorCode::ResourceNotFound);
        assert_eq!(err.detail, "item 99 not found");
    }

    // -----------------------------------------------------------------------
    // #110 — nested causes
    // -----------------------------------------------------------------------

    #[test]
    fn with_causes_roundtrip() {
        let cause = ApiError::not_found("upstream missing");
        let err = ApiError::internal("pipeline failed").with_causes(vec![cause.clone()]);
        assert_eq!(err.causes.len(), 1);
        assert_eq!(err.causes[0].detail, cause.detail);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn causes_serialized_as_extension() {
        let _g = lock_and_reset_mode();
        let cause = ApiError::not_found("db row missing");
        let err = ApiError::internal("handler failed").with_causes(vec![cause]);
        let json = serde_json::to_value(&err).unwrap();
        let causes = json["causes"].as_array().expect("causes must be array");
        assert_eq!(causes.len(), 1);
        assert_eq!(causes[0]["status"], 404);
        assert_eq!(causes[0]["detail"], "db row missing");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn causes_omitted_when_empty() {
        let _g = lock_and_reset_mode();
        let err = ApiError::internal("oops");
        let json = serde_json::to_value(&err).unwrap();
        assert!(json.get("causes").is_none());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn causes_propagated_through_problem_json() {
        use crate::error::ProblemJson;
        let _g = lock_and_reset_mode();
        let cause = ApiError::not_found("missing row");
        let err = ApiError::internal("failed").with_causes(vec![cause]);
        let p = ProblemJson::from(err);
        assert!(p.extensions.contains_key("causes"));
        let causes = p.extensions["causes"].as_array().unwrap();
        assert_eq!(causes.len(), 1);
        assert_eq!(causes[0]["status"], 404);
    }

    #[test]
    fn builder_with_causes() {
        let cause = ApiError::bad_request("bad input");
        let err = ApiError::builder()
            .code(ErrorCode::UnprocessableEntity)
            .detail("entity failed")
            .causes(vec![cause.clone()])
            .build();
        assert_eq!(err.causes.len(), 1);
        assert_eq!(err.causes[0].detail, cause.detail);
    }

    // -----------------------------------------------------------------------
    // #111 — custom extension members
    // -----------------------------------------------------------------------

    #[cfg(feature = "serde")]
    #[test]
    fn with_extension_roundtrip() {
        let _g = lock_and_reset_mode();
        let err = ApiError::internal("boom").with_extension("trace_id", "abc-123");
        assert_eq!(err.extensions["trace_id"], "abc-123");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn extension_flattened_in_wire_format() {
        let _g = lock_and_reset_mode();
        let err = ApiError::not_found("gone").with_extension("tenant", "acme");
        let json = serde_json::to_value(&err).unwrap();
        // Extension must appear at the top level alongside standard fields.
        assert_eq!(json["tenant"], "acme");
        // Standard fields still present.
        assert_eq!(json["status"], 404);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn extension_roundtrip_ser_de() {
        let _g = lock_and_reset_mode();
        let err = ApiError::bad_request("bad").with_extension("request_num", 42_u64);
        let json = serde_json::to_value(&err).unwrap();
        let back: ApiError = serde_json::from_value(json).unwrap();
        assert_eq!(back.extensions["request_num"], 42_u64);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn extension_propagated_through_problem_json() {
        use crate::error::ProblemJson;
        let _g = lock_and_reset_mode();
        let err = ApiError::forbidden("denied").with_extension("policy", "read-only");
        let p = ProblemJson::from(err);
        assert_eq!(p.extensions["policy"], "read-only");
    }
}

// ---------------------------------------------------------------------------
// ProblemJson — RFC 7807 / 9457 wire-format response type
// ---------------------------------------------------------------------------

/// RFC 7807 / 9457 Problem Details response body with optional extension members.
///
/// Unlike [`ApiError`] (which carries in-process state such as `source` and
/// `Arc`), `ProblemJson` is a pure serialization type — every field maps
/// directly to the wire format.
///
/// The `extensions` map serializes **flat** into the JSON object, so arbitrary
/// key-value members (e.g. `trace_id`, `request_id`) appear at the top level
/// alongside the standard fields:
///
/// ```json
/// {
///   "type":     "urn:api-bones:error:resource-not-found",
///   "title":    "Resource Not Found",
///   "status":   404,
///   "detail":   "Booking 42 not found",
///   "instance": "urn:uuid:01234567-89ab-cdef-0123-456789abcdef",
///   "trace_id": "abc123"
/// }
/// ```
///
/// Content-Type is `application/problem+json`.
///
/// # `no_std` support
///
/// Available when either `std` or `alloc` is enabled together with `serde`
/// (required for `serde_json::Value` and `BTreeMap`).
/// Uses [`BTreeMap`](alloc::collections::BTreeMap) internally so heap
/// allocation is the only requirement — `std` is not needed.
///
/// # Examples
///
/// ```rust
/// use api_bones::error::{ApiError, ErrorCode, ProblemJson};
///
/// let err = ApiError::not_found("booking 42 not found");
/// let problem = ProblemJson::from(err);
/// assert_eq!(problem.status, 404);
/// assert_eq!(problem.title, "Resource Not Found");
/// ```
///
/// Adding extension members:
///
/// ```rust
/// use api_bones::error::{ApiError, ProblemJson};
///
/// let mut problem = ProblemJson::from(ApiError::internal("db timeout"));
/// problem.extensions.insert("trace_id".into(), "abc123".into());
/// assert_eq!(problem.extensions["trace_id"], "abc123");
/// ```
#[cfg(all(any(feature = "std", feature = "alloc"), feature = "serde"))]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "proptest", derive(proptest_derive::Arbitrary))]
pub struct ProblemJson {
    /// Machine-readable error type URI (RFC 9457 §3.1.1 `type`).
    #[cfg_attr(feature = "serde", serde(rename = "type"))]
    pub r#type: String,
    /// Human-friendly summary (RFC 9457 §3.1.2 `title`).
    pub title: String,
    /// HTTP status code (RFC 9457 §3.1.3 `status`).
    pub status: u16,
    /// Human-readable specifics (RFC 9457 §3.1.4 `detail`).
    pub detail: String,
    /// URI identifying this occurrence (RFC 9457 §3.1.5 `instance`).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub instance: Option<String>,
    /// Flat extension members (e.g. `trace_id`, `request_id`).
    ///
    /// Serialized **inline** at the top level of the JSON object via
    /// `#[serde(flatten)]`. Keys must not collide with the standard fields.
    /// Uses [`BTreeMap`](alloc::collections::BTreeMap) for `no_std` compatibility.
    #[cfg_attr(feature = "serde", serde(flatten))]
    #[cfg_attr(feature = "schemars", schemars(skip))]
    #[cfg_attr(feature = "arbitrary", arbitrary(default))]
    #[cfg_attr(
        feature = "proptest",
        proptest(strategy = "proptest::strategy::Just(BTreeMap::new())")
    )]
    pub extensions: BTreeMap<String, serde_json::Value>,
}

#[cfg(all(any(feature = "std", feature = "alloc"), feature = "serde"))]
impl ProblemJson {
    /// Build a `ProblemJson` directly from its components.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::error::ProblemJson;
    ///
    /// let p = ProblemJson::new(
    ///     "urn:api-bones:error:bad-request",
    ///     "Bad Request",
    ///     400,
    ///     "missing field `email`",
    /// );
    /// assert_eq!(p.status, 400);
    /// assert!(p.extensions.is_empty());
    /// ```
    #[must_use]
    pub fn new(
        r#type: impl Into<String>,
        title: impl Into<String>,
        status: u16,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            r#type: r#type.into(),
            title: title.into(),
            status,
            detail: detail.into(),
            instance: None,
            extensions: BTreeMap::new(),
        }
    }

    /// Set the `instance` field (RFC 9457 §3.1.5).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::error::ProblemJson;
    ///
    /// let p = ProblemJson::new("urn:api-bones:error:bad-request", "Bad Request", 400, "oops")
    ///     .with_instance("urn:uuid:00000000-0000-0000-0000-000000000000");
    /// assert!(p.instance.is_some());
    /// ```
    #[must_use]
    pub fn with_instance(mut self, instance: impl Into<String>) -> Self {
        self.instance = Some(instance.into());
        self
    }

    /// Insert an extension member.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::error::ProblemJson;
    ///
    /// let mut p = ProblemJson::new("urn:api-bones:error:bad-request", "Bad Request", 400, "oops");
    /// p.extend("trace_id", "abc123");
    /// assert_eq!(p.extensions["trace_id"], "abc123");
    /// ```
    pub fn extend(&mut self, key: impl Into<String>, value: impl Into<serde_json::Value>) {
        self.extensions.insert(key.into(), value.into());
    }
}

#[cfg(all(feature = "std", feature = "serde"))]
impl From<ApiError> for ProblemJson {
    /// Convert an [`ApiError`] into a `ProblemJson`.
    ///
    /// - `code` → `type` via [`ErrorCode::urn`]
    /// - `request_id` (UUID) → `instance` as `"urn:uuid:<id>"`
    /// - `errors` (validation) → `"errors"` extension member
    /// - `source` is dropped (not part of the wire format)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::error::{ApiError, ErrorCode, ProblemJson};
    ///
    /// let err = ApiError::new(ErrorCode::Forbidden, "not allowed");
    /// let p = ProblemJson::from(err);
    /// assert_eq!(p.status, 403);
    /// assert_eq!(p.title, "Forbidden");
    /// ```
    fn from(err: ApiError) -> Self {
        let mut p = Self::new(err.code.urn(), err.title, err.status, err.detail);

        #[cfg(feature = "uuid")]
        if let Some(id) = err.request_id {
            p.instance = Some(format!("urn:uuid:{id}"));
        }

        if !err.errors.is_empty() {
            let errs =
                serde_json::to_value(&err.errors).unwrap_or(serde_json::Value::Array(vec![]));
            p.extensions.insert("errors".into(), errs);
        }

        if let Some(info) = err.rate_limit
            && let Ok(v) = serde_json::to_value(&info)
        {
            p.extensions.insert("rate_limit".into(), v);
        }

        if !err.causes.is_empty() {
            let causes: Vec<serde_json::Value> = err
                .causes
                .into_iter()
                .map(|c| {
                    let cp = Self::from(c);
                    serde_json::to_value(cp).unwrap_or(serde_json::Value::Null)
                })
                .collect();
            p.extensions
                .insert("causes".into(), serde_json::Value::Array(causes));
        }

        // Merge caller-provided extensions last (they may intentionally
        // override the generated members above).
        for (k, v) in err.extensions {
            p.extensions.insert(k, v);
        }

        p
    }
}

#[cfg(all(feature = "std", feature = "serde", test))]
mod problem_json_tests {
    use super::*;

    #[test]
    fn new_sets_fields_and_empty_extensions() {
        let p = ProblemJson::new(
            "urn:api-bones:error:bad-request",
            "Bad Request",
            400,
            "missing email",
        );
        assert_eq!(p.r#type, "urn:api-bones:error:bad-request");
        assert_eq!(p.title, "Bad Request");
        assert_eq!(p.status, 400);
        assert_eq!(p.detail, "missing email");
        assert!(p.instance.is_none());
        assert!(p.extensions.is_empty());
    }

    #[test]
    fn with_instance_sets_instance() {
        let p = ProblemJson::new("urn:t", "T", 400, "d")
            .with_instance("urn:uuid:00000000-0000-0000-0000-000000000000");
        assert_eq!(
            p.instance.as_deref(),
            Some("urn:uuid:00000000-0000-0000-0000-000000000000")
        );
    }

    #[test]
    fn extend_inserts_entry() {
        let mut p = ProblemJson::new("urn:t", "T", 400, "d");
        p.extend("trace_id", "abc123");
        assert_eq!(p.extensions["trace_id"], "abc123");
    }

    #[test]
    fn from_api_error_maps_standard_fields() {
        #[cfg(feature = "std")]
        let _ = super::super::error_type_mode(); // ensure mode initialised
        let err = ApiError::new(ErrorCode::Forbidden, "not allowed");
        let p = ProblemJson::from(err);
        assert_eq!(p.status, 403);
        assert_eq!(p.title, "Forbidden");
        assert_eq!(p.detail, "not allowed");
    }

    #[test]
    fn from_api_error_maps_rate_limit_to_extension() {
        use crate::ratelimit::RateLimitInfo;
        let info = RateLimitInfo::new(100, 0, 1_700_000_000).retry_after(30);
        let err = ApiError::rate_limited_with(info);
        let p = ProblemJson::from(err);
        assert!(p.extensions.contains_key("rate_limit"));
        let rl = &p.extensions["rate_limit"];
        assert_eq!(rl["limit"], 100);
        assert_eq!(rl["remaining"], 0);
        assert_eq!(rl["reset"], 1_700_000_000_u64);
        assert_eq!(rl["retry_after"], 30);
    }

    #[test]
    fn api_error_rate_limit_serializes_inline() {
        use crate::ratelimit::RateLimitInfo;
        let err = ApiError::rate_limited(60)
            .with_rate_limit(RateLimitInfo::new(100, 0, 1_700_000_000).retry_after(60));
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["rate_limit"]["limit"], 100);
        assert_eq!(json["rate_limit"]["retry_after"], 60);
    }

    #[test]
    fn api_error_rate_limit_omitted_when_none() {
        let err = ApiError::bad_request("x");
        let json = serde_json::to_value(&err).unwrap();
        assert!(json.get("rate_limit").is_none());
    }

    #[test]
    fn from_api_error_maps_validation_errors_to_extension() {
        let err = ApiError::new(ErrorCode::ValidationFailed, "bad input").with_errors(vec![
            ValidationError {
                field: "/email".into(),
                message: "invalid".into(),
                rule: None,
            },
        ]);
        let p = ProblemJson::from(err);
        assert!(p.extensions.contains_key("errors"));
        let errs = p.extensions["errors"].as_array().unwrap();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0]["field"], "/email");
    }

    #[cfg(feature = "uuid")]
    #[test]
    fn from_api_error_maps_request_id_to_instance() {
        let id = uuid::Uuid::nil();
        let err = ApiError::new(ErrorCode::BadRequest, "x").with_request_id(id);
        let p = ProblemJson::from(err);
        assert_eq!(
            p.instance.as_deref(),
            Some("urn:uuid:00000000-0000-0000-0000-000000000000")
        );
    }

    #[test]
    fn serializes_extensions_flat() {
        let mut p = ProblemJson::new("urn:t", "T", 400, "d");
        p.extend("trace_id", "xyz");
        let json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        // extension must appear at top level, not nested
        assert_eq!(json["trace_id"], "xyz");
        assert!(json.get("extensions").is_none());
    }

    #[test]
    fn instance_omitted_when_none() {
        let p = ProblemJson::new("urn:t", "T", 400, "d");
        let json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert!(json.get("instance").is_none());
    }
}

// ---------------------------------------------------------------------------
// Axum IntoResponse integration
// ---------------------------------------------------------------------------

#[cfg(feature = "axum")]
mod axum_impl {
    use super::ApiError;
    use axum::response::{IntoResponse, Response};
    use http::{HeaderValue, StatusCode};

    impl IntoResponse for ApiError {
        fn into_response(self) -> Response {
            let status =
                StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            // ApiError contains only String/u16/Vec<String> fields — serialization
            // cannot fail, so expect() is safe here and avoids a dead branch.
            let body = serde_json::to_string(&self).expect("ApiError serialization is infallible");

            let mut response = (status, body).into_response();
            response.headers_mut().insert(
                http::header::CONTENT_TYPE,
                HeaderValue::from_static("application/problem+json"),
            );
            response
        }
    }

    #[cfg(all(any(feature = "std", feature = "alloc"), feature = "serde"))]
    impl IntoResponse for super::ProblemJson {
        fn into_response(self) -> Response {
            let status =
                StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            let body =
                serde_json::to_string(&self).expect("ProblemJson serialization is infallible");
            let mut response = (status, body).into_response();
            response.headers_mut().insert(
                http::header::CONTENT_TYPE,
                HeaderValue::from_static("application/problem+json"),
            );
            response
        }
    }
}

#[cfg(all(test, feature = "axum"))]
mod axum_tests {
    use super::*;
    use axum::response::IntoResponse;
    use http::StatusCode;

    #[tokio::test]
    async fn into_response_status_and_content_type() {
        reset_error_type_mode();
        let err = ApiError::not_found("thing 42 not found");
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/problem+json"
        );
    }

    #[tokio::test]
    async fn into_response_body() {
        reset_error_type_mode();
        let err = ApiError::unauthorized("bad token");
        let response = err.into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["type"], "urn:api-bones:error:unauthorized");
        assert_eq!(json["status"], 401);
        assert_eq!(json["detail"], "bad token");
    }

    #[cfg(feature = "utoipa")]
    #[test]
    fn error_code_schema_is_string_type() {
        use utoipa::PartialSchema as _;
        use utoipa::openapi::schema::Schema;

        let schema_ref = ErrorCode::schema();
        let schema = match schema_ref {
            utoipa::openapi::RefOr::T(s) => s,
            utoipa::openapi::RefOr::Ref(_) => panic!("expected inline schema"),
        };
        assert!(
            matches!(schema, Schema::Object(_)),
            "ErrorCode schema should be an object (string type)"
        );
    }

    #[cfg(feature = "utoipa")]
    #[test]
    fn error_code_schema_name() {
        use utoipa::ToSchema as _;
        assert_eq!(ErrorCode::name(), "ErrorCode");
    }

    #[cfg(feature = "serde")]
    #[tokio::test]
    async fn problem_json_into_response_status_and_content_type() {
        use super::ProblemJson;
        let p = ProblemJson::new("urn:api-bones:error:not-found", "Not Found", 404, "gone");
        let response = p.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/problem+json"
        );
    }

    #[cfg(feature = "serde")]
    #[tokio::test]
    async fn problem_json_into_response_body_with_extension() {
        use super::ProblemJson;
        let mut p = ProblemJson::new(
            "urn:api-bones:error:bad-request",
            "Bad Request",
            400,
            "missing field",
        );
        p.extend("trace_id", "abc123");
        let response = p.into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["type"], "urn:api-bones:error:bad-request");
        assert_eq!(json["status"], 400);
        assert_eq!(json["trace_id"], "abc123");
        assert!(json.get("extensions").is_none());
    }

    #[cfg(feature = "serde")]
    #[tokio::test]
    async fn problem_json_instance_omitted_when_none() {
        use super::ProblemJson;
        let p = ProblemJson::new("urn:t", "T", 500, "d");
        let response = p.into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json.get("instance").is_none());
    }

    // -----------------------------------------------------------------------
    // rate_limited_with — None retry_after branch
    // -----------------------------------------------------------------------

    #[test]
    fn rate_limited_with_no_retry_after() {
        use crate::ratelimit::RateLimitInfo;
        let info = RateLimitInfo::new(100, 5, 1_700_000_000);
        let err = ApiError::rate_limited_with(info);
        assert_eq!(err.status, 429);
        assert_eq!(err.detail, "Rate limited");
        assert!(err.rate_limit.is_some());
    }

}
