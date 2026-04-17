//! Authentication scheme types, credential newtypes, `Authorization` header
//! parser/builder, and scope/permission types for RBAC/ABAC.
//!
//! # Feature gate
//!
//! This module is only available when the **`auth`** feature is enabled.
//!
//! # Overview
//!
//! | Type                    | Purpose                                              |
//! |-------------------------|------------------------------------------------------|
//! | [`AuthScheme`]          | Enum of well-known HTTP auth schemes (Bearer, Basic, …) |
//! | [`BearerToken`]         | Opaque bearer token (zeroized on drop)               |
//! | [`BasicCredentials`]    | Username + password pair (password zeroized on drop) |
//! | [`ApiKeyCredentials`]   | API key credential (zeroized on drop)                |
//! | [`OAuth2Token`]         | OAuth 2.0 access token with optional token type      |
//! | [`AuthorizationHeader`] | Parse/build `Authorization:` header values           |
//! | [`Scope`]               | Space-delimited OAuth 2.0 scope string with set ops  |
//! | [`Permission`]          | Single permission token for RBAC/ABAC                |
//!
//! # Example
//!
//! ```rust
//! use api_bones::auth::{AuthorizationHeader, BearerToken};
//!
//! let header = "Bearer my-secret-token";
//! let auth: AuthorizationHeader = header.parse().unwrap();
//!
//! // Round-trip
//! assert_eq!(auth.to_string(), header);
//!
//! // Pattern-match on the parsed credential
//! if let AuthorizationHeader::Bearer(tok) = &auth {
//!     assert_eq!(tok.as_str(), "my-secret-token");
//! }
//! ```

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{
    borrow::ToOwned,
    collections::BTreeSet,
    format,
    string::{String, ToString},
    vec::Vec,
};
#[cfg(feature = "std")]
use std::collections::BTreeSet;
use core::{fmt, str::FromStr};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop};

// ---------------------------------------------------------------------------
// AuthScheme (#90)
// ---------------------------------------------------------------------------

/// An HTTP authentication scheme.
///
/// Well-known schemes are represented by dedicated variants; any other scheme
/// string is captured by `Custom(String)`.
///
/// # Examples
///
/// ```
/// use api_bones::auth::AuthScheme;
/// use core::str::FromStr;
///
/// assert_eq!(AuthScheme::from_str("Bearer").unwrap(), AuthScheme::Bearer);
/// assert_eq!(AuthScheme::Bearer.to_string(), "Bearer");
///
/// let custom = AuthScheme::from_str("NTLM").unwrap();
/// assert_eq!(custom, AuthScheme::Custom("NTLM".to_owned()));
/// assert_eq!(custom.to_string(), "NTLM");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "scheme", content = "value"))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum AuthScheme {
    /// RFC 6750 Bearer token.
    Bearer,
    /// RFC 7617 Basic authentication (base64-encoded user:password).
    Basic,
    /// API key passed in the `Authorization` header.
    ApiKey,
    /// OAuth 2.0 token (see [`AuthorizationHeader::OAuth2`]).
    OAuth2,
    /// RFC 7616 Digest authentication.
    Digest,
    /// Any other scheme not listed above.
    Custom(String),
}

impl fmt::Display for AuthScheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bearer => f.write_str("Bearer"),
            Self::Basic => f.write_str("Basic"),
            Self::ApiKey => f.write_str("ApiKey"),
            Self::OAuth2 => f.write_str("OAuth2"),
            Self::Digest => f.write_str("Digest"),
            Self::Custom(s) => f.write_str(s),
        }
    }
}

impl FromStr for AuthScheme {
    type Err = core::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "Bearer" | "bearer" => Self::Bearer,
            "Basic" | "basic" => Self::Basic,
            "ApiKey" | "apikey" | "APIKEY" => Self::ApiKey,
            "OAuth2" | "oauth2" | "OAuth" => Self::OAuth2,
            "Digest" | "digest" => Self::Digest,
            other => Self::Custom(other.to_owned()),
        })
    }
}

// ---------------------------------------------------------------------------
// Credential types (#91)
// ---------------------------------------------------------------------------

