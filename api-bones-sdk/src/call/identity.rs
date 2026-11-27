// SPDX-License-Identifier: MIT
//! Who a call is made as, checked before every send and after every answer.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};

use api_bones::connect::error_info;
use api_bones_batch::{
    BATCH_IDENTITY_CHANGED, Credential, CredentialAsk, CredentialFuture, CredentialGuard,
    CredentialSource,
};
use api_bones_page::{CacheIdentity, IdentityFn, RevisionCache};
use api_bones_rt::{BoxFuture, BoxStream, MaybeSend};
use connectrpc::ConnectError;
use futures_util::future::{FutureExt as _, Shared};
use futures_util::lock::Mutex as Gate;
use futures_util::{StreamExt as _, stream};

use super::{CallEnv, refusal};
use crate::error::Error;

/// What a refresh of the credential a server refused came to.
pub(crate) enum Refreshed {
    /// The source renewed the credential, still for the same caller: send
    /// this one.
    Replay(Credential),
    /// The refresh ended without a credential to send: the source has nothing
    /// newer than the credential that was refused (`error` is `None`, and the
    /// call ends with the server's own refusal), or the refresh failed
    /// (`error`). `first` is whether this call is the first of the refresh's
    /// waiters to hear it: the one that tells the host and drops the revision
    /// cache slot, once for the whole refresh.
    Ended {
        error: Option<ConnectError>,
        first: bool,
    },
}

/// A refresh the waiters of one refused credential share. It lives while a
/// waiter holds it: when the last waiter is gone it is dropped and the read
/// it made is aborted.
struct Running {
    renewal: Shared<BoxFuture<'static, Result<Credential, ConnectError>>>,
    /// Set by the first waiter to hear that the refresh ended in failure.
    told: AtomicBool,
}

struct Flight {
    refused: Credential,
    running: Weak<Running>,
}

/// The refreshes in flight of one env, one per credential a server refused.
///
/// Calls refused on the same credential share one refresh instead of each
/// asking the source for its own, and a refresh that came back with the
/// refused credential is the host's one refusal, not one per call. A flight is
/// never reused once it has finished; the list holds no credential beyond the
/// flights still running.
#[derive(Clone, Default)]
pub(crate) struct RefreshFlights(Arc<Mutex<Vec<Flight>>>);

impl RefreshFlights {
    /// The running flight for `refused`. Finished and abandoned flights are
    /// dropped from the list first.
    fn running(flights: &mut Vec<Flight>, refused: &Credential) -> Option<Arc<Running>> {
        flights.retain(|flight| {
            flight
                .running
                .upgrade()
                .is_some_and(|running| running.renewal.peek().is_none())
        });
        flights
            .iter()
            .find(|flight| flight.refused == *refused)
            .and_then(|flight| flight.running.upgrade())
    }

    fn join(&self, refused: &Credential) -> Option<Arc<Running>> {
        Self::running(
            &mut self.0.lock().unwrap_or_else(PoisonError::into_inner),
            refused,
        )
    }

    /// The running flight for `refused`, or a new one started now: one
    /// `refresh: true` read of `source`.
    fn join_or_start(
        &self,
        source: &Arc<dyn CredentialSource>,
        refused: &Credential,
    ) -> Arc<Running> {
        let mut flights = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(running) = Self::running(&mut flights, refused) {
            return running;
        }
        let source = Arc::clone(source);
        let read: BoxFuture<'static, _> =
            Box::pin(async move { source.current(CredentialAsk { refresh: true }).await });
        let running = Arc::new(Running {
            renewal: read.shared(),
            told: AtomicBool::new(false),
        });
        flights.push(Flight {
            refused: refused.clone(),
            running: Arc::downgrade(&running),
        });
        running
    }

