//! Fluent URL and query-string builders.
//!
//! # [`UrlBuilder`]
//!
//! A fluent builder for constructing URLs from scheme, host, path segments,
//! query parameters, and fragment. Path segments are percent-encoded; query
//! values are form-encoded.
//!
//! ```rust
//! use api_bones::url::UrlBuilder;
//!
//! let url = UrlBuilder::new()
//!     .scheme("https")
//!     .host("api.example.com")
//!     .path("v1")
//!     .path("users")
//!     .path("42")
//!     .query("active", "true")
//!     .build();
//!
//! assert_eq!(url, "https://api.example.com/v1/users/42?active=true");
//! ```
//!
//! # [`QueryBuilder`]
//!
//! A standalone query-string builder with typed `Display` values and optional
//! merge into an existing URL.
//!
//! ```rust
//! use api_bones::url::QueryBuilder;
//!
//! let qs = QueryBuilder::new()
//!     .param("limit", 20u32)
//!     .param("sort", "desc")
//!     .build();
//! assert_eq!(qs, "limit=20&sort=desc");
//! ```

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use core::fmt;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Percent-encoding helpers
// ---------------------------------------------------------------------------

/// Percent-encode a string using the path-segment allowed set (RFC 3986 §3.3).
///
/// Unreserved characters (`A-Z a-z 0-9 - . _ ~`) and sub-delimiters
/// (`: @ ! $ & ' ( ) * + , ; =`) are left as-is. Everything else is encoded.
#[must_use]
fn percent_encode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'.'
                    | b'_'
                    | b'~'
                    | b':'
                    | b'@'
                    | b'!'
                    | b'$'
                    | b'&'
                    | b'\''
                    | b'('
                    | b')'
                    | b'*'
                    | b'+'
                    | b','
                    | b';'
                    | b'='
            )
        {
            out.push(byte as char);
        } else {
            let _ = core::fmt::write(&mut out, format_args!("%{byte:02X}"));
        }
    }
    out
}

/// Percent-encode a query key or value (application/x-www-form-urlencoded style).
///
/// Space is encoded as `+`; everything else outside the unreserved set is `%XX`.
#[must_use]
fn percent_encode_query(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b' ' => out.push('+'),
            b if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') => {
                out.push(byte as char);
            }
            _ => {
                let _ = core::fmt::write(&mut out, format_args!("%{byte:02X}"));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// UrlBuilder
// ---------------------------------------------------------------------------

/// Fluent URL builder.
///
/// Build a URL incrementally by chaining setter methods, then call [`build`](Self::build)
/// to produce the final `String`.
///
/// Path segments are automatically percent-encoded. Query parameters are
/// form-encoded. No validation of scheme or host is performed — this is a
/// string-composition helper, not a full URL parser.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct UrlBuilder {
    scheme: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    segments: Vec<String>,
    query: Vec<(String, String)>,
    fragment: Option<String>,
}

impl UrlBuilder {
    /// Create an empty `UrlBuilder`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the URL scheme (e.g. `"https"`).
    ///
    /// # Examples
    ///
    /// ```
    /// use api_bones::url::UrlBuilder;
    ///
    /// let url = UrlBuilder::new().scheme("https").host("example.com").build();
    /// assert_eq!(url, "https://example.com");
    /// ```
    #[must_use]
    pub fn scheme(mut self, scheme: impl Into<String>) -> Self {
        self.scheme = Some(scheme.into());
        self
    }