/// An opaque Bearer token.
///
/// The inner string is zeroized on drop to limit the time the secret is
/// resident in memory. The `Debug` implementation redacts the value.
///
/// ```
/// use api_bones::auth::BearerToken;
///
/// let tok = BearerToken::new("secret");
/// assert_eq!(tok.as_str(), "secret");
/// // Debug does not leak the value:
/// assert_eq!(format!("{tok:?}"), "BearerToken(\"[REDACTED]\")");
/// ```
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct BearerToken(String);

impl BearerToken {
    /// Construct a new `BearerToken` from any string-like value.
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }

    /// Return the raw token string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for BearerToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("BearerToken")
            .field(&"[REDACTED]")
            .finish()
    }
}

impl fmt::Display for BearerToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl PartialEq for BearerToken {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for BearerToken {}

// ---------------------------------------------------------------------------

/// Username + password credentials for HTTP Basic authentication.
///
/// The `password` field is zeroized on drop. `Debug` redacts the password.
///
/// ```
/// use api_bones::auth::BasicCredentials;
///
/// let creds = BasicCredentials::new("alice", "s3cr3t");
/// assert_eq!(creds.username(), "alice");
/// // Debug redacts password:
/// let dbg = format!("{creds:?}");
/// assert!(dbg.contains("[REDACTED]"));
/// assert!(!dbg.contains("s3cr3t"));
/// ```
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct BasicCredentials {
    username: String,
    password: String,
}

impl BasicCredentials {
    /// Construct new Basic credentials.
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
        }
    }

    /// Return the username.
    #[must_use]
    pub fn username(&self) -> &str {
        &self.username
    }

    /// Return the password.
    #[must_use]
    pub fn password(&self) -> &str {
        &self.password
    }
}

impl fmt::Debug for BasicCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BasicCredentials")
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .finish()
    }
}

impl PartialEq for BasicCredentials {
    fn eq(&self, other: &Self) -> bool {
        self.username == other.username && self.password == other.password
    }
}

impl Eq for BasicCredentials {}

// ---------------------------------------------------------------------------

/// An API key passed in an `Authorization` header.
///
/// The key is zeroized on drop and redacted in `Debug`.
///
/// ```
/// use api_bones::auth::ApiKeyCredentials;
///
/// let key = ApiKeyCredentials::new("abcd-1234");
/// assert_eq!(key.as_str(), "abcd-1234");
/// assert!(format!("{key:?}").contains("[REDACTED]"));
/// ```
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ApiKeyCredentials(String);

impl ApiKeyCredentials {
    /// Construct a new `ApiKeyCredentials` value.
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    /// Return the raw key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKeyCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ApiKeyCredentials")
            .field(&"[REDACTED]")
            .finish()
    }
}

impl PartialEq for ApiKeyCredentials {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for ApiKeyCredentials {}

// ---------------------------------------------------------------------------

/// An OAuth 2.0 access token, optionally carrying the token type string.
///
/// The access token value is zeroized on drop and redacted in `Debug`.
///
/// ```
/// use api_bones::auth::OAuth2Token;
///
/// let tok = OAuth2Token::new("super-secret-tok", Some("Bearer"));
/// assert_eq!(tok.token_type(), Some("Bearer"));
/// assert!(format!("{tok:?}").contains("[REDACTED]"));
/// assert!(!format!("{tok:?}").contains("super-secret-tok"));
/// ```
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct OAuth2Token {
    access_token: String,
    token_type: Option<String>,
}

impl OAuth2Token {
    /// Construct a new `OAuth2Token`.
    pub fn new(access_token: impl Into<String>, token_type: Option<impl Into<String>>) -> Self {
        Self {
            access_token: access_token.into(),
            token_type: token_type.map(Into::into),
        }
    }

    /// Return the raw access token string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.access_token
    }

    /// Return the token type, if any was provided.
    #[must_use]
    pub fn token_type(&self) -> Option<&str> {
        self.token_type.as_deref()
    }
}

impl fmt::Debug for OAuth2Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuth2Token")
            .field("access_token", &"[REDACTED]")
            .field("token_type", &self.token_type)
            .finish()
    }
}

impl PartialEq for OAuth2Token {
    fn eq(&self, other: &Self) -> bool {
        self.access_token == other.access_token && self.token_type == other.token_type
    }
}

impl Eq for OAuth2Token {}

// ---------------------------------------------------------------------------
// AuthorizationHeader (#92)
// ---------------------------------------------------------------------------