    /// Renews the credential the server refused: joins the refresh already
    /// running for it, or starts one.
    ///
    /// A call that finds no refresh running first reads the credential: when
    /// it is no longer the refused one, someone already renewed it and that
    /// is replayed without a refresh. A refresh that comes back with the
    /// refused credential reads once more before it gives up, for another
    /// refresh may have renewed it meanwhile. Each waiter checks the renewal
    /// against its own `guard`.
    ///
    /// # Errors
    /// The source's failure to read the credential, or `IDENTITY_CHANGED` when
    /// the renewed credential speaks for another caller than `guard`'s.
    pub(crate) async fn refresh(
        &self,
        guard: &IdentityGuard,
        source: &Arc<dyn CredentialSource>,
        refused: &Credential,
    ) -> Result<Refreshed, ConnectError> {
        let joined = if let Some(joined) = self.join(refused) {
            joined
        } else {
            let current = source.current(CredentialAsk::default()).await?;
            if current != *refused {
                guard.check(&current)?;
                return Ok(Refreshed::Replay(current));
            }
            self.join_or_start(source, refused)
        };
        let renewed = joined.renewal.clone().await;
        let first = || !joined.told.swap(true, Ordering::SeqCst);
        let renewed = match renewed {
            Ok(renewed) => renewed,
            Err(error) => {
                return Ok(Refreshed::Ended {
                    error: Some(error),
                    first: first(),
                });
            }
        };
        if renewed != *refused {
            guard.check(&renewed)?;
            return Ok(Refreshed::Replay(renewed));
        }
        let current = source.current(CredentialAsk::default()).await?;
        if current != *refused {
            guard.check(&current)?;
            return Ok(Refreshed::Replay(current));
        }
        Ok(Refreshed::Ended {
            error: None,
            first: first(),
        })
    }
}

/// The credential source of an env that holds a revision cache, and what the
/// env filled of it.
///
/// Every read of the credential goes through it. It records the cache
/// identities the env's walks filled, shared by every clone of the env, and
/// drops them eagerly when the session behind the credential ends: a read of
/// another caller drops the slots of the callers before it, and a read that
/// fails for any reason but a changed identity (a sign-out rejects or answers
/// no credential) drops every slot the session filled. Slots another env of
/// the shared cache filled are never touched.
#[derive(Clone)]
pub(crate) struct Session {
    inner: Arc<dyn CredentialSource>,
    cache: RevisionCache,
    filled: Arc<Mutex<Vec<String>>>,
}

impl Session {
    pub(crate) fn over(inner: Arc<dyn CredentialSource>, cache: RevisionCache) -> Self {
        Self {
            inner,
            cache,
            filled: Arc::default(),
        }
    }

    /// The source the session reads.
    pub(crate) fn into_inner(self) -> Arc<dyn CredentialSource> {
        self.inner
    }

    /// Notes that the env filled the slot of `identity`.
    pub(crate) fn record(&self, identity: &CacheIdentity) {
        let mut filled = self.filled.lock().unwrap_or_else(PoisonError::into_inner);
        if !filled.iter().any(|name| name == identity.as_str()) {
            filled.push(identity.as_str().to_owned());
        }
    }

    /// Clears every recorded slot but `keep`. The names stay recorded: a walk
    /// records its identity before its first send and creates the slot only
    /// when the first answer arrives, so a slot cleared here can be filled
    /// again by a walk that was already in flight, and the next end must clear
    /// it too. Clearing a name twice is harmless. The set holds the distinct
    /// identities this session's walks started as, one short string each,
    /// which is as many as the callers that signed in on this env.
    fn drop_filled(&self, keep: Option<&str>) {
        for name in self
            .filled
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|name| keep != Some(name.as_str()))
        {
            self.cache.clear_identity(name);
        }
    }

    fn recorded(&self) -> bool {
        !self
            .filled
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
    }
}

impl CredentialSource for Session {
    fn current(&self, ask: CredentialAsk) -> CredentialFuture<'_> {
        Box::pin(async move {
            let read = self.inner.current(ask).await;
            if self.recorded() {
                match &read {
                    Ok(credential) => {
                        let identity = CredentialGuard::capture(credential)
                            .ok()
                            .and_then(|guard| guard.cache_identity());
                        self.drop_filled(identity.as_ref().map(CacheIdentity::as_str));
                    }
                    Err(error) if !refusal::is_identity_changed(error) => self.drop_filled(None),
                    Err(_) => {}
                }
            }
            read
        })
    }
}

/// The identity a call started as, and the credential headers it started
/// with.
///
/// A refreshed token keeps the identity and is accepted; a different subject
/// or organization, or a credential header that went missing, is an identity
/// change.
pub(crate) struct IdentityGuard {
    guard: CredentialGuard,
    batch: bool,
}

/// `error` with the batch identity-changed refusal in the call's own
/// vocabulary; any other refusal passes through.
fn own_refusal(error: ConnectError, batch: bool) -> ConnectError {
    if error_info(&error).is_some_and(|info| info.code == BATCH_IDENTITY_CHANGED) {
        refusal::identity_changed(batch)
    } else {
        error
    }
}

impl IdentityGuard {
    /// Reads the credential the call starts as. A source that cannot answer
    /// fails the call closed.
    pub(crate) async fn capture(
        source: &dyn CredentialSource,
        batch: bool,
    ) -> Result<Self, ConnectError> {
        let credential = source.current(CredentialAsk::default()).await?;
        let guard = CredentialGuard::capture(&credential).map_err(|e| own_refusal(e, batch))?;
        Ok(Self { guard, batch })
    }

