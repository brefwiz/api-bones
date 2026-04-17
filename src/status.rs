//! Full HTTP status code type covering 1xx/2xx/3xx/4xx/5xx.
//!
//! [`StatusCode`] extends the domain of [`ErrorCode`](crate::error::ErrorCode),
//! which only covers 4xx/5xx error responses, to include informational (1xx),
//! successful (2xx), and redirect (3xx) status codes as well.
//!
//! # Example
//!
//! ```rust
//! use api_bones::status::StatusCode;
//!
//! let sc = StatusCode::Ok;
//! assert_eq!(sc.as_u16(), 200);
//! assert!(sc.is_success());
//! assert!(!sc.is_error());
//!
//! let redirect = StatusCode::MovedPermanently;
//! assert!(redirect.is_redirection());
//!
//! let sc2: StatusCode = 201u16.try_into().unwrap();
//! assert_eq!(sc2, StatusCode::Created);
//! ```

use core::{fmt, str::FromStr};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::error::ErrorCode;

// ---------------------------------------------------------------------------
// StatusCode
// ---------------------------------------------------------------------------

/// All standard HTTP status codes (RFC 9110 + common extensions).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "proptest", derive(proptest_derive::Arbitrary))]
#[non_exhaustive]
pub enum StatusCode {
    // 1xx Informational
    /// 100 Continue
    Continue,
    /// 101 Switching Protocols
    SwitchingProtocols,
    /// 102 Processing (WebDAV)
    Processing,
    /// 103 Early Hints
    EarlyHints,

    // 2xx Success
    /// 200 OK
    Ok,
    /// 201 Created
    Created,
    /// 202 Accepted
    Accepted,
    /// 203 Non-Authoritative Information
    NonAuthoritativeInformation,
    /// 204 No Content
    NoContent,
    /// 205 Reset Content
    ResetContent,
    /// 206 Partial Content
    PartialContent,
    /// 207 Multi-Status (WebDAV)
    MultiStatus,
    /// 208 Already Reported (WebDAV)
    AlreadyReported,
    /// 226 IM Used
    ImUsed,

    // 3xx Redirection
    /// 300 Multiple Choices
    MultipleChoices,
    /// 301 Moved Permanently
    MovedPermanently,
    /// 302 Found
    Found,
    /// 303 See Other
    SeeOther,
    /// 304 Not Modified
    NotModified,
    /// 305 Use Proxy
    UseProxy,
    /// 307 Temporary Redirect
    TemporaryRedirect,
    /// 308 Permanent Redirect
    PermanentRedirect,

    // 4xx Client Error
    /// 400 Bad Request
    BadRequest,
    /// 401 Unauthorized
    Unauthorized,
    /// 402 Payment Required
    PaymentRequired,
    /// 403 Forbidden
    Forbidden,
    /// 404 Not Found
    NotFound,
    /// 405 Method Not Allowed
    MethodNotAllowed,
    /// 406 Not Acceptable
    NotAcceptable,
    /// 407 Proxy Authentication Required
    ProxyAuthenticationRequired,
    /// 408 Request Timeout
    RequestTimeout,
    /// 409 Conflict
    Conflict,
    /// 410 Gone
    Gone,
    /// 411 Length Required
    LengthRequired,
    /// 412 Precondition Failed
    PreconditionFailed,
    /// 413 Content Too Large
    ContentTooLarge,
    /// 414 URI Too Long
    UriTooLong,
    /// 415 Unsupported Media Type
    UnsupportedMediaType,
    /// 416 Range Not Satisfiable
    RangeNotSatisfiable,
    /// 417 Expectation Failed
    ExpectationFailed,
    /// 418 I'm a Teapot (RFC 2324)
    ImATeapot,
    /// 421 Misdirected Request
    MisdirectedRequest,
    /// 422 Unprocessable Content
    UnprocessableContent,
    /// 423 Locked (WebDAV)
    Locked,
    /// 424 Failed Dependency (WebDAV)
    FailedDependency,
    /// 425 Too Early
    TooEarly,
    /// 426 Upgrade Required
    UpgradeRequired,
    /// 428 Precondition Required
    PreconditionRequired,
    /// 429 Too Many Requests
    TooManyRequests,
    /// 431 Request Header Fields Too Large
    RequestHeaderFieldsTooLarge,
    /// 451 Unavailable For Legal Reasons
    UnavailableForLegalReasons,