/// Error variants for malformed `Authorization` header values.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ParseAuthorizationError {
    /// The header value was empty.
    #[error("authorization header is empty")]
    Empty,
    /// No space separator between the scheme and credentials was found.
    #[error("authorization header is missing the credentials after the scheme")]
    MissingCredentials,
    /// The Basic credentials are not valid base64.
    #[error("basic credentials are not valid base64: {0}")]
    InvalidBase64(String),
    /// The Basic credentials do not contain a `:` separator.
    #[error("basic credentials must contain a ':' separator between username and password")]
    InvalidBasicFormat,
    /// The decoded Basic credentials are not valid UTF-8.
    #[error("basic credentials are not valid UTF-8")]
    InvalidUtf8,
}

/// A parsed `Authorization:` HTTP request-header value.
///
/// Supports round-trip parsing and serialization for all common schemes.
/// Basic credentials are base64-decoded on parse and re-encoded on display.
///
/// # Examples
///
/// ```
/// use api_bones::auth::{AuthorizationHeader, BasicCredentials};
///
/// // Bearer
/// let a: AuthorizationHeader = "Bearer tok123".parse().unwrap();
/// assert_eq!(a.to_string(), "Bearer tok123");
///
/// // Basic — credentials are decoded from base64
/// let b: AuthorizationHeader = "Basic dXNlcjpwYXNz".parse().unwrap();
/// if let AuthorizationHeader::Basic(creds) = &b {
///     assert_eq!(creds.username(), "user");
///     assert_eq!(creds.password(), "pass");
/// }
/// // Round-trips back to the same base64-encoded form
/// assert_eq!(b.to_string(), "Basic dXNlcjpwYXNz");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorizationHeader {
    /// `Bearer <token>`
    Bearer(BearerToken),
    /// `Basic <base64(user:pass)>`
    Basic(BasicCredentials),
    /// `ApiKey <key>`
    ApiKey(ApiKeyCredentials),
    /// `OAuth2 <access_token>`
    OAuth2(OAuth2Token),
    /// Any other scheme: `<scheme> <credentials>`
    Other {
        /// The auth scheme string.
        scheme: String,
        /// The raw credentials string following the scheme.
        credentials: String,
    },
}

impl AuthorizationHeader {
    /// Return the [`AuthScheme`] variant for this header value.
    #[must_use]
    pub fn scheme(&self) -> AuthScheme {
        match self {
            Self::Bearer(_) => AuthScheme::Bearer,
            Self::Basic(_) => AuthScheme::Basic,
            Self::ApiKey(_) => AuthScheme::ApiKey,
            Self::OAuth2(_) => AuthScheme::OAuth2,
            Self::Other { scheme, .. } => AuthScheme::Custom(scheme.clone()),
        }
    }
}

impl fmt::Display for AuthorizationHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bearer(tok) => write!(f, "Bearer {}", tok.as_str()),
            Self::Basic(creds) => {
                use base64::Engine as _;
                let plain = format!("{}:{}", creds.username(), creds.password());
                let encoded = base64::engine::general_purpose::STANDARD.encode(plain.as_bytes());
                write!(f, "Basic {encoded}")
            }
            Self::ApiKey(key) => write!(f, "ApiKey {}", key.as_str()),
            Self::OAuth2(tok) => write!(f, "OAuth2 {}", tok.as_str()),
            Self::Other {
                scheme,
                credentials,
            } => write!(f, "{scheme} {credentials}"),
        }
    }
}

impl FromStr for AuthorizationHeader {
    type Err = ParseAuthorizationError;