    /// Whether `credential` is still the one the call started as.
    ///
    /// # Errors
    /// `IDENTITY_CHANGED` when the subject or organization differ or a
    /// credential header the call started with is gone.
    pub(crate) fn check(&self, credential: &Credential) -> Result<(), ConnectError> {
        self.guard
            .verify(credential)
            .map_err(|error| own_refusal(error, self.batch))
    }

    /// Reads the credential now and answers it when it is still the one the
    /// call started as.
    pub(crate) async fn verify(
        &self,
        source: &dyn CredentialSource,
    ) -> Result<Credential, ConnectError> {
        let credential = source.current(CredentialAsk::default()).await?;
        self.check(&credential)?;
        Ok(credential)
    }
}

/// The one identity a whole list walk runs as: captured by the first read,
/// then shared by every later page, so a walk that started as one identity can
/// never continue as another.
#[derive(Default)]
pub(crate) struct WalkGuard {
    /// Lets one read capture while the others wait for its answer; a read
    /// that is dropped first leaves the capture to the next.
    gate: Gate<()>,
    captured: Mutex<Option<Result<Arc<IdentityGuard>, ConnectError>>>,
}

impl WalkGuard {
    /// The walk's guard; the first call reads the credential, later calls
    /// answer the guard that call captured.
    pub(crate) async fn get(
        &self,
        source: &dyn CredentialSource,
        batch: bool,
    ) -> Result<Arc<IdentityGuard>, ConnectError> {
        let _capturing = self.gate.lock().await;
        if let Some(done) = self.held() {
            return done;
        }
        let captured = IdentityGuard::capture(source, batch).await.map(Arc::new);
        *self.captured.lock().unwrap_or_else(PoisonError::into_inner) = Some(captured.clone());
        captured
    }

    fn held(&self) -> Option<Result<Arc<IdentityGuard>, ConnectError>> {
        self.captured
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The identity a revision cache is scoped to for this walk, read from
    /// the walk's own capture. `None` until a credential was captured and
    /// when the capture failed: nothing is scoped to an unnamed caller.
    pub(crate) fn cache_identity(&self) -> Option<CacheIdentity> {
        self.held()?.ok()?.guard.cache_identity()
    }
}

/// The hook a list walk resolves its cache identity with: the identity the
/// walk captured, never a second read of the credential.
pub(super) fn cache_identity_of(walk: Arc<WalkGuard>) -> IdentityFn {
    Arc::new(move |()| {
        let identity = walk.cache_identity().ok_or_else(|| {
            ConnectError::failed_precondition("the walk has not captured its identity")
        });
        Box::pin(std::future::ready(identity))
    })
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
                    Ok(item) => {
                        let verified = match guard.get(source, false).await {
                            Ok(own) => own.verify(source).await,
                            Err(error) => Err(error),
                        };
                        match verified {
                            Ok(_) => Some((Ok(item), Some(items))),
                            Err(refusal) => {
                                env.refused(&refusal, Some(guard.as_ref()), true);
                                Some((Err(Error::from(refusal)), None))
                            }
                        }
                    }
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
    use api_bones_batch::Identity;
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
        move |_: CredentialAsk| {
            let credential = credential.clone();
            async move { Ok(credential) }
        }
    }

    fn broken() -> impl CredentialSource {
        |_: CredentialAsk| async { Err::<Credential, _>(ConnectError::unavailable("no")) }
    }

    async fn captured(credential: Credential) -> IdentityGuard {
        IdentityGuard::capture(&source(credential), false)
            .await
            .expect("captured")
    }

    #[tokio::test]
    async fn a_refreshed_token_of_the_same_identity_is_accepted() {
        let guard = captured(who("a", &[("authorization", "Bearer 1")])).await;
        let refreshed = guard
            .verify(&source(who("a", &[("authorization", "Bearer 2")])))
            .await
            .expect("same identity");
        assert_eq!(refreshed.headers[0].1, b"Bearer 2");
    }

    #[tokio::test]
    async fn another_subject_or_a_missing_credential_header_is_a_change() {
        let guard = captured(who("a", &[("authorization", "Bearer 1")])).await;
        for changed in [who("b", &[("authorization", "Bearer 1")]), who("a", &[])] {
            let refusal = guard
                .verify(&source(changed.clone()))
                .await
                .expect_err("changed");
            assert!(refusal::is_identity_changed(&refusal));
            assert!(refusal::is_identity_changed(
                &guard.check(&changed).expect_err("changed")
            ));
        }
        assert!(
            guard
                .check(&who("a", &[("authorization", "Bearer 9")]))
                .is_ok()
        );
    }