    // 5xx Server Error
    /// 500 Internal Server Error
    InternalServerError,
    /// 501 Not Implemented
    NotImplemented,
    /// 502 Bad Gateway
    BadGateway,
    /// 503 Service Unavailable
    ServiceUnavailable,
    /// 504 Gateway Timeout
    GatewayTimeout,
    /// 505 HTTP Version Not Supported
    HttpVersionNotSupported,
    /// 506 Variant Also Negotiates
    VariantAlsoNegotiates,
    /// 507 Insufficient Storage (WebDAV)
    InsufficientStorage,
    /// 508 Loop Detected (WebDAV)
    LoopDetected,
    /// 510 Not Extended
    NotExtended,
    /// 511 Network Authentication Required
    NetworkAuthenticationRequired,
}

impl StatusCode {
    /// Return the numeric status code.
    ///
    /// ```
    /// use api_bones::status::StatusCode;
    ///
    /// assert_eq!(StatusCode::Ok.as_u16(), 200);
    /// assert_eq!(StatusCode::NotFound.as_u16(), 404);
    /// ```
    #[must_use]
    pub const fn as_u16(&self) -> u16 {
        match self {
            Self::Continue => 100,
            Self::SwitchingProtocols => 101,
            Self::Processing => 102,
            Self::EarlyHints => 103,
            Self::Ok => 200,
            Self::Created => 201,
            Self::Accepted => 202,
            Self::NonAuthoritativeInformation => 203,
            Self::NoContent => 204,
            Self::ResetContent => 205,
            Self::PartialContent => 206,
            Self::MultiStatus => 207,
            Self::AlreadyReported => 208,
            Self::ImUsed => 226,
            Self::MultipleChoices => 300,
            Self::MovedPermanently => 301,
            Self::Found => 302,
            Self::SeeOther => 303,
            Self::NotModified => 304,
            Self::UseProxy => 305,
            Self::TemporaryRedirect => 307,
            Self::PermanentRedirect => 308,
            Self::BadRequest => 400,
            Self::Unauthorized => 401,
            Self::PaymentRequired => 402,
            Self::Forbidden => 403,
            Self::NotFound => 404,
            Self::MethodNotAllowed => 405,
            Self::NotAcceptable => 406,
            Self::ProxyAuthenticationRequired => 407,
            Self::RequestTimeout => 408,
            Self::Conflict => 409,
            Self::Gone => 410,
            Self::LengthRequired => 411,
            Self::PreconditionFailed => 412,
            Self::ContentTooLarge => 413,
            Self::UriTooLong => 414,
            Self::UnsupportedMediaType => 415,
            Self::RangeNotSatisfiable => 416,
            Self::ExpectationFailed => 417,
            Self::ImATeapot => 418,
            Self::MisdirectedRequest => 421,
            Self::UnprocessableContent => 422,
            Self::Locked => 423,
            Self::FailedDependency => 424,
            Self::TooEarly => 425,
            Self::UpgradeRequired => 426,
            Self::PreconditionRequired => 428,
            Self::TooManyRequests => 429,
            Self::RequestHeaderFieldsTooLarge => 431,
            Self::UnavailableForLegalReasons => 451,
            Self::InternalServerError => 500,
            Self::NotImplemented => 501,
            Self::BadGateway => 502,
            Self::ServiceUnavailable => 503,
            Self::GatewayTimeout => 504,
            Self::HttpVersionNotSupported => 505,
            Self::VariantAlsoNegotiates => 506,
            Self::InsufficientStorage => 507,
            Self::LoopDetected => 508,
            Self::NotExtended => 510,
            Self::NetworkAuthenticationRequired => 511,
        }
    }

