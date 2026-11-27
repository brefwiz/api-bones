// SPDX-License-Identifier: MIT
//! Who a call is made as, and the key two calls must share to merge.
//!
//! Calls under different identities, credentials, headers, deadlines or
//! request fields never share a window. The key is a typed tuple: its parts
//! are compared one by one, never joined into a string.

use std::time::Duration;

use api_bones_page::CacheIdentity;
use api_bones_rt::{BoxFuture, MaybeSend, MaybeSync};
use sha2::{Digest as _, Sha256};

/// Header carrying the per-call deadline, in milliseconds.
pub const TIMEOUT_HEADER: &str = "connect-timeout-ms";

/// Headers that carry the caller's credential.
pub const CREDENTIAL_HEADERS: [&str; 2] = ["authorization", "brefwiz-relayed-credential"];

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

/// The name a credential's cached chains are held under: its subject, its
/// organization and the lower-cased, sorted, de-duplicated names of every
/// header it carries (never a value, so a renewed token keeps its chains),
/// each length-prefixed so no two credentials share an encoding.
pub(crate) fn cache_identity(credential: &Credential) -> Option<CacheIdentity> {
    let mut names: Vec<String> = credential
        .headers
        .iter()
        .map(|(name, _)| name.to_ascii_lowercase())
        .collect();
    names.sort_unstable();
    names.dedup();
    let mut encoded = String::new();
    push_part(&mut encoded, &credential.identity.subject);
    push_part(&mut encoded, &credential.identity.org);
    encoded.push_str(&names.len().to_string());
    encoded.push(':');
    for name in &names {
        push_part(&mut encoded, name);
    }
    CacheIdentity::new(encoded)
}

fn push_part(encoded: &mut String, part: &str) {
    encoded.push_str(&part.len().to_string());
    encoded.push(':');
    encoded.push_str(part);
}

/// The identity a call started as and the credential headers it started
/// with. A renewed token of the same identity is accepted; another subject or
/// organization, or a credential header that went missing, is an identity
/// change.
#[derive(Clone)]
pub struct CredentialGuard {
    identity: Identity,
    required: [bool; 2],
    cache: Option<CacheIdentity>,
}

impl CredentialGuard {
    /// Capture the identity and credential headers of `credential`.
    ///
    /// # Errors
    /// `invalid_argument` when a credential header is duplicated; the batch
    /// identity-changed refusal when a credential header is malformed.
    pub fn capture(credential: &Credential) -> Result<Self, connectrpc::ConnectError> {
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
            cache: cache_identity(credential),
        })
    }

    /// The name the captured credential's cached chains are held under.
    #[must_use]
    pub fn cache_identity(&self) -> Option<CacheIdentity> {
        self.cache.clone()
    }

    /// Whether `credential` is still the one the call started as.
    ///
    /// # Errors
    /// `invalid_argument` when a credential header is duplicated; the batch
    /// identity-changed refusal when the identity differs, a credential header
    /// the call started with is gone, or one is malformed.
    pub fn verify(&self, credential: &Credential) -> Result<(), connectrpc::ConnectError> {
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
#[derive(Clone, Default)]
pub struct Credential {
    /// Who the credential speaks for.
    pub identity: Identity,
    /// The headers that carry it: `authorization`, `brefwiz-relayed-credential`.
    pub headers: Vec<HeaderPair>,
}

impl Credential {
    /// The headers ordered by lower-cased name, then value.
    fn sorted_headers(&self) -> Vec<(String, &[u8])> {
        let mut sorted: Vec<_> = self
            .headers
            .iter()
            .map(|(name, value)| (name.to_ascii_lowercase(), value.as_slice()))
            .collect();
        sorted.sort_unstable();
        sorted
    }
}

/// Equal when subject, organization and every header (by lower-cased name and
/// exact value bytes) are equal, whatever the header order.
impl PartialEq for Credential {
    fn eq(&self, other: &Self) -> bool {
        self.identity == other.identity && self.sorted_headers() == other.sorted_headers()
    }
}

impl Eq for Credential {}

/// Prints the identity and the header names, never a header value: a
/// credential must not reach a log through `{:?}`.
impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credential")
            .field("identity", &self.identity)
            .field(
                "headers",
                &self
                    .headers
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
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

    /// The per-call deadline budget, if one is set.
    pub(crate) fn timeout_budget(&self) -> Option<Duration> {
        self.timeout
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
        for (name, value) in credential.iter().chain(&self.effective()) {
            let name = http::header::HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| connectrpc::ConnectError::invalid_argument("invalid header name"))?;
            let value = http::header::HeaderValue::from_bytes(value)
                .map_err(|_| connectrpc::ConnectError::invalid_argument("invalid header value"))?;
            options = options.with_header(name, value);
        }
        Ok(options)
    }
}

/// What the core asks of a credential source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CredentialAsk {
    /// The server refused the credential last read: the source renews it
    /// before answering, instead of answering what it holds.
    pub refresh: bool,
}

/// What a credential read resolves to: sendable natively, not `Send` on
/// `wasm32`.
pub type CredentialFuture<'a> = BoxFuture<'a, Result<Credential, connectrpc::ConnectError>>;