    /// Set the host (e.g. `"api.example.com"`).
    #[must_use]
    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.host = Some(host.into());
        self
    }

    /// Set an optional port number.
    ///
    /// # Examples
    ///
    /// ```
    /// use api_bones::url::UrlBuilder;
    ///
    /// let url = UrlBuilder::new()
    ///     .scheme("http")
    ///     .host("localhost")
    ///     .port(8080)
    ///     .build();
    /// assert_eq!(url, "http://localhost:8080");
    /// ```
    #[must_use]
    pub fn port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    /// Append a path segment (will be percent-encoded).
    ///
    /// Call multiple times to build up `/a/b/c` style paths.
    ///
    /// # Examples
    ///
    /// ```
    /// use api_bones::url::UrlBuilder;
    ///
    /// let url = UrlBuilder::new()
    ///     .scheme("https")
    ///     .host("example.com")
    ///     .path("v1")
    ///     .path("users")
    ///     .path("hello world")
    ///     .build();
    /// assert_eq!(url, "https://example.com/v1/users/hello%20world");
    /// ```
    #[must_use]
    pub fn path(mut self, segment: impl Into<String>) -> Self {
        self.segments.push(segment.into());
        self
    }

    /// Append a query parameter (key and value are form-encoded).
    ///
    /// # Examples
    ///
    /// ```
    /// use api_bones::url::UrlBuilder;
    ///
    /// let url = UrlBuilder::new()
    ///     .scheme("https")
    ///     .host("example.com")
    ///     .query("q", "hello world")
    ///     .build();
    /// assert_eq!(url, "https://example.com?q=hello+world");
    /// ```
    #[must_use]
    #[allow(clippy::needless_pass_by_value)]
    pub fn query(mut self, key: impl Into<String>, value: impl ToString) -> Self {
        self.query.push((key.into(), value.to_string()));
        self
    }

    /// Set the URL fragment (the part after `#`).
    ///
    /// # Examples
    ///
    /// ```
    /// use api_bones::url::UrlBuilder;
    ///
    /// let url = UrlBuilder::new()
    ///     .scheme("https")
    ///     .host("example.com")
    ///     .fragment("section-1")
    ///     .build();
    /// assert_eq!(url, "https://example.com#section-1");
    /// ```
    #[must_use]
    pub fn fragment(mut self, fragment: impl Into<String>) -> Self {
        self.fragment = Some(fragment.into());
        self
    }

    /// Produce the final URL string.
    ///
    /// # Examples
    ///
    /// ```
    /// use api_bones::url::UrlBuilder;
    ///
    /// let url = UrlBuilder::new()
    ///     .scheme("https")
    ///     .host("api.example.com")
    ///     .path("v1")
    ///     .path("items")
    ///     .query("page", 2u32)
    ///     .fragment("top")
    ///     .build();
    ///
    /// assert_eq!(url, "https://api.example.com/v1/items?page=2#top");
    /// ```
    #[must_use]
    pub fn build(&self) -> String {
        let mut out = String::new();

        // scheme://host[:port]
        if let Some(scheme) = &self.scheme {
            out.push_str(scheme);
            out.push_str("://");
        }
        if let Some(host) = &self.host {
            out.push_str(host);
        }
        if let Some(port) = self.port {
            let _ = core::fmt::write(&mut out, format_args!(":{port}"));
        }

        // /path/segments
        for seg in &self.segments {
            out.push('/');
            out.push_str(&percent_encode_path(seg));
        }

        // ?key=value&…
        for (i, (k, v)) in self.query.iter().enumerate() {
            out.push(if i == 0 { '?' } else { '&' });
            out.push_str(&percent_encode_query(k));
            out.push('=');
            out.push_str(&percent_encode_query(v));
        }

        // #fragment
        if let Some(frag) = &self.fragment {
            out.push('#');
            out.push_str(frag);
        }

        out
    }
}

impl fmt::Display for UrlBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.build())
    }
}

// ---------------------------------------------------------------------------
// QueryBuilder
// ---------------------------------------------------------------------------

/// Standalone query-string builder with typed `Display` values.
///
/// Produces `key=value` pairs separated by `&`, with form-encoding applied to
/// both key and value. Use [`merge_into`](Self::merge_into) to append the
/// query string to an existing URL.
///
/// # Examples
///
/// ```
/// use api_bones::url::QueryBuilder;
///
/// let qs = QueryBuilder::new()
///     .param("limit", 20u32)
///     .param("sort", "desc")
///     .build();
/// assert_eq!(qs, "limit=20&sort=desc");
/// ```
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct QueryBuilder {
    params: Vec<(String, String)>,
}