    /// Return the canonical reason phrase.
    ///
    /// ```
    /// use api_bones::status::StatusCode;
    ///
    /// assert_eq!(StatusCode::Ok.reason_phrase(), "OK");
    /// assert_eq!(StatusCode::NotFound.reason_phrase(), "Not Found");
    /// ```
    #[must_use]
    pub const fn reason_phrase(&self) -> &'static str {
        match self {
            Self::Continue => "Continue",
            Self::SwitchingProtocols => "Switching Protocols",
            Self::Processing => "Processing",
            Self::EarlyHints => "Early Hints",
            Self::Ok => "OK",
            Self::Created => "Created",
            Self::Accepted => "Accepted",
            Self::NonAuthoritativeInformation => "Non-Authoritative Information",
            Self::NoContent => "No Content",
            Self::ResetContent => "Reset Content",
            Self::PartialContent => "Partial Content",
            Self::MultiStatus => "Multi-Status",
            Self::AlreadyReported => "Already Reported",
            Self::ImUsed => "IM Used",
            Self::MultipleChoices => "Multiple Choices",
            Self::MovedPermanently => "Moved Permanently",
            Self::Found => "Found",
            Self::SeeOther => "See Other",
            Self::NotModified => "Not Modified",
            Self::UseProxy => "Use Proxy",
            Self::TemporaryRedirect => "Temporary Redirect",
            Self::PermanentRedirect => "Permanent Redirect",
            Self::BadRequest => "Bad Request",
            Self::Unauthorized => "Unauthorized",
            Self::PaymentRequired => "Payment Required",
            Self::Forbidden => "Forbidden",
            Self::NotFound => "Not Found",
            Self::MethodNotAllowed => "Method Not Allowed",
            Self::NotAcceptable => "Not Acceptable",
            Self::ProxyAuthenticationRequired => "Proxy Authentication Required",
            Self::RequestTimeout => "Request Timeout",
            Self::Conflict => "Conflict",
            Self::Gone => "Gone",
            Self::LengthRequired => "Length Required",
            Self::PreconditionFailed => "Precondition Failed",
            Self::ContentTooLarge => "Content Too Large",
            Self::UriTooLong => "URI Too Long",
            Self::UnsupportedMediaType => "Unsupported Media Type",
            Self::RangeNotSatisfiable => "Range Not Satisfiable",
            Self::ExpectationFailed => "Expectation Failed",
            Self::ImATeapot => "I'm a teapot",
            Self::MisdirectedRequest => "Misdirected Request",
            Self::UnprocessableContent => "Unprocessable Content",
            Self::Locked => "Locked",
            Self::FailedDependency => "Failed Dependency",
            Self::TooEarly => "Too Early",
            Self::UpgradeRequired => "Upgrade Required",
            Self::PreconditionRequired => "Precondition Required",
            Self::TooManyRequests => "Too Many Requests",
            Self::RequestHeaderFieldsTooLarge => "Request Header Fields Too Large",
            Self::UnavailableForLegalReasons => "Unavailable For Legal Reasons",
            Self::InternalServerError => "Internal Server Error",
            Self::NotImplemented => "Not Implemented",
            Self::BadGateway => "Bad Gateway",
            Self::ServiceUnavailable => "Service Unavailable",
            Self::GatewayTimeout => "Gateway Timeout",
            Self::HttpVersionNotSupported => "HTTP Version Not Supported",
            Self::VariantAlsoNegotiates => "Variant Also Negotiates",
            Self::InsufficientStorage => "Insufficient Storage",
            Self::LoopDetected => "Loop Detected",
            Self::NotExtended => "Not Extended",
            Self::NetworkAuthenticationRequired => "Network Authentication Required",
        }
    }

    // -----------------------------------------------------------------------
    // Category predicates
    // -----------------------------------------------------------------------

    /// Returns `true` for 1xx Informational responses.
    #[must_use]
    pub const fn is_informational(&self) -> bool {
        self.as_u16() / 100 == 1
    }

    /// Returns `true` for 2xx Successful responses.
    ///
    /// ```
    /// use api_bones::status::StatusCode;
    ///
    /// assert!(StatusCode::Ok.is_success());
    /// assert!(StatusCode::Created.is_success());
    /// assert!(!StatusCode::BadRequest.is_success());
    /// ```
    #[must_use]
    pub const fn is_success(&self) -> bool {
        self.as_u16() / 100 == 2
    }

    /// Returns `true` for 3xx Redirection responses.
    ///
    /// ```
    /// use api_bones::status::StatusCode;
    ///
    /// assert!(StatusCode::MovedPermanently.is_redirection());
    /// assert!(!StatusCode::Ok.is_redirection());
    /// ```
    #[must_use]
    pub const fn is_redirection(&self) -> bool {
        self.as_u16() / 100 == 3
    }

    /// Returns `true` for 4xx Client Error responses.
    ///
    /// ```
    /// use api_bones::status::StatusCode;
    ///
    /// assert!(StatusCode::NotFound.is_client_error());
    /// ```
    #[must_use]
    pub const fn is_client_error(&self) -> bool {
        self.as_u16() / 100 == 4
    }

    /// Returns `true` for 5xx Server Error responses.
    ///
    /// ```
    /// use api_bones::status::StatusCode;
    ///
    /// assert!(StatusCode::InternalServerError.is_server_error());
    /// ```
    #[must_use]
    pub const fn is_server_error(&self) -> bool {
        self.as_u16() / 100 == 5
    }

    /// Returns `true` for either 4xx or 5xx responses.
    #[must_use]
    pub const fn is_error(&self) -> bool {
        self.is_client_error() || self.is_server_error()
    }
}

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