/// Supplies the caller's credential, re-read before every send.
pub trait CredentialSource: MaybeSend + MaybeSync + 'static {
    /// The credential right now, renewed first when `ask` says the last one
    /// was refused.
    ///
    /// # Errors
    /// When the source cannot answer. The call fails closed: nothing is sent.
    fn current(&self, ask: CredentialAsk) -> CredentialFuture<'_>;
}

impl<F, Fut> CredentialSource for F
where
    F: Fn(CredentialAsk) -> Fut + MaybeSend + MaybeSync + 'static,
    Fut: Future<Output = Result<Credential, connectrpc::ConnectError>> + MaybeSend + 'static,
{
    fn current(&self, ask: CredentialAsk) -> CredentialFuture<'_> {
        Box::pin(self(ask))
    }
}

/// A client that presents no credential: every call is made as the empty identity.
#[derive(Debug, Clone, Copy, Default)]
pub struct Anonymous;

impl CredentialSource for Anonymous {
    fn current(&self, _ask: CredentialAsk) -> CredentialFuture<'_> {
        Box::pin(std::future::ready(Ok(Credential::default())))
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

    fn who(subject: &str) -> Credential {
        Credential {
            identity: Identity {
                subject: subject.to_owned(),
                org: String::new(),
            },
            headers: Vec::new(),
        }
    }

    #[test]
    fn a_header_name_outside_the_credential_headers_splits_the_cache_identity() {
        let with = |extra: &[&str]| {
            let mut credential = who("alice");
            credential.identity.org = "org".to_owned();
            credential.headers = std::iter::once("authorization")
                .chain(extra.iter().copied())
                .map(|name| pair(name, "Bearer 1"))
                .collect();
            credential
        };
        let plain = cache_identity(&with(&[])).expect("a name");
        let extra = cache_identity(&with(&["x-session"])).expect("a name");
        assert_ne!(plain, extra);
        // Case, order and duplicates of a name do not split it.
        assert_eq!(
            extra,
            cache_identity(&with(&["X-Session", "x-session"])).expect("a name")
        );
        let guard = CredentialGuard::capture(&with(&["x-session"])).expect("captured");
        assert_eq!(guard.cache_identity(), Some(extra));
    }

    #[tokio::test]
    async fn a_closure_is_a_credential_source_and_sees_what_it_was_asked() {
        let source = |ask: CredentialAsk| async move {
            Ok(who(if ask.refresh { "renewed" } else { "held" }))
        };
        let held = source.current(CredentialAsk::default()).await.unwrap();
        assert_eq!(held.identity.subject, "held");
        let renewed = source.current(CredentialAsk { refresh: true }).await;
        assert_eq!(renewed.unwrap().identity.subject, "renewed");
    }

    #[tokio::test]
    async fn a_source_that_cannot_answer_says_why() {
        let source = |_: CredentialAsk| async {
            Err::<Credential, _>(connectrpc::ConnectError::unavailable("no token"))
        };
        let error = source.current(CredentialAsk::default()).await.unwrap_err();
        assert_eq!(error.code, connectrpc::ErrorCode::Unavailable);
    }

    #[tokio::test]
    async fn anonymous_answers_the_empty_identity_whether_or_not_asked_to_refresh() {
        for refresh in [false, true] {
            let credential = Anonymous.current(CredentialAsk { refresh }).await.unwrap();
            assert_eq!(credential.identity, Identity::default());
            assert_eq!(credential.headers.len(), 0);
        }
    }

    #[test]
    fn credentials_compare_by_identity_and_headers_in_any_order_and_name_case() {
        let a = Credential {
            identity: who("u").identity,
            headers: vec![pair("authorization", "Bearer 1"), pair("x-b", "2")],
        };
        let reordered = Credential {
            identity: who("u").identity,
            headers: vec![pair("X-B", "2"), pair("Authorization", "Bearer 1")],
        };
        assert_eq!(a, reordered);
        let other_value = Credential {
            identity: who("u").identity,
            headers: vec![pair("authorization", "Bearer 2"), pair("x-b", "2")],
        };
        assert_ne!(a, other_value);
        let other_subject = Credential {
            identity: who("v").identity,
            headers: a.headers.clone(),
        };
        assert_ne!(a, other_subject);
        let fewer = Credential {
            identity: who("u").identity,
            headers: vec![pair("authorization", "Bearer 1")],
        };
        assert_ne!(a, fewer);
    }

    #[test]
    fn a_credential_prints_header_names_and_never_values() {
        let credential = Credential {
            identity: who("u").identity,
            headers: vec![
                pair("authorization", "Bearer secret-token"),
                pair("brefwiz-relayed-credential", "relayed-secret"),
            ],
        };
        let printed = format!("{credential:?}");
        assert!(printed.contains("authorization"));
        assert!(printed.contains("brefwiz-relayed-credential"));
        assert!(!printed.contains("secret-token"));
        assert!(!printed.contains("relayed-secret"));
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
    fn deadline_travels_as_a_header_only() {
        let headers = CallHeaders::new().timeout(Duration::from_millis(1500));
        assert_eq!(headers.timeout_budget(), Some(Duration::from_millis(1500)));
        assert_eq!(CallHeaders::new().timeout_budget(), None);
        let options = headers.call_options(&[]).unwrap();
        assert_eq!(options.timeout(), None);
        assert_eq!(options.headers().get(TIMEOUT_HEADER).unwrap(), "1500");
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