impl QueryBuilder {
    /// Create an empty `QueryBuilder`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a typed query parameter.
    ///
    /// The value is converted to a string via [`Display`](core::fmt::Display).
    ///
    /// # Examples
    ///
    /// ```
    /// use api_bones::url::QueryBuilder;
    ///
    /// let qs = QueryBuilder::new().param("active", true).build();
    /// assert_eq!(qs, "active=true");
    /// ```
    #[must_use]
    #[allow(clippy::needless_pass_by_value)]
    pub fn param(mut self, key: impl Into<String>, value: impl ToString) -> Self {
        self.params.push((key.into(), value.to_string()));
        self
    }

    /// Append an optional parameter — skipped if `value` is `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use api_bones::url::QueryBuilder;
    ///
    /// let qs = QueryBuilder::new()
    ///     .param("a", 1u32)
    ///     .maybe_param("b", None::<&str>)
    ///     .build();
    /// assert_eq!(qs, "a=1");
    /// ```
    #[must_use]
    pub fn maybe_param(self, key: impl Into<String>, value: Option<impl ToString>) -> Self {
        match value {
            Some(v) => self.param(key, v),
            None => self,
        }
    }

    /// Build the query string (without leading `?`).
    ///
    /// Returns an empty string when no parameters have been added.
    #[must_use]
    pub fn build(&self) -> String {
        let mut out = String::new();
        for (i, (k, v)) in self.params.iter().enumerate() {
            if i > 0 {
                out.push('&');
            }
            out.push_str(&percent_encode_query(k));
            out.push('=');
            out.push_str(&percent_encode_query(v));
        }
        out
    }

    /// Append the query string to `url`, using `?` if there is no existing
    /// query, or `&` if one already exists.
    ///
    /// Returns `url` unchanged when there are no params.
    ///
    /// # Examples
    ///
    /// ```
    /// use api_bones::url::QueryBuilder;
    ///
    /// let qs = QueryBuilder::new().param("page", 2u32);
    /// assert_eq!(qs.merge_into("https://example.com"), "https://example.com?page=2");
    /// assert_eq!(qs.merge_into("https://example.com?limit=20"), "https://example.com?limit=20&page=2");
    /// ```
    #[must_use]
    pub fn merge_into(&self, url: &str) -> String {
        let qs = self.build();
        if qs.is_empty() {
            return url.to_string();
        }
        let sep = if url.contains('?') { '&' } else { '?' };
        let mut out = String::with_capacity(url.len() + 1 + qs.len());
        out.push_str(url);
        out.push(sep);
        out.push_str(&qs);
        out
    }

    /// Return `true` when no parameters have been added.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.params.is_empty()
    }
}