    #[tokio::test]
    async fn a_source_that_cannot_answer_fails_the_call_closed() {
        assert!(IdentityGuard::capture(&broken(), false).await.is_err());
        let guard = captured(who("a", &[])).await;
        assert!(guard.verify(&broken()).await.is_err());
    }

    #[tokio::test]
    async fn the_cache_identity_names_subject_org_and_credential_headers_never_a_value() {
        let identity = |subject: &str, headers: &[(&str, &str)]| {
            let credential = who(subject, headers);
            async move {
                captured(credential)
                    .await
                    .guard
                    .cache_identity()
                    .expect("a name")
            }
        };
        let bearer = identity("a", &[("authorization", "Bearer 1")]).await;
        assert_eq!(
            bearer,
            identity("a", &[("authorization", "Bearer 2")]).await
        );
        assert!(!bearer.as_str().contains("Bearer"));
        let relayed = identity("a", &[("brefwiz-relayed-credential", "x")]).await;
        let both = identity(
            "a",
            &[
                ("authorization", "Bearer x"),
                ("brefwiz-relayed-credential", "x"),
            ],
        )
        .await;
        let other = identity("b", &[("authorization", "Bearer 1")]).await;
        let cookie_user = identity("a", &[("cookie", "s1")]).await;
        let other_header_user = identity("b", &[("x-session", "s1")]).await;
        let extra_header =
            identity("a", &[("authorization", "Bearer 1"), ("x-session", "s1")]).await;
        let all = [
            &bearer,
            &relayed,
            &both,
            &other,
            &cookie_user,
            &other_header_user,
            &extra_header,
        ];
        for (i, left) in all.iter().enumerate() {
            assert!(left.as_str().starts_with(|c: char| c.is_ascii_digit()));
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
            async move {
                captured(credential)
                    .await
                    .guard
                    .cache_identity()
                    .expect("a name")
            }
        };
        assert_ne!(joined("ab", "c").await, joined("a", "bc").await);
    }

    /// A source whose plain reads answer `plain` and whose refreshes wait for
    /// `gate`, then answer `renewed` and leave `adopted` (when there is one) as
    /// what a plain read answers from then on.
    struct Lab {
        plain: Mutex<Credential>,
        renewed: Mutex<Result<Credential, ConnectError>>,
        adopted: Mutex<Option<Credential>>,
        gate: AtomicBool,
        refreshes: AtomicUsize,
        /// Refresh reads that were dropped before they answered.
        aborted: AtomicUsize,
    }

