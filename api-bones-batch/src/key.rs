// SPDX-License-Identifier: MIT
//! Who a call is made as, and the key two calls must share to merge.
//!
//! Calls under different identities, credentials, headers, deadlines or
//! request fields never share a window. The key is a typed tuple: its parts
//! are compared one by one, never joined into a string.

use std::time::Duration;

use sha2::{Digest as _, Sha256};

/// Header carrying the per-call deadline, in milliseconds.
pub const TIMEOUT_HEADER: &str = "connect-timeout-ms";

/// Headers that carry the caller's credential.
const CREDENTIAL_HEADERS: [&str; 2] = ["authorization", "brefwiz-relayed-credential"];

/// Reject ambiguous credentials before canonical hashing or wire dispatch.
pub(crate) fn validate_credentials(pairs: &[HeaderPair]) -> Result<(), connectrpc::ConnectError> {
    for credential in CREDENTIAL_HEADERS {
        if pairs
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case(credential))
            .count()
            > 1
        {
            return Err(connectrpc::ConnectError::invalid_argument(
                "duplicate credential header",
            ));
        }
    }
    Ok(())
}

fn validate_header_pairs(pairs: &[HeaderPair]) -> Result<(), connectrpc::ConnectError> {
    for (name, value) in pairs {
        http::header::HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| connectrpc::ConnectError::invalid_argument("invalid header name"))?;
        http::header::HeaderValue::from_bytes(value)
            .map_err(|_| connectrpc::ConnectError::invalid_argument("invalid header value"))?;
    }
    Ok(())
}

fn validate_source_headers(pairs: &[HeaderPair]) -> Result<(), connectrpc::ConnectError> {
    validate_credentials(pairs)?;
    validate_header_pairs(pairs)?;
    for (name, value) in pairs {
        if CREDENTIAL_HEADERS
            .iter()
            .any(|credential| name.eq_ignore_ascii_case(credential))
        {
            let value =
                std::str::from_utf8(value).map_err(|_| crate::item_error::identity_changed())?;
            let token = if name.eq_ignore_ascii_case("authorization") {
                value
                    .strip_prefix("Bearer ")
                    .ok_or_else(crate::item_error::identity_changed)?
            } else {
                value
            };
            if token.is_empty() || token.chars().any(char::is_whitespace) {
                return Err(crate::item_error::identity_changed());
            }
        }
    }
    Ok(())
}

#[derive(Clone)]
pub(crate) struct CredentialGuard {
    identity: Identity,
    required: [bool; 2],
}

impl CredentialGuard {
    pub(crate) fn capture(credential: &Credential) -> Result<Self, connectrpc::ConnectError> {
        validate_credentials(&credential.headers)?;
        validate_source_headers(&credential.headers)
            .map_err(|_| crate::item_error::identity_changed())?;
        Ok(Self {
            identity: credential.identity.clone(),
            required: CREDENTIAL_HEADERS.map(|name| {
                credential
                    .headers
                    .iter()
                    .any(|(present, _)| present.eq_ignore_ascii_case(name))
            }),
        })
    }

    pub(crate) fn verify(&self, credential: &Credential) -> Result<(), connectrpc::ConnectError> {
        validate_credentials(&credential.headers)?;
        if credential.identity != self.identity {
            return Err(crate::item_error::identity_changed());
        }
        validate_source_headers(&credential.headers)
            .map_err(|_| crate::item_error::identity_changed())?;
        for (name, required) in CREDENTIAL_HEADERS.into_iter().zip(self.required) {
            if required
                && !credential
                    .headers
                    .iter()
                    .any(|(present, _)| present.eq_ignore_ascii_case(name))
            {
                return Err(crate::item_error::identity_changed());
            }
        }
        Ok(())
    }
}

/// The subject and organization a credential speaks for.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Identity {
    /// The credential's subject.
    pub subject: String,
    /// The credential's organization.
    pub org: String,
}

/// A header as sent on the wire: a lower-case name and the value's bytes.
pub type HeaderPair = (String, Vec<u8>);

/// The credential a token source holds right now.
#[derive(Debug, Clone, Default)]
pub struct Credential {
    /// Who the credential speaks for.
    pub identity: Identity,
    /// The headers that carry it: `authorization`, `brefwiz-relayed-credential`.
    pub headers: Vec<HeaderPair>,
}

/// Per-call headers and deadline, as the caller gave them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CallHeaders {
    pairs: Vec<HeaderPair>,
    timeout: Option<Duration>,
}