impl fmt::Display for QueryBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.build())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- percent_encode_path ---

    #[test]
    fn encode_path_alphanumeric_unchanged() {
        assert_eq!(percent_encode_path("hello123"), "hello123");
    }

    #[test]
    fn encode_path_space_encoded() {
        assert_eq!(percent_encode_path("hello world"), "hello%20world");
    }

    #[test]
    fn encode_path_slash_encoded() {
        assert_eq!(percent_encode_path("a/b"), "a%2Fb");
    }

    // --- percent_encode_query ---

    #[test]
    fn encode_query_space_as_plus() {
        assert_eq!(percent_encode_query("hello world"), "hello+world");
    }

    #[test]
    fn encode_query_ampersand() {
        assert_eq!(percent_encode_query("a&b"), "a%26b");
    }

    // --- UrlBuilder ---

    #[test]
    fn full_url() {
        let url = UrlBuilder::new()
            .scheme("https")
            .host("api.example.com")
            .path("v1")
            .path("users")
            .path("42")
            .query("active", "true")
            .fragment("top")
            .build();
        assert_eq!(url, "https://api.example.com/v1/users/42?active=true#top");
    }

    #[test]
    fn url_with_port() {
        let url = UrlBuilder::new()
            .scheme("http")
            .host("localhost")
            .port(8080)
            .path("health")
            .build();
        assert_eq!(url, "http://localhost:8080/health");
    }

    #[test]
    fn url_path_encoding() {
        let url = UrlBuilder::new()
            .scheme("https")
            .host("example.com")
            .path("hello world")
            .build();
        assert_eq!(url, "https://example.com/hello%20world");
    }

    #[test]
    fn url_multiple_query_params() {
        let url = UrlBuilder::new()
            .scheme("https")
            .host("example.com")
            .query("a", 1u32)
            .query("b", 2u32)
            .build();
        assert_eq!(url, "https://example.com?a=1&b=2");
    }

    #[test]
    fn url_no_scheme_no_host() {
        let url = UrlBuilder::new().path("v1").path("items").build();
        assert_eq!(url, "/v1/items");
    }

    #[test]
    fn display_matches_build() {
        let b = UrlBuilder::new().scheme("https").host("example.com");
        assert_eq!(b.to_string(), b.build());
    }

    // --- QueryBuilder ---

    #[test]
    fn query_builder_basic() {
        let qs = QueryBuilder::new()
            .param("limit", 20u32)
            .param("sort", "desc")
            .build();
        assert_eq!(qs, "limit=20&sort=desc");
    }

    #[test]
    fn query_builder_empty() {
        let qs = QueryBuilder::new().build();
        assert!(qs.is_empty());
    }

    #[test]
    fn query_builder_maybe_param_some() {
        let qs = QueryBuilder::new()
            .maybe_param("after", Some("cursor123"))
            .build();
        assert_eq!(qs, "after=cursor123");
    }

    #[test]
    fn query_builder_maybe_param_none() {
        let qs = QueryBuilder::new()
            .param("a", 1u32)
            .maybe_param("b", None::<&str>)
            .build();
        assert_eq!(qs, "a=1");
    }

    #[test]
    fn merge_into_no_existing_query() {
        let qs = QueryBuilder::new().param("page", 2u32);
        assert_eq!(
            qs.merge_into("https://example.com"),
            "https://example.com?page=2"
        );
    }

    #[test]
    fn merge_into_existing_query() {
        let qs = QueryBuilder::new().param("page", 2u32);
        assert_eq!(
            qs.merge_into("https://example.com?limit=20"),
            "https://example.com?limit=20&page=2"
        );
    }

    #[test]
    fn merge_into_empty_returns_url_unchanged() {
        let qs = QueryBuilder::new();
        assert_eq!(qs.merge_into("https://example.com"), "https://example.com");
    }

    #[test]
    fn query_builder_url_encodes_special_chars() {
        let qs = QueryBuilder::new().param("q", "hello world&more").build();
        assert_eq!(qs, "q=hello+world%26more");
    }

    // --- UrlBuilder Default ---

    #[test]
    fn url_builder_default_produces_empty_string() {
        let b = UrlBuilder::default();
        assert_eq!(b.build(), "");
    }

    // --- QueryBuilder Display ---

    #[test]
    fn query_builder_display_matches_build() {
        let qb = QueryBuilder::new()
            .param("limit", 10u32)
            .param("sort", "asc");
        assert_eq!(qb.to_string(), qb.build());
    }

    // --- QueryBuilder is_empty ---

    #[test]
    fn query_builder_is_empty_true_when_no_params() {
        assert!(QueryBuilder::new().is_empty());
    }

    #[test]
    fn query_builder_is_empty_false_after_param() {
        assert!(!QueryBuilder::new().param("k", "v").is_empty());
    }

    // --- merge_into with empty QueryBuilder ---

    #[test]
    fn merge_into_empty_no_change() {
        let qb = QueryBuilder::default();
        assert_eq!(
            qb.merge_into("https://example.com/path"),
            "https://example.com/path"
        );
    }
}
