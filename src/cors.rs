//! Typed helpers for CORS response headers (`Access-Control-*`).
//!
//! [`CorsHeaders`] models the complete set of CORS response headers defined in
//! the Fetch specification. A fluent builder API makes it easy to construct
//! both simple and preflight responses.
//!
//! # Example
//!
//! ```rust
//! use api_bones::cors::{CorsHeaders, CorsOrigin};
//!
//! // Simple CORS response.
//! let cors = CorsHeaders::new()
//!     .allow_origin(CorsOrigin::Any)
//!     .allow_methods(["GET", "POST"])
//!     .allow_headers(["Content-Type", "Authorization"])
//!     .max_age(86_400);
//!
//! assert_eq!(cors.allow_origin.as_ref().unwrap().to_string(), "*");
//! assert_eq!(cors.max_age, Some(86_400));
//!
//! // Preflight response helper.
//! let preflight = CorsHeaders::preflight(
//!     CorsOrigin::Origin("https://example.com".into()),
//!     ["GET", "POST", "DELETE"],
//!     ["Content-Type"],
//! );
//! assert!(preflight.allow_credentials.is_none() || preflight.allow_credentials == Some(false));
//! ```

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{
    string::String,
    vec::Vec,
};
use core::fmt;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// CorsOrigin
// ---------------------------------------------------------------------------

/// The value of the `Access-Control-Allow-Origin` header.
///
/// - [`CorsOrigin::Any`] — `*`
/// - [`CorsOrigin::Origin(url)`] — a specific origin URL
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum CorsOrigin {
    /// `Access-Control-Allow-Origin: *`
    Any,
    /// `Access-Control-Allow-Origin: <url>`
    Origin(String),
}

impl fmt::Display for CorsOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Any => f.write_str("*"),
            Self::Origin(url) => f.write_str(url),
        }
    }
}

// ---------------------------------------------------------------------------
// CorsHeaders
// ---------------------------------------------------------------------------

/// Structured CORS response headers.
///
/// All fields are `Option` so that headers can be omitted when not needed.
/// Use the builder methods to set individual fields.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct CorsHeaders {
    /// `Access-Control-Allow-Origin`
    pub allow_origin: Option<CorsOrigin>,
    /// `Access-Control-Allow-Methods`
    pub allow_methods: Option<Vec<String>>,
    /// `Access-Control-Allow-Headers`
    pub allow_headers: Option<Vec<String>>,
    /// `Access-Control-Expose-Headers`
    pub expose_headers: Option<Vec<String>>,
    /// `Access-Control-Max-Age` (seconds)
    pub max_age: Option<u64>,
    /// `Access-Control-Allow-Credentials`
    pub allow_credentials: Option<bool>,
}

impl CorsHeaders {
    /// Create a new, empty `CorsHeaders`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    // -----------------------------------------------------------------------
    // Builder methods
    // -----------------------------------------------------------------------

    /// Set `Access-Control-Allow-Origin`.
    ///
    /// ```
    /// use api_bones::cors::{CorsHeaders, CorsOrigin};
    ///
    /// let cors = CorsHeaders::new().allow_origin(CorsOrigin::Any);
    /// assert_eq!(cors.allow_origin.unwrap().to_string(), "*");
    /// ```
    #[must_use]
    pub fn allow_origin(mut self, origin: CorsOrigin) -> Self {
        self.allow_origin = Some(origin);
        self
    }

    /// Set `Access-Control-Allow-Methods` from an iterator of method strings.
    ///
    /// ```
    /// use api_bones::cors::CorsHeaders;
    ///
    /// let cors = CorsHeaders::new().allow_methods(["GET", "POST"]);
    /// let methods = cors.allow_methods.unwrap();
    /// assert!(methods.contains(&"GET".to_string()));
    /// ```
    #[must_use]
    pub fn allow_methods<I>(mut self, methods: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        self.allow_methods = Some(methods.into_iter().map(Into::into).collect());
        self
    }

    /// Set `Access-Control-Allow-Headers` from an iterator of header names.
    #[must_use]
    pub fn allow_headers<I>(mut self, headers: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        self.allow_headers = Some(headers.into_iter().map(Into::into).collect());
        self
    }