impl fmt::Display for StatusCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.as_u16(), self.reason_phrase())
    }
}

// ---------------------------------------------------------------------------
// TryFrom<u16>
// ---------------------------------------------------------------------------

/// Error returned when converting an unknown numeric status code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownStatusCode(pub u16);

impl fmt::Display for UnknownStatusCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown HTTP status code: {}", self.0)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for UnknownStatusCode {}

impl TryFrom<u16> for StatusCode {
    type Error = UnknownStatusCode;

    fn try_from(code: u16) -> Result<Self, Self::Error> {
        match code {
            100 => Ok(Self::Continue),
            101 => Ok(Self::SwitchingProtocols),
            102 => Ok(Self::Processing),
            103 => Ok(Self::EarlyHints),
            200 => Ok(Self::Ok),
            201 => Ok(Self::Created),
            202 => Ok(Self::Accepted),
            203 => Ok(Self::NonAuthoritativeInformation),
            204 => Ok(Self::NoContent),
            205 => Ok(Self::ResetContent),
            206 => Ok(Self::PartialContent),
            207 => Ok(Self::MultiStatus),
            208 => Ok(Self::AlreadyReported),
            226 => Ok(Self::ImUsed),
            300 => Ok(Self::MultipleChoices),
            301 => Ok(Self::MovedPermanently),
            302 => Ok(Self::Found),
            303 => Ok(Self::SeeOther),
            304 => Ok(Self::NotModified),
            305 => Ok(Self::UseProxy),
            307 => Ok(Self::TemporaryRedirect),
            308 => Ok(Self::PermanentRedirect),
            400 => Ok(Self::BadRequest),
            401 => Ok(Self::Unauthorized),
            402 => Ok(Self::PaymentRequired),
            403 => Ok(Self::Forbidden),
            404 => Ok(Self::NotFound),
            405 => Ok(Self::MethodNotAllowed),
            406 => Ok(Self::NotAcceptable),
            407 => Ok(Self::ProxyAuthenticationRequired),
            408 => Ok(Self::RequestTimeout),
            409 => Ok(Self::Conflict),
            410 => Ok(Self::Gone),
            411 => Ok(Self::LengthRequired),
            412 => Ok(Self::PreconditionFailed),
            413 => Ok(Self::ContentTooLarge),
            414 => Ok(Self::UriTooLong),
            415 => Ok(Self::UnsupportedMediaType),
            416 => Ok(Self::RangeNotSatisfiable),
            417 => Ok(Self::ExpectationFailed),
            418 => Ok(Self::ImATeapot),
            421 => Ok(Self::MisdirectedRequest),
            422 => Ok(Self::UnprocessableContent),
            423 => Ok(Self::Locked),
            424 => Ok(Self::FailedDependency),
            425 => Ok(Self::TooEarly),
            426 => Ok(Self::UpgradeRequired),
            428 => Ok(Self::PreconditionRequired),
            429 => Ok(Self::TooManyRequests),
            431 => Ok(Self::RequestHeaderFieldsTooLarge),
            451 => Ok(Self::UnavailableForLegalReasons),
            500 => Ok(Self::InternalServerError),
            501 => Ok(Self::NotImplemented),
            502 => Ok(Self::BadGateway),
            503 => Ok(Self::ServiceUnavailable),
            504 => Ok(Self::GatewayTimeout),
            505 => Ok(Self::HttpVersionNotSupported),
            506 => Ok(Self::VariantAlsoNegotiates),
            507 => Ok(Self::InsufficientStorage),
            508 => Ok(Self::LoopDetected),
            510 => Ok(Self::NotExtended),
            511 => Ok(Self::NetworkAuthenticationRequired),
            other => Err(UnknownStatusCode(other)),
        }
    }
}

