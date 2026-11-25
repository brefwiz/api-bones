// SPDX-License-Identifier: MIT
//! Who a call is made as, checked before every send and after every answer.

use std::sync::{Arc, OnceLock};

use api_bones_batch::{Credential, CredentialSource, Identity};
use api_bones_rt::{BoxStream, MaybeSend};
use connectrpc::ConnectError;
use futures_util::{StreamExt as _, stream};

use super::{CallEnv, refusal};
use crate::error::Error;

/// The headers that carry the caller's credential.
pub(super) const CREDENTIAL_HEADERS: [&str; 2] = ["authorization", "brefwiz-relayed-credential"];

fn carries(credential: &Credential, name: &str) -> bool {
    credential
        .headers
        .iter()
        .any(|(present, _)| present.eq_ignore_ascii_case(name))
}

/// The identity a call started as, and the credential headers it started
/// with.
///
/// A refreshed token keeps the identity and is accepted; a different subject
/// or organization, or a credential header that went missing, is an identity
/// change.
pub(crate) struct IdentityGuard {
    identity: Identity,
    required: Vec<&'static str>,
    batch: bool,
}

impl IdentityGuard {
    /// Reads the credential the call starts as. A source that cannot answer
    /// fails the call closed.
    pub(crate) fn capture(
        source: &dyn CredentialSource,
        batch: bool,
    ) -> Result<Self, ConnectError> {
        let credential = source.current()?;
        Ok(Self {
            required: CREDENTIAL_HEADERS
                .into_iter()
                .filter(|name| carries(&credential, name))
                .collect(),
            identity: credential.identity,
            batch,
        })
    }

    /// The identity a revision cache is scoped to: the subject, the
    /// organization and the names of the credential headers the call
    /// started with, each length-prefixed so no two identities share an
    /// encoding. Never a header value, so a refreshed token keeps the cache.
    pub(crate) fn cache_identity(&self) -> String {
        let mut names = self.required.clone();
        names.sort_unstable();
        let mut encoded = String::new();
        push_part(&mut encoded, &self.identity.subject);
        push_part(&mut encoded, &self.identity.org);
        encoded.push_str(&names.len().to_string());
        encoded.push(':');
        for name in names {
            push_part(&mut encoded, name);
        }
        encoded
    }

    /// Reads the credential now and answers it when it is still the one the
    /// call started as.
    pub(crate) fn verify(&self, source: &dyn CredentialSource) -> Result<Credential, ConnectError> {
        let credential = source.current()?;
        if credential.identity != self.identity
            || !self.required.iter().all(|name| carries(&credential, name))
        {
            return Err(refusal::identity_changed(self.batch));
        }
        Ok(credential)
    }
}

fn push_part(encoded: &mut String, part: &str) {
    encoded.push_str(&part.len().to_string());
    encoded.push(':');
    encoded.push_str(part);
}

/// The cache identity of a walk that could not read its credential. A real
/// encoding always starts with a digit, so no identity equals it.
pub(super) const UNREADABLE: &str = "";

/// The one identity a whole list walk runs as: captured by the first page
/// that sends, then shared by every later page, so a walk that started as one
/// identity can never continue as another.
#[derive(Default)]
pub(crate) struct WalkGuard(OnceLock<Result<IdentityGuard, ConnectError>>);

impl WalkGuard {
    /// The walk's guard; the first call reads the credential, later calls
    /// answer the guard that call captured.
    pub(crate) fn get(
        &self,
        source: &dyn CredentialSource,
        batch: bool,
    ) -> Result<&IdentityGuard, ConnectError> {
        self.0
            .get_or_init(|| IdentityGuard::capture(source, batch))
            .as_ref()
            .map_err(Clone::clone)
    }

    /// The identity a revision cache is scoped to for this walk, read from
    /// the walk's own guard. A credential that cannot be read answers
    /// [`UNREADABLE`]: that empty identity never has a slot, so clearing it
    /// clears nothing, and the walk's first send fails closed with the
    /// capture error.
    pub(crate) fn cache_identity(&self, source: &dyn CredentialSource) -> String {
        self.get(source, false)
            .map_or_else(|_| UNREADABLE.to_owned(), IdentityGuard::cache_identity)
    }
}