    /// Parse an `Authorization` header value.
    ///
    /// # Errors
    ///
    /// Returns [`ParseAuthorizationError`] when the input is empty, missing
    /// credentials, or (for Basic) contains malformed base64 or UTF-8.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.is_empty() {
            return Err(ParseAuthorizationError::Empty);
        }
        let (scheme_str, credentials) = s
            .split_once(' ')
            .ok_or(ParseAuthorizationError::MissingCredentials)?;
        let credentials = credentials.trim();
        if credentials.is_empty() {
            return Err(ParseAuthorizationError::MissingCredentials);
        }

        match scheme_str {
            "Bearer" | "bearer" => Ok(Self::Bearer(BearerToken::new(credentials))),
            "Basic" | "basic" => {
                use base64::Engine as _;
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(credentials.as_bytes())
                    .map_err(|e| ParseAuthorizationError::InvalidBase64(e.to_string()))?;
                let plain = core::str::from_utf8(&decoded)
                    .map_err(|_| ParseAuthorizationError::InvalidUtf8)?;
                let (user, pass) = plain
                    .split_once(':')
                    .ok_or(ParseAuthorizationError::InvalidBasicFormat)?;
                Ok(Self::Basic(BasicCredentials::new(user, pass)))
            }
            "ApiKey" | "apikey" | "APIKEY" => {
                Ok(Self::ApiKey(ApiKeyCredentials::new(credentials)))
            }
            "OAuth2" | "oauth2" | "OAuth" => {
                Ok(Self::OAuth2(OAuth2Token::new(credentials, None::<String>)))
            }
            other => Ok(Self::Other {
                scheme: other.to_owned(),
                credentials: credentials.to_owned(),
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Scope / Permission (#93)
// ---------------------------------------------------------------------------

/// A single authorization permission token, used in RBAC/ABAC systems.
///
/// A `Permission` is a non-empty string token such as `"read"`, `"orders:write"`,
/// or `"admin"`. It cannot contain ASCII whitespace.
///
/// ```
/// use api_bones::auth::{Permission, ParsePermissionError};
///
/// let p = Permission::new("orders:read").unwrap();
/// assert_eq!(p.as_str(), "orders:read");
///
/// assert!(matches!(Permission::new(""), Err(ParsePermissionError::Empty)));
/// assert!(matches!(Permission::new("bad token"), Err(ParsePermissionError::ContainsWhitespace)));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Permission(String);

/// Error returned when constructing a [`Permission`] fails.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ParsePermissionError {
    /// The input was empty.
    #[error("permission must not be empty")]
    Empty,
    /// The input contains ASCII whitespace, which is not allowed.
    #[error("permission must not contain whitespace")]
    ContainsWhitespace,
}

impl Permission {
    /// Construct a `Permission` from a string.
    ///
    /// # Errors
    ///
    /// Returns [`ParsePermissionError::Empty`] for empty input, or
    /// [`ParsePermissionError::ContainsWhitespace`] if the token contains spaces/tabs.
    pub fn new(s: impl AsRef<str>) -> Result<Self, ParsePermissionError> {
        let s = s.as_ref();
        if s.is_empty() {
            return Err(ParsePermissionError::Empty);
        }
        if s.chars().any(|c| c.is_ascii_whitespace()) {
            return Err(ParsePermissionError::ContainsWhitespace);
        }
        Ok(Self(s.to_owned()))
    }

    /// Return the inner permission string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Permission {
    type Err = ParsePermissionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

// ---------------------------------------------------------------------------

/// A set of OAuth 2.0 scopes (space-delimited tokens per RFC 6749 §3.3).
///
/// Internally the scopes are stored as a sorted, deduplicated
/// [`BTreeSet`] of [`Permission`] values.
///
/// # Examples
///
/// ```
/// use api_bones::auth::Scope;
///
/// let s: Scope = "read write openid".parse().unwrap();
/// assert_eq!(s.len(), 3);
/// assert!(s.contains("read"));
///
/// // Serialize back to space-delimited string (sorted order)
/// let rendered = s.to_string();
/// assert!(rendered.contains("read"));
/// assert!(rendered.contains("write"));
/// assert!(rendered.contains("openid"));
///
/// // Subset check
/// let narrow: Scope = "read".parse().unwrap();
/// assert!(narrow.is_subset_of(&s));
/// assert!(!s.is_subset_of(&narrow));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(
    feature = "serde",
    serde(
        try_from = "String",
        into = "String"
    )
)]
pub struct Scope(BTreeSet<Permission>);

/// Error returned when parsing a [`Scope`] fails.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid scope token: {0}")]
pub struct ParseScopeError(#[from] ParsePermissionError);

impl Scope {
    /// Construct an empty `Scope`.
    #[must_use]
    pub fn empty() -> Self {
        Self(BTreeSet::new())
    }

    /// Construct a `Scope` from an iterator of [`Permission`] values.
    pub fn from_permissions(perms: impl IntoIterator<Item = Permission>) -> Self {
        Self(perms.into_iter().collect())
    }

    /// Return the number of permission tokens in this scope.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Return `true` if this scope contains no tokens.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Return `true` if this scope contains the given permission string.
    ///
    /// ```
    /// use api_bones::auth::Scope;
    ///
    /// let s: Scope = "read write".parse().unwrap();
    /// assert!(s.contains("read"));
    /// assert!(!s.contains("admin"));
    /// ```
    #[must_use]
    pub fn contains(&self, perm: &str) -> bool {
        self.0.iter().any(|p| p.as_str() == perm)
    }

    /// Return `true` if every token in `self` is also present in `other`.
    ///
    /// ```
    /// use api_bones::auth::Scope;
    ///
    /// let full: Scope = "read write admin".parse().unwrap();
    /// let partial: Scope = "read write".parse().unwrap();
    /// assert!(partial.is_subset_of(&full));
    /// assert!(!full.is_subset_of(&partial));
    /// ```
    #[must_use]
    pub fn is_subset_of(&self, other: &Self) -> bool {
        self.0.iter().all(|p| other.0.contains(p))
    }

    /// Return a new `Scope` that is the union of `self` and `other`.
    ///
    /// ```
    /// use api_bones::auth::Scope;
    ///
    /// let a: Scope = "read".parse().unwrap();
    /// let b: Scope = "write".parse().unwrap();
    /// let c = a.union(&b);
    /// assert!(c.contains("read"));
    /// assert!(c.contains("write"));
    /// ```
    #[must_use]
    pub fn union(&self, other: &Self) -> Self {
        Self(self.0.iter().chain(other.0.iter()).cloned().collect())
    }

    /// Return a new `Scope` containing only tokens present in both `self` and `other`.
    ///
    /// ```
    /// use api_bones::auth::Scope;
    ///
    /// let a: Scope = "read write".parse().unwrap();
    /// let b: Scope = "write admin".parse().unwrap();
    /// let c = a.intersection(&b);
    /// assert_eq!(c.len(), 1);
    /// assert!(c.contains("write"));
    /// ```
    #[must_use]
    pub fn intersection(&self, other: &Self) -> Self {
        Self(
            self.0
                .iter()
                .filter(|p| other.0.contains(*p))
                .cloned()
                .collect(),
        )
    }

    /// Iterate over the permissions in this scope (sorted order).
    pub fn iter(&self) -> impl Iterator<Item = &Permission> {
        self.0.iter()
    }
}

impl fmt::Display for Scope {
    /// Serialize as a space-delimited string (alphabetically sorted).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for p in &self.0 {
            if !first {
                f.write_str(" ")?;
            }
            f.write_str(p.as_str())?;
            first = false;
        }
        Ok(())
    }
}

impl FromStr for Scope {
    type Err = ParseScopeError;

    /// Parse a space-delimited OAuth 2.0 scope string.
    ///
    /// # Errors
    ///
    /// Returns [`ParseScopeError`] if any token is invalid (currently only the
    /// empty-string token can trigger this, since whitespace is used as the
    /// delimiter and consecutive spaces produce empty tokens).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.is_empty() {
            return Ok(Self::empty());
        }
        let perms = s
            .split_ascii_whitespace()
            .map(Permission::new)
            .collect::<Result<BTreeSet<_>, _>>()?;
        Ok(Self(perms))
    }
}