impl CallHeaders {
    pub(crate) fn validate(
        &self,
        credential: &[HeaderPair],
    ) -> Result<(), connectrpc::ConnectError> {
        if !credential_only(&self.pairs).is_empty() {
            return Err(connectrpc::ConnectError::invalid_argument(
                "per-call credentials must come from the batch credential source",
            ));
        }
        let mut effective = credential.to_vec();
        effective.extend(self.effective());
        validate_credentials(&effective)?;
        validate_header_pairs(&effective)
    }

    /// No extra headers and no deadline.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a header. The name is lower-cased.
    #[must_use]
    pub fn header(mut self, name: &str, value: impl Into<Vec<u8>>) -> Self {
        self.pairs.push((name.to_ascii_lowercase(), value.into()));
        self
    }

    /// Set the per-call deadline.
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Every header the call sends besides its credential, the deadline
    /// included as `connect-timeout-ms`.
    pub(crate) fn effective(&self) -> Vec<HeaderPair> {
        let mut pairs = self.pairs.clone();
        if let Some(timeout) = self.timeout {
            pairs.push((
                TIMEOUT_HEADER.to_owned(),
                timeout.as_millis().to_string().into_bytes(),
            ));
        }
        pairs
    }

    pub(crate) fn continuation(&self) -> Self {
        Self {
            pairs: self
                .pairs
                .iter()
                .filter(|(name, _)| {
                    !name.eq_ignore_ascii_case("if-none-match")
                        && !name.eq_ignore_ascii_case("bones-list-cache")
                })
                .cloned()
                .collect(),
            timeout: self.timeout,
        }
    }

    /// The call options a generated client takes for these headers plus the
    /// credential headers.
    pub(crate) fn call_options(
        &self,
        credential: &[HeaderPair],
    ) -> Result<connectrpc::client::CallOptions, connectrpc::ConnectError> {
        self.validate(credential)?;
        let mut options = connectrpc::client::CallOptions::default();
        if let Some(timeout) = self.timeout {
            options = options.with_timeout(timeout);
        }
        for (name, value) in credential.iter().chain(&self.pairs) {
            let name = http::header::HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| connectrpc::ConnectError::invalid_argument("invalid header name"))?;
            let value = http::header::HeaderValue::from_bytes(value)
                .map_err(|_| connectrpc::ConnectError::invalid_argument("invalid header value"))?;
            options = options.with_header(name, value);
        }
        Ok(options)
    }
}

/// Supplies the caller's credential, re-read before every send.
pub trait CredentialSource: Send + Sync + 'static {
    /// The credential right now.
    ///
    /// # Errors
    /// When the source cannot answer. The batch runtime fails closed: nothing
    /// is sent.
    fn current(&self) -> Result<Credential, connectrpc::ConnectError>;
}

impl<F> CredentialSource for F
where
    F: Fn() -> Result<Credential, connectrpc::ConnectError> + Send + Sync + 'static,
{
    fn current(&self) -> Result<Credential, connectrpc::ConnectError> {
        self()
    }
}

/// A client that presents no credential: every call is made as the empty identity.
#[derive(Debug, Clone, Copy, Default)]
pub struct Anonymous;

impl CredentialSource for Anonymous {
    fn current(&self) -> Result<Credential, connectrpc::ConnectError> {
        Ok(Credential::default())
    }
}

/// What two calls must share to merge into one window.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CoalesceKey {
    /// The single method's full name.
    pub method: &'static str,
    /// SHA-256 of the request's binary encoding with its id cleared.
    pub request: [u8; 32],
    /// SHA-256 of the length-prefixed header pairs.
    pub headers: [u8; 32],
    /// Who the call is made as.
    pub identity: Identity,
}

/// What bounds the queue of unsent chunks: the identity and the hash of its
/// credential headers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QueueKey {
    /// Who the chunks are sent as.
    pub identity: Identity,
    /// SHA-256 of the length-prefixed credential headers.
    pub credentials: [u8; 32],
}

/// SHA-256 of bytes.
#[must_use]
pub fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// SHA-256 of the length-prefixed `(name, value)` pairs, names lower-cased,
/// sorted by name then value.
#[must_use]
pub fn header_hash(pairs: &[HeaderPair]) -> [u8; 32] {
    let mut sorted: Vec<(String, &[u8])> = pairs
        .iter()
        .map(|(name, value)| (name.to_ascii_lowercase(), value.as_slice()))
        .collect();
    sorted.sort();
    let mut hasher = Sha256::new();
    for (name, value) in sorted {
        hasher.update((name.len() as u64).to_be_bytes());
        hasher.update(name.as_bytes());
        hasher.update((value.len() as u64).to_be_bytes());
        hasher.update(value);
    }
    hasher.finalize().into()
}