    /// Counts a refresh read that is dropped before it answers.
    struct Unanswered<'a>(&'a Lab, bool);

    impl Drop for Unanswered<'_> {
        fn drop(&mut self) {
            if self.1 {
                self.0.aborted.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    impl Lab {
        fn new(plain: Credential, renewed: Result<Credential, ConnectError>) -> Arc<Self> {
            Arc::new(Self {
                plain: Mutex::new(plain),
                renewed: Mutex::new(renewed),
                adopted: Mutex::new(None),
                gate: AtomicBool::new(true),
                refreshes: AtomicUsize::new(0),
                aborted: AtomicUsize::new(0),
            })
        }

        fn source(self: &Arc<Self>) -> Arc<dyn CredentialSource> {
            let lab = Arc::clone(self);
            Arc::new(move |ask: CredentialAsk| {
                let lab = Arc::clone(&lab);
                async move {
                    if !ask.refresh {
                        return Ok(lab.plain.lock().expect("plain").clone());
                    }
                    lab.refreshes.fetch_add(1, Ordering::SeqCst);
                    let mut unanswered = Unanswered(&lab, true);
                    // Every waiter gets to join before the answer arrives.
                    for _ in 0..3 {
                        tokio::task::yield_now().await;
                    }
                    while !lab.gate.load(Ordering::SeqCst) {
                        tokio::task::yield_now().await;
                    }
                    unanswered.1 = false;
                    if let Some(adopted) = lab.adopted.lock().expect("adopted").clone() {
                        *lab.plain.lock().expect("plain") = adopted;
                    }
                    lab.renewed.lock().expect("renewed").clone()
                }
            })
        }

        fn adopt(&self, credential: Credential) {
            *self.adopted.lock().expect("adopted") = Some(credential);
        }

        fn refreshed(&self) -> usize {
            self.refreshes.load(Ordering::SeqCst)
        }
    }

    fn token(n: usize) -> Credential {
        who("a", &[("authorization", &format!("Bearer {n}"))])
    }

    /// `waiters` refreshes of the credential `refused`, all running together.
    async fn together(
        flights: &RefreshFlights,
        source: &Arc<dyn CredentialSource>,
        refused: &Credential,
        waiters: usize,
    ) -> Vec<Result<Refreshed, ConnectError>> {
        let guard = captured(refused.clone()).await;
        futures_util::future::join_all(
            (0..waiters).map(|_| flights.refresh(&guard, source, refused)),
        )
        .await
    }

    fn replayed(outcome: Result<Refreshed, ConnectError>) -> Credential {
        match outcome.expect("refreshed") {
            Refreshed::Replay(credential) => credential,
            Refreshed::Ended { .. } => panic!("a credential was to be replayed"),
        }
    }

    /// Polls `future` once without waking it.
    fn poll_once<F: Future>(future: &mut std::pin::Pin<Box<F>>) -> std::task::Poll<F::Output> {
        let waker = futures_util::task::noop_waker();
        future
            .as_mut()
            .poll(&mut std::task::Context::from_waker(&waker))
    }

    type Outcome = Result<Refreshed, ConnectError>;
    type Parked<F> = std::pin::Pin<Box<F>>;

    /// A flight renewing `token(1)` into `token(2)`, and what its calls share.
    struct Parking {
        lab: Arc<Lab>,
        flights: RefreshFlights,
        source: Arc<dyn CredentialSource>,
        refused: Credential,
        guard: IdentityGuard,
    }

    impl Parking {
        async fn new() -> Self {
            let lab = Lab::new(token(1), Ok(token(2)));
            let source = lab.source();
            Self {
                lab,
                flights: RefreshFlights::default(),
                source,
                refused: token(1),
                guard: captured(token(1)).await,
            }
        }

        /// The waiter left of two calls parked behind the closed gate of one
        /// flight once the call that started it is dropped.
        fn waiter_left(&self) -> Parked<impl Future<Output = Outcome> + '_> {
            self.lab.gate.store(false, Ordering::SeqCst);
            let refresh = || {
                Box::pin(
                    self.flights
                        .refresh(&self.guard, &self.source, &self.refused),
                )
            };
            let (mut starter, mut waiter) = (refresh(), refresh());
            assert!(poll_once(&mut starter).is_pending());
            assert!(poll_once(&mut waiter).is_pending());
            drop(starter);
            waiter
        }
    }

    #[tokio::test]
    async fn concurrent_calls_refused_on_one_credential_renew_it_once() {
        let lab = Lab::new(token(1), Ok(token(2)));
        lab.adopt(token(2));
        let flights = RefreshFlights::default();
        let outcomes = together(&flights, &lab.source(), &token(1), 4).await;
        assert_eq!(outcomes.len(), 4);
        for outcome in outcomes {
            assert_eq!(replayed(outcome), token(2));
        }
        assert_eq!(lab.refreshed(), 1);
    }

    #[tokio::test]
    async fn a_refresh_with_nothing_newer_ends_every_waiter_and_is_heard_once() {
        let lab = Lab::new(token(1), Ok(token(1)));
        let flights = RefreshFlights::default();
        let outcomes = together(&flights, &lab.source(), &token(1), 4).await;
        let firsts = outcomes
            .into_iter()
            .map(|outcome| match outcome.expect("answered") {
                Refreshed::Ended { error, first } => {
                    assert!(error.is_none(), "the server's own refusal stands");
                    first
                }
                Refreshed::Replay(_) => panic!("nothing newer to replay"),
            })
            .filter(|first| *first)
            .count();
        assert_eq!(firsts, 1, "one waiter tells the host");
        assert_eq!(lab.refreshed(), 1);
    }

    #[derive(Clone)]
    struct Req {
        cursor: String,
    }

    impl api_bones_page::PagedRequest for Req {
        fn set_page(&mut self, cursor: &str, _page_size: Option<u32>) {
            cursor.clone_into(&mut self.cursor);
        }
    }

    impl api_bones_page::CanonicalPagedRequest for Req {
        fn encode(&self) -> Vec<u8> {
            b"widgets".to_vec()
        }
    }

    struct Resp(u32, &'static str);

    impl api_bones_page::PagedResponse for Resp {
        type Item = u32;

        fn encoded_len(_: &u32) -> usize {
            1
        }

        fn split(self) -> (Vec<u32>, api_bones_page::PageInfo) {
            (
                vec![self.0],
                api_bones_page::PageInfo {
                    next_cursor: self.1.to_owned(),
                    revision: "r1".to_owned(),
                    total: None,
                },
            )
        }
    }

    /// One walk of a two-page list through `env`; the server confirms the
    /// revision of any page it is asked to revalidate.
    async fn walked(env: &CallEnv) -> Vec<api_bones_page::Page<u32>> {
        let send: super::super::PageSend<Req, Resp> = Arc::new(
            |(request, attempt): (Req, super::super::Attempt)| -> BoxFuture<'static, _> {
                let confirms = attempt
                    .headers()
                    .iter()
                    .any(|(name, _)| name == "if-none-match");
                Box::pin(async move {
                    let body = match (confirms, request.cursor.is_empty()) {
                        (true, _) => Resp(0, ""),
                        (false, true) => Resp(1, "p2"),
                        (false, false) => Resp(2, ""),
                    };
                    Ok(api_bones_page::ListResponse {
                        body,
                        not_modified: confirms,
                    })
                })
            },
        );
        let list = env.call("acme.v1.Widgets", "ListWidgets").list(
            api_bones::connect::Idempotency::NoSideEffects,
            Req {
                cursor: String::new(),
            },
            send,
            api_bones_page::PageOptions {
                prefetch: 0,
                ..api_bones_page::PageOptions::default()
            },
        );
        let mut pages = Vec::new();
        while let Some(page) = list.next().await.expect("a page") {
            pages.push(page);
        }
        pages
    }

    #[tokio::test]
    async fn waiters_of_different_walks_on_one_ended_flight_clear_their_own_slots_and_the_host_hears_once()
     {
        let lab = Lab::new(token(1), Ok(token(1)));
        let flights = RefreshFlights::default();
        let source = lab.source();
        let heard = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&heard);
        let cache = api_bones_page::RevisionCache::default();
        let refused = token(1);
        let carried = |header: &str| {
            let value = if header.eq_ignore_ascii_case("authorization") {
                "Bearer x"
            } else {
                "x"
            };
            who("a", &[(header, value)])
        };
        let envs = ["authorization", "cookie"].map(|header| {
            CallEnv::new(self::source(carried(header)))
                .with_revision_cache(cache.clone())
                .at("http://widgets.example")
                .expect("a URI")
                .1
        });
        let mut held = Vec::new();
        for env in &envs {
            held.push(walked(env).await);
        }
        for env in &envs {
            assert!(
                walked(env).await.iter().all(|page| page.from_cache),
                "the walk left a slot to replay from"
            );
        }
        let env = envs[0].clone().on_unauthenticated(Arc::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
        }));
        let walks = [
            Arc::new(WalkGuard::default()),
            Arc::new(WalkGuard::default()),
        ];
        for (walk, header) in walks.iter().zip(["authorization", "cookie"]) {
            walk.get(&self::source(carried(header)), false)
                .await
                .expect("captured");
        }
        assert_ne!(
            walks[0].cache_identity(),
            walks[1].cache_identity(),
            "each walk has its own slot"
        );
        let mut guards = Vec::new();
        for walk in &walks {
            guards.push(
                walk.get(&self::source(carried("x")), false)
                    .await
                    .expect("held"),
            );
        }
        let ended = guards
            .iter()
            .map(|guard| flights.refresh(guard, &source, &refused));
        let firsts: Vec<bool> = futures_util::future::join_all(ended)
            .await
            .into_iter()
            .map(|outcome| match outcome.expect("answered") {
                Refreshed::Ended { error: None, first } => first,
                _ => panic!("nothing newer to replay"),
            })
            .collect();
        assert_eq!(firsts.iter().filter(|first| **first).count(), 1);
        let refusal = ConnectError::unauthenticated("refused");
        for (walk, first) in walks.iter().zip(firsts) {
            env.refused(&refusal, Some(walk), first);
        }
        assert_eq!(heard.load(Ordering::SeqCst), 1, "the host hears once");
        for other in &envs {
            assert!(
                walked(other).await.iter().all(|page| !page.from_cache),
                "each waiter cleared its own walk's slot"
            );
        }
    }

    #[tokio::test]
    async fn the_same_credential_with_its_headers_reordered_is_the_one_refused() {
        let refused = who(
            "a",
            &[
                ("authorization", "Bearer 1"),
                ("brefwiz-relayed-credential", "x"),
            ],
        );
        let reordered = who(
            "a",
            &[
                ("brefwiz-relayed-credential", "x"),
                ("authorization", "Bearer 1"),
            ],
        );
        let lab = Lab::new(refused.clone(), Ok(reordered));
        let outcomes = together(&RefreshFlights::default(), &lab.source(), &refused, 1).await;
        assert!(matches!(
            outcomes[0],
            Ok(Refreshed::Ended {
                error: None,
                first: true
            })
        ));
    }

    #[tokio::test]
    async fn a_call_refused_after_the_flight_finished_replays_the_renewal_without_a_refresh() {
        let lab = Lab::new(token(1), Ok(token(2)));
        lab.adopt(token(2));
        let flights = RefreshFlights::default();
        let source = lab.source();
        let first = together(&flights, &source, &token(1), 2).await;
        assert_eq!(first.len(), 2);
        let late = together(&flights, &source, &token(1), 1).await.remove(0);
        assert_eq!(replayed(late), token(2));
        assert_eq!(lab.refreshed(), 1, "the finished flight is not reused");
    }

    #[tokio::test]
    async fn a_flight_that_found_nothing_newer_but_a_plain_read_that_did_replays_that() {
        let lab = Lab::new(token(1), Ok(token(1)));
        lab.adopt(token(3));
        let outcomes = together(&RefreshFlights::default(), &lab.source(), &token(1), 3).await;
        for outcome in outcomes {
            assert_eq!(replayed(outcome), token(3));
        }
        assert_eq!(lab.refreshed(), 1);
    }

    #[tokio::test]
    async fn a_failed_flight_fails_every_waiter_and_the_next_refusal_starts_a_new_one() {
        let lab = Lab::new(token(1), Err(ConnectError::unavailable("no token")));
        let flights = RefreshFlights::default();
        let source = lab.source();
        let outcomes = together(&flights, &source, &token(1), 3).await;
        let mut firsts = 0;
        for outcome in outcomes {
            match outcome.expect("answered") {
                Refreshed::Ended { error, first } => {
                    assert_eq!(
                        error.expect("the flight's failure").code,
                        connectrpc::ErrorCode::Unavailable
                    );
                    firsts += usize::from(first);
                }
                Refreshed::Replay(_) => panic!("a failed flight renews nothing"),
            }
        }
        assert_eq!(firsts, 1);
        assert_eq!(lab.refreshed(), 1);
        *lab.renewed.lock().expect("renewed") = Ok(token(2));
        let next = together(&flights, &source, &token(1), 1).await.remove(0);
        assert_eq!(replayed(next), token(2));
        assert_eq!(lab.refreshed(), 2, "a fresh flight");
    }

    #[tokio::test]
    async fn a_flight_outlives_the_call_that_started_it_while_others_wait() {
        let parking = Parking::new().await;
        let waiter = parking.waiter_left();
        parking.lab.gate.store(true, Ordering::SeqCst);
        assert_eq!(replayed(waiter.await), token(2));
        assert_eq!(parking.lab.refreshed(), 1);
        assert_eq!(parking.lab.aborted.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_flight_every_waiter_dropped_is_aborted_and_not_joined_again() {
        let parking = Parking::new().await;
        let (lab, flights, source) = (&parking.lab, &parking.flights, &parking.source);
        let waiter = parking.waiter_left();
        assert_eq!(lab.aborted.load(Ordering::SeqCst), 0, "one waiter is left");
        drop(waiter);
        assert_eq!(lab.aborted.load(Ordering::SeqCst), 1, "the read is aborted");
        lab.gate.store(true, Ordering::SeqCst);
        let next = together(flights, source, &token(1), 1).await.remove(0);
        assert_eq!(replayed(next), token(2));
        assert_eq!(lab.refreshed(), 2, "the abandoned flight is not joined");
    }

    #[tokio::test]
    async fn a_renewal_for_another_identity_is_an_identity_change_for_each_waiter() {
        let other = who("b", &[("authorization", "Bearer 2")]);
        let lab = Lab::new(token(1), Ok(other.clone()));
        lab.adopt(other);
        let outcomes = together(&RefreshFlights::default(), &lab.source(), &token(1), 3).await;
        assert_eq!(outcomes.len(), 3);
        for outcome in outcomes {
            let refusal = outcome.err().expect("another caller");
            assert!(refusal::is_identity_changed(&refusal));
        }
        assert_eq!(lab.refreshed(), 1);
    }

    #[tokio::test]
    async fn calls_refused_on_different_credentials_make_separate_flights() {
        let lab = Lab::new(token(1), Ok(token(3)));
        lab.gate.store(false, Ordering::SeqCst);
        let flights = RefreshFlights::default();
        let source = lab.source();
        let (one, two, guard) = (token(1), token(2), captured(token(1)).await);
        let mut first = Box::pin(flights.refresh(&guard, &source, &one));
        assert!(poll_once(&mut first).is_pending());
        // The source holds another credential by the time the second call is
        // refused on it.
        *lab.plain.lock().expect("plain") = token(2);
        let mut second = Box::pin(flights.refresh(&guard, &source, &two));
        assert!(poll_once(&mut second).is_pending());
        lab.gate.store(true, Ordering::SeqCst);
        assert_eq!(replayed(first.await), token(3));
        assert_eq!(replayed(second.await), token(3));
        assert_eq!(lab.refreshed(), 2);
    }

    #[tokio::test]
    async fn a_refresh_that_cannot_read_the_plain_credential_says_why() {
        let guard = captured(who("a", &[])).await;
        let source: Arc<dyn CredentialSource> = Arc::new(broken());
        let error = RefreshFlights::default()
            .refresh(&guard, &source, &who("a", &[]))
            .await
            .err()
            .expect("unread");
        assert_eq!(error.code, connectrpc::ErrorCode::Unavailable);
    }

    #[tokio::test]
    async fn a_walk_that_cannot_read_its_credential_has_no_cache_identity() {
        let walk = WalkGuard::default();
        assert!(walk.cache_identity().is_none(), "nothing captured yet");
        assert!(walk.get(&broken(), false).await.is_err());
        assert!(walk.cache_identity().is_none(), "the capture failed");
    }

    #[tokio::test]
    async fn a_walk_keeps_the_identity_its_first_read_captured() {
        let walk = WalkGuard::default();
        let first = walk
            .get(&source(who("a", &[("authorization", "Bearer 1")])), false)
            .await
            .expect("captured");
        // A later read of another identity does not replace the guard.
        let later = walk
            .get(&source(who("b", &[("authorization", "Bearer 1")])), false)
            .await
            .expect("shared");
        assert!(Arc::ptr_eq(&first, &later));
        assert_eq!(walk.cache_identity(), first.guard.cache_identity());
        let refusal = later
            .verify(&source(who("b", &[("authorization", "Bearer 1")])))
            .await
            .expect_err("another identity");
        assert!(refusal::is_identity_changed(&refusal));
    }

    #[tokio::test]
    async fn a_walk_whose_first_read_failed_stays_failed() {
        let walk = WalkGuard::default();
        assert!(walk.get(&broken(), false).await.is_err());
        assert!(walk.get(&source(who("a", &[])), false).await.is_err());
    }

    #[tokio::test]
    async fn concurrent_reads_of_a_walk_capture_once() {
        let reads = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&reads);
        let counting = move |_: CredentialAsk| {
            count.fetch_add(1, Ordering::SeqCst);
            async { Ok(who("a", &[])) }
        };
        let walk = WalkGuard::default();
        let (left, right) =
            futures_util::future::join(walk.get(&counting, false), walk.get(&counting, false))
                .await;
        assert!(Arc::ptr_eq(&left.expect("left"), &right.expect("right")));
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_capture_that_was_dropped_unanswered_is_made_again() {
        let walk = WalkGuard::default();
        let never = |_: CredentialAsk| std::future::pending::<Result<Credential, _>>();
        assert!(
            futures_util::FutureExt::now_or_never(walk.get(&never, false)).is_none(),
            "the read is still pending"
        );
        walk.get(&source(who("a", &[])), false)
            .await
            .expect("captured by the next read");
    }

    #[tokio::test]
    async fn the_hook_names_the_identity_the_walk_captured() {
        let walk = Arc::new(WalkGuard::default());
        let hook = cache_identity_of(Arc::clone(&walk));
        let unread = hook(()).await.expect_err("nothing captured");
        assert_eq!(unread.code, connectrpc::ErrorCode::FailedPrecondition);
        walk.get(&source(who("a", &[])), false)
            .await
            .expect("captured");
        let named = hook(()).await.expect("captured");
        assert_eq!(Some(named), walk.cache_identity());
    }

    /// An env whose `n`th credential read answers `read(n)`, and how many reads
    /// it has served.
    fn reading(
        read: impl Fn(usize) -> Result<Credential, ConnectError> + Send + Sync + 'static,
    ) -> (CallEnv, Arc<AtomicUsize>) {
        let reads = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&reads);
        let env = CallEnv::new(move |_: CredentialAsk| {
            let answer = read(count.fetch_add(1, Ordering::SeqCst));
            async move { answer }
        });
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
                &[("authorization", "Bearer x")],
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
        let (env, reads) = reading(|_| Ok(who("a", &[("authorization", "Bearer x")])));
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