// Needed for `serde(into = "String")`
impl From<Scope> for String {
    fn from(s: Scope) -> Self {
        s.to_string()
    }
}

// Needed for `serde(try_from = "String")`
impl TryFrom<String> for Scope {
    type Error = ParseScopeError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // AuthScheme
    // -----------------------------------------------------------------------

    #[test]
    fn auth_scheme_display() {
        assert_eq!(AuthScheme::Bearer.to_string(), "Bearer");
        assert_eq!(AuthScheme::Basic.to_string(), "Basic");
        assert_eq!(AuthScheme::ApiKey.to_string(), "ApiKey");
        assert_eq!(AuthScheme::OAuth2.to_string(), "OAuth2");
        assert_eq!(AuthScheme::Digest.to_string(), "Digest");
        assert_eq!(
            AuthScheme::Custom("NTLM".to_owned()).to_string(),
            "NTLM"
        );
    }

    #[test]
    fn auth_scheme_from_str_known() {
        assert_eq!("Bearer".parse::<AuthScheme>().unwrap(), AuthScheme::Bearer);
        assert_eq!("bearer".parse::<AuthScheme>().unwrap(), AuthScheme::Bearer);
        assert_eq!("Basic".parse::<AuthScheme>().unwrap(), AuthScheme::Basic);
        assert_eq!("Digest".parse::<AuthScheme>().unwrap(), AuthScheme::Digest);
    }