impl FromStr for StatusCode {
    type Err = UnknownStatusCode;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let n: u16 = s.trim().parse().map_err(|_| UnknownStatusCode(0))?;
        Self::try_from(n)
    }
}

// ---------------------------------------------------------------------------
// Conversion from/to ErrorCode
// ---------------------------------------------------------------------------

impl From<ErrorCode> for StatusCode {
    fn from(code: ErrorCode) -> Self {
        match code {
            ErrorCode::BadRequest | ErrorCode::ValidationFailed => Self::BadRequest,
            ErrorCode::Unauthorized
            | ErrorCode::InvalidCredentials
            | ErrorCode::TokenExpired
            | ErrorCode::TokenInvalid => Self::Unauthorized,
            ErrorCode::Forbidden | ErrorCode::InsufficientPermissions => Self::Forbidden,
            ErrorCode::ResourceNotFound => Self::NotFound,
            ErrorCode::MethodNotAllowed => Self::MethodNotAllowed,
            ErrorCode::NotAcceptable => Self::NotAcceptable,
            ErrorCode::RequestTimeout => Self::RequestTimeout,
            ErrorCode::Conflict | ErrorCode::ResourceAlreadyExists => Self::Conflict,
            ErrorCode::Gone => Self::Gone,
            ErrorCode::PreconditionFailed => Self::PreconditionFailed,
            ErrorCode::PayloadTooLarge => Self::ContentTooLarge,
            ErrorCode::UnsupportedMediaType => Self::UnsupportedMediaType,
            ErrorCode::UnprocessableEntity => Self::UnprocessableContent,
            ErrorCode::PreconditionRequired => Self::PreconditionRequired,
            ErrorCode::RateLimited => Self::TooManyRequests,
            ErrorCode::RequestHeaderFieldsTooLarge => Self::RequestHeaderFieldsTooLarge,
            ErrorCode::InternalServerError => Self::InternalServerError,
            ErrorCode::NotImplemented => Self::NotImplemented,
            ErrorCode::BadGateway => Self::BadGateway,
            ErrorCode::ServiceUnavailable => Self::ServiceUnavailable,
            ErrorCode::GatewayTimeout => Self::GatewayTimeout,
        }
    }
}

// ---------------------------------------------------------------------------
// Interop with `http` crate
// ---------------------------------------------------------------------------

#[cfg(feature = "http")]
mod http_interop {
    use super::{StatusCode, UnknownStatusCode};

    impl TryFrom<http::StatusCode> for StatusCode {
        type Error = UnknownStatusCode;

        fn try_from(sc: http::StatusCode) -> Result<Self, Self::Error> {
            Self::try_from(sc.as_u16())
        }
    }

    impl TryFrom<StatusCode> for http::StatusCode {
        type Error = http::status::InvalidStatusCode;