/// `items` with the identity checked against `guard` before each `Ok` item is
/// delivered.
///
/// A change drops the revision cache, delivers that one `IDENTITY_CHANGED`
/// in place of the item, and ends the stream; the item that found it is never
/// delivered. A failure of `items` passes through unchecked, and a refreshed
/// token of the same identity keeps the items flowing.
pub(super) fn checked<T: MaybeSend + 'static>(
    items: BoxStream<'static, Result<T, Error>>,
    env: CallEnv,
    guard: Arc<WalkGuard>,
) -> BoxStream<'static, Result<T, Error>> {
    Box::pin(
        stream::unfold(Some(items), move |state| {
            let env = env.clone();
            let guard = Arc::clone(&guard);
            async move {
                let mut items = state?;
                let item = items.next().await?;
                let source: &dyn CredentialSource = &*env.credentials;
                match item {
                    Ok(item) => match guard.get(source, false).and_then(|own| own.verify(source)) {
                        Ok(_) => Some((Ok(item), Some(items))),
                        Err(refusal) => {
                            env.refused(&refusal, Some(guard.as_ref()));
                            Some((Err(Error::from(refusal)), None))
                        }
                    },
                    Err(error) => Some((Err(error), Some(items))),
                }
            }
        })
        // Polling after the end answers `None` again instead of panicking.
        .fuse(),
    )
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn who(subject: &str, headers: &[(&str, &str)]) -> Credential {
        Credential {
            identity: Identity {
                subject: subject.to_owned(),
                org: "org".to_owned(),
            },
            headers: headers
                .iter()
                .map(|(name, value)| ((*name).to_owned(), value.as_bytes().to_vec()))
                .collect(),
        }
    }

    fn source(credential: Credential) -> impl CredentialSource {
        move || Ok(credential.clone())
    }

    #[test]
    fn a_refreshed_token_of_the_same_identity_is_accepted() {
        let guard =
            IdentityGuard::capture(&source(who("a", &[("authorization", "Bearer 1")])), false)
                .expect("captured");
        let refreshed = guard
            .verify(&source(who("a", &[("authorization", "Bearer 2")])))
            .expect("same identity");
        assert_eq!(refreshed.headers[0].1, b"Bearer 2");
    }

    #[test]
    fn another_subject_or_a_missing_credential_header_is_a_change() {
        let guard =
            IdentityGuard::capture(&source(who("a", &[("authorization", "Bearer 1")])), false)
                .expect("captured");
        for changed in [who("b", &[("authorization", "Bearer 1")]), who("a", &[])] {
            let refusal = guard.verify(&source(changed)).expect_err("changed");
            assert!(refusal::is_identity_changed(&refusal));
        }
    }

    #[test]
    fn a_source_that_cannot_answer_fails_the_call_closed() {
        let broken = || Err::<Credential, _>(ConnectError::unavailable("no credential"));
        assert!(IdentityGuard::capture(&broken, false).is_err());
        let guard = IdentityGuard::capture(&source(who("a", &[])), false).expect("captured");
        assert!(guard.verify(&broken).is_err());
    }

    #[test]
    fn the_cache_identity_names_subject_org_and_credential_headers_never_a_value() {
        let identity = |subject: &str, headers: &[(&str, &str)]| {
            IdentityGuard::capture(&source(who(subject, headers)), false)
                .expect("captured")
                .cache_identity()
        };
        let bearer = identity("a", &[("authorization", "Bearer 1")]);
        assert_eq!(bearer, identity("a", &[("authorization", "Bearer 2")]));
        assert!(!bearer.contains("Bearer"));
        let relayed = identity("a", &[("brefwiz-relayed-credential", "x")]);
        let both = identity(
            "a",
            &[("authorization", "x"), ("brefwiz-relayed-credential", "x")],
        );
        let other = identity("b", &[("authorization", "Bearer 1")]);
        let all = [&bearer, &relayed, &both, &other];
        for (i, left) in all.iter().enumerate() {
            assert!(left.starts_with(|c: char| c.is_ascii_digit()));
            for right in &all[i + 1..] {
                assert_ne!(left, right);
            }
        }
        // The length prefix keeps a subject that ends like an org apart.
        let joined = |subject: &str, org: &str| {
            let credential = Credential {
                identity: Identity {
                    subject: subject.to_owned(),
                    org: org.to_owned(),
                },
                headers: Vec::new(),
            };
            IdentityGuard::capture(&source(credential), false)
                .expect("captured")
                .cache_identity()
        };
        assert_ne!(joined("ab", "c"), joined("a", "bc"));
    }

    #[test]
    fn a_walk_that_cannot_read_its_credential_has_a_cache_identity_no_one_holds() {
        let broken = || Err::<Credential, _>(ConnectError::unavailable("no credential"));
        let walk = WalkGuard::default();
        assert_eq!(walk.cache_identity(&broken), UNREADABLE);
        let real = IdentityGuard::capture(&source(who("a", &[])), false)
            .expect("captured")
            .cache_identity();
        assert_ne!(real, UNREADABLE);
    }

    #[test]
    fn a_walk_keeps_the_identity_its_first_page_captured() {
        let walk = WalkGuard::default();
        let first = walk
            .get(&source(who("a", &[("authorization", "Bearer 1")])), false)
            .expect("captured");
        // A later page reading another identity does not replace the guard.
        let later = walk
            .get(&source(who("b", &[("authorization", "Bearer 1")])), false)
            .expect("shared");
        assert!(std::ptr::eq(first, later));
        let refusal = later
            .verify(&source(who("b", &[("authorization", "Bearer 1")])))
            .expect_err("another identity");
        assert!(refusal::is_identity_changed(&refusal));
    }

    #[test]
    fn a_walk_whose_first_read_failed_stays_failed() {
        let broken = || Err::<Credential, _>(ConnectError::unavailable("no credential"));
        let walk = WalkGuard::default();
        assert!(walk.get(&broken, false).is_err());
        assert!(walk.get(&source(who("a", &[])), false).is_err());
    }

    /// An env whose `n`th credential read answers `read(n)`, and how many reads
    /// it has served.
    fn reading(
        read: impl Fn(usize) -> Result<Credential, ConnectError> + Send + Sync + 'static,
    ) -> (CallEnv, Arc<AtomicUsize>) {
        let reads = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&reads);
        let env = CallEnv::new(move || read(count.fetch_add(1, Ordering::SeqCst)));
        (env, reads)
    }

    fn items(results: Vec<Result<u32, Error>>) -> BoxStream<'static, Result<u32, Error>> {
        Box::pin(stream::iter(results))
    }

    async fn next(
        stream: &mut BoxStream<'static, Result<u32, Error>>,
    ) -> Option<Result<u32, Error>> {
        futures_util::StreamExt::next(stream).await
    }

    #[tokio::test]
    async fn an_item_that_finds_another_identity_is_never_delivered_and_ends_the_stream() {
        let told = Arc::new(AtomicUsize::new(0));
        let heard = Arc::clone(&told);
        // Read 0 captures the identity and read 1 checks the first item; the
        // check of the second item finds another identity.
        let (env, _) = reading(|n| {
            Ok(who(
                if n < 2 { "a" } else { "b" },
                &[("authorization", "x")],
            ))
        });
        let env = env.on_unauthenticated(Arc::new(move || {
            heard.fetch_add(1, Ordering::SeqCst);
        }));
        let mut delivered = checked(
            items(vec![Ok(1), Ok(2), Ok(3)]),
            env,
            Arc::new(WalkGuard::default()),
        );
        assert_eq!(next(&mut delivered).await.expect("an item").expect("ok"), 1);
        let refusal = next(&mut delivered)
            .await
            .expect("the refusal")
            .expect_err("another identity");
        assert_eq!(refusal.code, crate::error::IDENTITY_CHANGED);
        assert_eq!(refusal.platform, "UNAUTHORIZED");
        assert!(next(&mut delivered).await.is_none());
        assert!(next(&mut delivered).await.is_none());
        assert_eq!(
            told.load(Ordering::SeqCst),
            0,
            "no server refused the caller"
        );
    }

    #[tokio::test]
    async fn a_refreshed_token_of_the_same_identity_keeps_the_items_flowing() {
        let (env, reads) = reading(|n| Ok(who("a", &[("authorization", &format!("Bearer {n}"))])));
        let mut delivered = checked(
            items(vec![Ok(1), Ok(2), Ok(3)]),
            env,
            Arc::new(WalkGuard::default()),
        );
        for expected in 1..=3 {
            assert_eq!(
                next(&mut delivered).await.expect("an item").expect("ok"),
                expected
            );
        }
        assert!(next(&mut delivered).await.is_none());
        assert_eq!(
            reads.load(Ordering::SeqCst),
            4,
            "one capture, one check each"
        );
    }

    #[tokio::test]
    async fn a_failure_of_the_stream_passes_through_without_a_check() {
        let (env, reads) = reading(|_| Ok(who("a", &[("authorization", "x")])));
        let mut delivered = checked(
            items(vec![
                Err(Error::from(ConnectError::not_found("gone"))),
                Ok(1),
            ]),
            env,
            Arc::new(WalkGuard::default()),
        );
        let failure = next(&mut delivered)
            .await
            .expect("the failure")
            .expect_err("passed through");
        assert_eq!(failure.platform, "RESOURCE_NOT_FOUND");
        assert_eq!(reads.load(Ordering::SeqCst), 0);
        assert_eq!(next(&mut delivered).await.expect("an item").expect("ok"), 1);
    }

    #[tokio::test]
    async fn a_source_that_cannot_answer_ends_the_stream_closed() {
        let (env, _) = reading(|_| Err(ConnectError::unavailable("no credential")));
        let mut delivered = checked(
            items(vec![Ok(1), Ok(2)]),
            env,
            Arc::new(WalkGuard::default()),
        );
        let failure = next(&mut delivered)
            .await
            .expect("the failure")
            .expect_err("closed");
        assert_eq!(failure.platform, "SERVICE_UNAVAILABLE");
        assert!(next(&mut delivered).await.is_none());
    }
}