    /// Set `Access-Control-Expose-Headers` from an iterator of header names.
    #[must_use]
    pub fn expose_headers<I>(mut self, headers: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        self.expose_headers = Some(headers.into_iter().map(Into::into).collect());
        self
    }

    /// Set `Access-Control-Max-Age` (seconds).
    #[must_use]
    pub fn max_age(mut self, seconds: u64) -> Self {
        self.max_age = Some(seconds);
        self
    }

    /// Set `Access-Control-Allow-Credentials`.
    ///
    /// Note: per the spec, `Allow-Credentials: true` is incompatible with
    /// `Allow-Origin: *`. This is not enforced at the type level but callers
    /// should be careful.
    #[must_use]
    pub fn allow_credentials(mut self, allow: bool) -> Self {
        self.allow_credentials = Some(allow);
        self
    }

    // -----------------------------------------------------------------------
    // Convenience constructors
    // -----------------------------------------------------------------------

    /// Build a preflight (`OPTIONS`) response with sensible defaults.
    ///
    /// Sets `Allow-Origin`, `Allow-Methods`, and `Allow-Headers`. Does not set
    /// `Allow-Credentials` (default: absent, treated as `false` by browsers).
    ///
    /// ```
    /// use api_bones::cors::{CorsHeaders, CorsOrigin};
    ///
    /// let preflight = CorsHeaders::preflight(
    ///     CorsOrigin::Origin("https://example.com".into()),
    ///     ["GET", "POST"],
    ///     ["Content-Type"],
    /// );
    /// assert!(preflight.allow_methods.is_some());
    /// assert!(preflight.allow_headers.is_some());
    /// ```
    #[must_use]
    pub fn preflight<M, H>(origin: CorsOrigin, methods: M, headers: H) -> Self
    where
        M: IntoIterator,
        M::Item: Into<String>,
        H: IntoIterator,
        H::Item: Into<String>,
    {
        Self::new()
            .allow_origin(origin)
            .allow_methods(methods)
            .allow_headers(headers)
    }

    // -----------------------------------------------------------------------
    // Header value accessors
    // -----------------------------------------------------------------------

    /// Render the `Access-Control-Allow-Methods` value as a comma-separated string.
    ///
    /// Returns `None` if the field is not set.
    #[must_use]
    pub fn allow_methods_header(&self) -> Option<String> {
        self.allow_methods.as_ref().map(|m| m.join(", "))
    }

    /// Render the `Access-Control-Allow-Headers` value as a comma-separated string.
    ///
    /// Returns `None` if the field is not set.
    #[must_use]
    pub fn allow_headers_header(&self) -> Option<String> {
        self.allow_headers.as_ref().map(|h| h.join(", "))
    }

    /// Render the `Access-Control-Expose-Headers` value as a comma-separated string.
    ///
    /// Returns `None` if the field is not set.
    #[must_use]
    pub fn expose_headers_header(&self) -> Option<String> {
        self.expose_headers.as_ref().map(|h| h.join(", "))
    }
}

// ---------------------------------------------------------------------------
// Axum integration
// ---------------------------------------------------------------------------

#[cfg(feature = "axum")]
mod axum_support {
    use super::CorsHeaders;
    use axum::http::HeaderValue;
    use axum::response::{IntoResponseParts, ResponseParts};

    impl IntoResponseParts for CorsHeaders {
        type Error = std::convert::Infallible;