/// The part of `pairs` that carries a credential.
pub(crate) fn credential_only(pairs: &[HeaderPair]) -> Vec<HeaderPair> {
    pairs
        .iter()
        .filter(|(name, _)| {
            CREDENTIAL_HEADERS
                .iter()
                .any(|credential| name.eq_ignore_ascii_case(credential))
        })
        .cloned()
        .collect()
}

impl QueueKey {
    pub(crate) fn of(credential: &Credential) -> Self {
        Self {
            identity: credential.identity.clone(),
            credentials: header_hash(&credential_only(&credential.headers)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(name: &str, value: &str) -> HeaderPair {
        (name.to_owned(), value.as_bytes().to_vec())
    }

    #[test]
    fn header_hash_ignores_order_and_name_case() {
        let a = header_hash(&[pair("A", "1"), pair("b", "2")]);
        let b = header_hash(&[pair("b", "2"), pair("a", "1")]);
        assert_eq!(a, b);
    }

    #[test]
    fn header_hash_is_length_prefixed() {
        let joined = header_hash(&[pair("ab", "c")]);
        let split = header_hash(&[pair("a", "bc")]);
        assert_ne!(joined, split);
        let two = header_hash(&[pair("a", "b"), pair("c", "d")]);
        let one = header_hash(&[pair("a", "bc"), pair("", "d")]);
        assert_ne!(two, one);
    }

    #[test]
    fn deadline_joins_the_effective_headers() {
        let headers = CallHeaders::new().timeout(Duration::from_millis(1500));
        assert_eq!(headers.effective(), vec![pair(TIMEOUT_HEADER, "1500")]);
    }

    #[test]
    fn credential_headers_are_picked_out() {
        let all = [pair("authorization", "x"), pair("x-other", "y")];
        assert_eq!(credential_only(&all), vec![pair("authorization", "x")]);
    }

    #[test]
    fn duplicate_credentials_are_rejected_regardless_of_case_order_or_source() {
        for name in CREDENTIAL_HEADERS {
            let pairs = [
                pair(name, "first"),
                pair(&name.to_ascii_uppercase(), "second"),
            ];
            assert!(validate_credentials(&pairs).is_err());
            assert!(validate_credentials(&[pairs[1].clone(), pairs[0].clone()]).is_err());
            let credential = Credential {
                headers: pairs.to_vec(),
                ..Credential::default()
            };
            assert_eq!(
                CredentialGuard::capture(&credential)
                    .err()
                    .expect("duplicate credential rejected during capture")
                    .code,
                connectrpc::ErrorCode::InvalidArgument
            );
            let guard = CredentialGuard::capture(&Credential::default()).unwrap();
            assert_eq!(
                guard.verify(&credential).unwrap_err().code,
                connectrpc::ErrorCode::InvalidArgument
            );
            assert!(
                CallHeaders::new()
                    .header(name, b"per-call".to_vec())
                    .validate(&pairs[..1])
                    .is_err()
            );
        }
        assert!(validate_credentials(&[pair("x-metadata", "a"), pair("x-metadata", "b")]).is_ok());
        assert!(
            CallHeaders::new()
                .header("authorization", Vec::new())
                .validate(&[])
                .is_err()
        );
    }

    #[test]
    fn continuation_discards_conditional_cache_headers_and_preserves_metadata() {
        let headers = CallHeaders::new()
            .header("if-none-match", b"revision".to_vec())
            .header("bones-list-cache", b"1".to_vec())
            .header("x-trace", b"kept".to_vec())
            .timeout(Duration::from_millis(500));
        assert_eq!(
            headers.continuation().effective(),
            vec![pair("x-trace", "kept"), pair(TIMEOUT_HEADER, "500")]
        );
    }

    #[test]
    fn malformed_metadata_cannot_be_silently_omitted_from_call_options() {
        assert!(
            CallHeaders::new()
                .header("x-metadata", vec![b'\n'])
                .call_options(&[])
                .is_err()
        );
        assert!(
            CallHeaders::new()
                .header("invalid name", Vec::new())
                .call_options(&[])
                .is_err()
        );
    }
}