    #[test]
    fn auth_scheme_from_str_custom() {
        let s = "NTLM".parse::<AuthScheme>().unwrap();
        assert_eq!(s, AuthScheme::Custom("NTLM".to_owned()));
    }

    // -----------------------------------------------------------------------
    // BearerToken
    // -----------------------------------------------------------------------

    #[test]
    fn bearer_token_as_str() {
        let t = BearerToken::new("tok");
        assert_eq!(t.as_str(), "tok");
    }

    #[test]
    fn bearer_token_debug_redacts() {
        let t = BearerToken::new("super-secret");
        let dbg = format!("{t:?}");
        assert!(dbg.contains("[REDACTED]"));
        assert!(!dbg.contains("super-secret"));
    }

    #[test]
    fn bearer_token_display() {
        let t = BearerToken::new("tok");
        assert_eq!(t.to_string(), "tok");
    }

    // -----------------------------------------------------------------------
    // BasicCredentials
    // -----------------------------------------------------------------------

    #[test]
    fn basic_credentials_fields() {
        let c = BasicCredentials::new("alice", "s3cr3t");
        assert_eq!(c.username(), "alice");
        assert_eq!(c.password(), "s3cr3t");
    }

    #[test]
    fn basic_credentials_debug_redacts_password() {
        let c = BasicCredentials::new("alice", "s3cr3t");
        let dbg = format!("{c:?}");
        assert!(dbg.contains("alice"));
        assert!(dbg.contains("[REDACTED]"));
        assert!(!dbg.contains("s3cr3t"));
    }

    // -----------------------------------------------------------------------
    // ApiKeyCredentials
    // -----------------------------------------------------------------------

    #[test]
    fn api_key_as_str() {
        let k = ApiKeyCredentials::new("key-123");
        assert_eq!(k.as_str(), "key-123");
    }

    #[test]
    fn api_key_debug_redacts() {
        let k = ApiKeyCredentials::new("key-123");
        let dbg = format!("{k:?}");
        assert!(dbg.contains("[REDACTED]"));
        assert!(!dbg.contains("key-123"));
    }

    // -----------------------------------------------------------------------
    // OAuth2Token
    // -----------------------------------------------------------------------

    #[test]
    fn oauth2_token_fields() {
        let t = OAuth2Token::new("access", Some("Bearer"));
        assert_eq!(t.as_str(), "access");
        assert_eq!(t.token_type(), Some("Bearer"));
    }

    #[test]
    fn oauth2_token_debug_redacts() {
        let t = OAuth2Token::new("super-secret-tok", Some("Bearer"));
        let dbg = format!("{t:?}");
        assert!(dbg.contains("[REDACTED]"));
        assert!(!dbg.contains("super-secret-tok"));
    }

    // -----------------------------------------------------------------------
    // AuthorizationHeader parsing
    // -----------------------------------------------------------------------

    #[test]
    fn parse_bearer() {
        let h: AuthorizationHeader = "Bearer mytoken".parse().unwrap();
        assert_eq!(h, AuthorizationHeader::Bearer(BearerToken::new("mytoken")));
        assert_eq!(h.to_string(), "Bearer mytoken");
    }

    #[test]
    fn parse_basic_roundtrip() {
        // "user:pass" base64 = "dXNlcjpwYXNz"
        let h: AuthorizationHeader = "Basic dXNlcjpwYXNz".parse().unwrap();
        if let AuthorizationHeader::Basic(c) = &h {
            assert_eq!(c.username(), "user");
            assert_eq!(c.password(), "pass");
        } else {
            panic!("expected Basic");
        }
        assert_eq!(h.to_string(), "Basic dXNlcjpwYXNz");
    }

    #[test]
    fn parse_apikey() {
        let h: AuthorizationHeader = "ApiKey abc-def".parse().unwrap();
        assert_eq!(
            h,
            AuthorizationHeader::ApiKey(ApiKeyCredentials::new("abc-def"))
        );
        assert_eq!(h.to_string(), "ApiKey abc-def");
    }

    #[test]
    fn parse_oauth2() {
        let h: AuthorizationHeader = "OAuth2 access123".parse().unwrap();
        assert_eq!(h.to_string(), "OAuth2 access123");
    }