        fn into_response_parts(self, mut res: ResponseParts) -> Result<ResponseParts, Self::Error> {
            let headers = res.headers_mut();

            if let Some(origin) = &self.allow_origin
                && let Ok(v) = HeaderValue::from_str(&origin.to_string())
            {
                headers.insert("access-control-allow-origin", v);
            }
            if let Some(methods) = &self.allow_methods
                && let Ok(v) = HeaderValue::from_str(&methods.join(", "))
            {
                headers.insert("access-control-allow-methods", v);
            }
            if let Some(hdrs) = &self.allow_headers
                && let Ok(v) = HeaderValue::from_str(&hdrs.join(", "))
            {
                headers.insert("access-control-allow-headers", v);
            }
            if let Some(expose) = &self.expose_headers
                && let Ok(v) = HeaderValue::from_str(&expose.join(", "))
            {
                headers.insert("access-control-expose-headers", v);
            }
            if let Some(max_age) = self.max_age
                && let Ok(v) = HeaderValue::from_str(&max_age.to_string())
            {
                headers.insert("access-control-max-age", v);
            }
            if let Some(creds) = self.allow_credentials {
                let val = if creds { "true" } else { "false" };
                let v = HeaderValue::from_static(val);
                headers.insert("access-control-allow-credentials", v);
            }

            Ok(res)
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
    fn default_all_none() {
        let cors = CorsHeaders::new();
        assert!(cors.allow_origin.is_none());
        assert!(cors.allow_methods.is_none());
        assert!(cors.allow_headers.is_none());
        assert!(cors.expose_headers.is_none());
        assert!(cors.max_age.is_none());
        assert!(cors.allow_credentials.is_none());
    }

    #[test]
    fn builder_allow_origin_any() {
        let cors = CorsHeaders::new().allow_origin(CorsOrigin::Any);
        assert_eq!(cors.allow_origin.unwrap().to_string(), "*");
    }

    #[test]
    fn builder_allow_origin_specific() {
        let cors =
            CorsHeaders::new().allow_origin(CorsOrigin::Origin("https://example.com".into()));
        assert_eq!(
            cors.allow_origin.unwrap().to_string(),
            "https://example.com"
        );
    }

    #[test]
    fn builder_allow_methods() {
        let cors = CorsHeaders::new().allow_methods(["GET", "POST", "DELETE"]);
        let methods = cors.allow_methods.unwrap();
        assert!(methods.contains(&"GET".to_string()));
        assert!(methods.contains(&"POST".to_string()));
        assert_eq!(methods.len(), 3);
    }

    #[test]
    fn builder_allow_headers() {
        let cors = CorsHeaders::new().allow_headers(["Content-Type", "Authorization"]);
        let hdrs = cors.allow_headers.unwrap();
        assert!(hdrs.contains(&"Content-Type".to_string()));
    }

    #[test]
    fn builder_expose_headers() {
        let cors = CorsHeaders::new().expose_headers(["X-Request-Id"]);
        assert_eq!(cors.expose_headers_header().unwrap(), "X-Request-Id");
    }

    #[test]
    fn builder_max_age() {
        let cors = CorsHeaders::new().max_age(3600);
        assert_eq!(cors.max_age, Some(3600));
    }

    #[test]
    fn builder_allow_credentials() {
        let cors = CorsHeaders::new().allow_credentials(true);
        assert_eq!(cors.allow_credentials, Some(true));
    }

    #[test]
    fn header_value_accessors() {
        let cors = CorsHeaders::new()
            .allow_methods(["GET", "POST"])
            .allow_headers(["Content-Type"]);
        assert_eq!(cors.allow_methods_header().unwrap(), "GET, POST");
        assert_eq!(cors.allow_headers_header().unwrap(), "Content-Type");
        assert!(cors.expose_headers_header().is_none());
    }

    #[test]
    fn preflight_constructor() {
        let p = CorsHeaders::preflight(
            CorsOrigin::Origin("https://app.example.com".into()),
            ["GET", "POST"],
            ["Content-Type", "Authorization"],
        );
        assert!(p.allow_origin.is_some());
        assert_eq!(p.allow_methods.as_ref().unwrap().len(), 2);
        assert_eq!(p.allow_headers.as_ref().unwrap().len(), 2);
        assert!(p.allow_credentials.is_none());
    }

    #[test]
    fn cors_origin_display() {
        assert_eq!(CorsOrigin::Any.to_string(), "*");
        assert_eq!(
            CorsOrigin::Origin("https://x.com".into()).to_string(),
            "https://x.com"
        );
    }

    #[cfg(feature = "axum")]
    #[test]
    fn into_response_parts_sets_headers() {
        use axum::response::IntoResponse;

        let cors = CorsHeaders::new()
            .allow_origin(CorsOrigin::Any)
            .allow_methods(["GET"])
            .max_age(600);

        let response = (cors, axum::http::StatusCode::NO_CONTENT).into_response();
        let headers = response.headers();

        assert_eq!(
            headers
                .get("access-control-allow-origin")
                .unwrap()
                .to_str()
                .unwrap(),
            "*"
        );
        assert_eq!(
            headers
                .get("access-control-max-age")
                .unwrap()
                .to_str()
                .unwrap(),
            "600"
        );
    }
}