        fn try_from(sc: StatusCode) -> Result<Self, Self::Error> {
            Self::from_u16(sc.as_u16())
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_round_trip() {
        let codes: &[u16] = &[
            100, 101, 102, 103, 200, 201, 202, 203, 204, 205, 206, 207, 208, 226, 300, 301, 302,
            303, 304, 305, 307, 308, 400, 401, 402, 403, 404, 405, 406, 407, 408, 409, 410, 411,
            412, 413, 414, 415, 416, 417, 418, 421, 422, 423, 424, 425, 426, 428, 429, 431, 451,
            500, 501, 502, 503, 504, 505, 506, 507, 508, 510, 511,
        ];
        for &code in codes {
            let sc = StatusCode::try_from(code).expect("should be known");
            assert_eq!(sc.as_u16(), code, "round-trip failed for {code}");
        }
    }

    #[test]
    fn unknown_status_errors() {
        assert!(StatusCode::try_from(0u16).is_err());
        assert!(StatusCode::try_from(600u16).is_err());
        let err = StatusCode::try_from(999u16).unwrap_err();
        assert_eq!(err.0, 999);
        assert!(err.to_string().contains("999"));
    }

    #[test]
    fn from_str_round_trip() {
        assert_eq!("200".parse::<StatusCode>().unwrap(), StatusCode::Ok);
        assert_eq!(" 404 ".parse::<StatusCode>().unwrap(), StatusCode::NotFound);
        assert!("abc".parse::<StatusCode>().is_err());
        assert!("999".parse::<StatusCode>().is_err());
    }

    #[test]
    fn reason_phrases_spot_check() {
        assert_eq!(StatusCode::Continue.reason_phrase(), "Continue");
        assert_eq!(StatusCode::ImUsed.reason_phrase(), "IM Used");
        assert_eq!(StatusCode::ImATeapot.reason_phrase(), "I'm a teapot");
        assert_eq!(
            StatusCode::NetworkAuthenticationRequired.reason_phrase(),
            "Network Authentication Required"
        );
    }

    #[test]
    fn category_predicates_all_ranges() {
        assert!(StatusCode::Processing.is_informational());
        assert!(StatusCode::EarlyHints.is_informational());
        assert!(StatusCode::Accepted.is_success());
        assert!(StatusCode::ImUsed.is_success());
        assert!(StatusCode::PermanentRedirect.is_redirection());
        assert!(StatusCode::Gone.is_client_error());
        assert!(StatusCode::UnavailableForLegalReasons.is_client_error());
        assert!(StatusCode::GatewayTimeout.is_server_error());
        assert!(StatusCode::NetworkAuthenticationRequired.is_server_error());
    }

    #[test]
    fn category_predicates() {
        assert!(StatusCode::Continue.is_informational());
        assert!(!StatusCode::Continue.is_success());

        assert!(StatusCode::Ok.is_success());
        assert!(StatusCode::Created.is_success());
        assert!(!StatusCode::Ok.is_error());

        assert!(StatusCode::MovedPermanently.is_redirection());
        assert!(!StatusCode::Ok.is_redirection());

        assert!(StatusCode::NotFound.is_client_error());
        assert!(StatusCode::NotFound.is_error());
        assert!(!StatusCode::NotFound.is_server_error());

        assert!(StatusCode::InternalServerError.is_server_error());
        assert!(StatusCode::InternalServerError.is_error());
    }

    #[test]
    fn display_format() {
        assert_eq!(StatusCode::Ok.to_string(), "200 OK");
        assert_eq!(StatusCode::NotFound.to_string(), "404 Not Found");
    }

    #[test]
    fn from_error_code() {
        assert_eq!(
            StatusCode::from(ErrorCode::ResourceNotFound),
            StatusCode::NotFound
        );
        assert_eq!(
            StatusCode::from(ErrorCode::RateLimited),
            StatusCode::TooManyRequests
        );
        assert_eq!(
            StatusCode::from(ErrorCode::InternalServerError),
            StatusCode::InternalServerError
        );
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_round_trip() {
        let sc = StatusCode::Created;
        let json = serde_json::to_string(&sc).unwrap();
        let back: StatusCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, sc);
    }

    #[cfg(feature = "http")]
    #[test]
    fn http_crate_round_trip() {
        let sc = StatusCode::Ok;
        let hsc: http::StatusCode = sc.try_into().unwrap();
        assert_eq!(hsc, http::StatusCode::OK);
        let back: StatusCode = hsc.try_into().unwrap();
        assert_eq!(back, StatusCode::Ok);
    }
}