    #[test]
    fn parse_other_scheme() {
        let h: AuthorizationHeader = "NTLM TlRMTVNTUAAB".parse().unwrap();
        assert_eq!(h.to_string(), "NTLM TlRMTVNTUAAB");
        assert_eq!(h.scheme(), AuthScheme::Custom("NTLM".to_owned()));
    }

    #[test]
    fn parse_empty_is_error() {
        assert_eq!(
            "".parse::<AuthorizationHeader>(),
            Err(ParseAuthorizationError::Empty)
        );
    }

    #[test]
    fn parse_missing_credentials_is_error() {
        assert_eq!(
            "Bearer".parse::<AuthorizationHeader>(),
            Err(ParseAuthorizationError::MissingCredentials)
        );
    }

    #[test]
    fn parse_invalid_base64_is_error() {
        assert!(matches!(
            "Basic !!!".parse::<AuthorizationHeader>(),
            Err(ParseAuthorizationError::InvalidBase64(_))
        ));
    }

    #[test]
    fn parse_basic_missing_colon_is_error() {
        // "userpass" without colon (valid base64 of "userpass")
        use base64::Engine as _;
        let encoded =
            base64::engine::general_purpose::STANDARD.encode(b"userpass");
        let input = format!("Basic {encoded}");
        assert_eq!(
            input.parse::<AuthorizationHeader>(),
            Err(ParseAuthorizationError::InvalidBasicFormat)
        );
    }

    // -----------------------------------------------------------------------
    // Permission
    // -----------------------------------------------------------------------

    #[test]
    fn permission_valid() {
        let p = Permission::new("orders:read").unwrap();
        assert_eq!(p.as_str(), "orders:read");
    }

    #[test]
    fn permission_empty_is_error() {
        assert_eq!(Permission::new(""), Err(ParsePermissionError::Empty));
    }

    #[test]
    fn permission_whitespace_is_error() {
        assert_eq!(
            Permission::new("bad token"),
            Err(ParsePermissionError::ContainsWhitespace)
        );
    }

    #[test]
    fn permission_display_and_from_str_roundtrip() {
        let p = Permission::new("admin").unwrap();
        let s = p.to_string();
        let back: Permission = s.parse().unwrap();
        assert_eq!(back, p);
    }

    // -----------------------------------------------------------------------
    // Scope
    // -----------------------------------------------------------------------

    #[test]
    fn scope_parse_and_display() {
        let s: Scope = "read write openid".parse().unwrap();
        assert_eq!(s.len(), 3);
        assert!(s.contains("read"));
        assert!(s.contains("write"));
        assert!(s.contains("openid"));
    }

    #[test]
    fn scope_empty() {
        let s: Scope = "".parse().unwrap();
        assert!(s.is_empty());
    }

    #[test]
    fn scope_deduplicates() {
        let s: Scope = "read read write".parse().unwrap();
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn scope_is_subset_of() {
        let full: Scope = "read write admin".parse().unwrap();
        let partial: Scope = "read write".parse().unwrap();
        assert!(partial.is_subset_of(&full));
        assert!(!full.is_subset_of(&partial));
    }

    #[test]
    fn scope_self_is_subset_of_self() {
        let s: Scope = "read write".parse().unwrap();
        assert!(s.is_subset_of(&s));
    }

    #[test]
    fn scope_union() {
        let a: Scope = "read".parse().unwrap();
        let b: Scope = "write".parse().unwrap();
        let c = a.union(&b);
        assert!(c.contains("read"));
        assert!(c.contains("write"));
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn scope_intersection() {
        let a: Scope = "read write".parse().unwrap();
        let b: Scope = "write admin".parse().unwrap();
        let c = a.intersection(&b);
        assert_eq!(c.len(), 1);
        assert!(c.contains("write"));
        assert!(!c.contains("read"));
    }

    #[test]
    fn scope_display_is_sorted() {
        let s: Scope = "z a m".parse().unwrap();
        assert_eq!(s.to_string(), "a m z");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn scope_serde_roundtrip() {
        let s: Scope = "read write".parse().unwrap();
        let json = serde_json::to_string(&s).unwrap();
        let back: Scope = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn bearer_token_serde_roundtrip() {
        let t = BearerToken::new("tok123");
        let json = serde_json::to_string(&t).unwrap();
        let back: BearerToken = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
    }
}
