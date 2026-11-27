// SPDX-License-Identifier: MIT
//! One RPC under the call helper: its span, its budget, its identity checks.

use std::pin::pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use api_bones::connect::fetch_with_retry;
use api_bones_batch::{Credential, CredentialSource, HeaderPair};
use api_bones_rt as rt;
use api_bones_transport::Deadline;
use connectrpc::{ConnectError, ErrorCode};
use futures_util::future::{Either, select};
use serde_json::json;

use super::abort::AbortFlag;
use super::attempt::Attempt;
use super::identity::{Refreshed, WalkGuard};
use super::{CallEnv, refusal};
use crate::ctx::CallCtx;
use crate::error::{CANCELED, Error};
use crate::span::{ActiveSpan, Attributes, SpanKind, SpanStatus, Tracer};

/// The RPC a call makes: its `package.Service` and its method.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Route {
    pub(crate) service: &'static str,
    pub(crate) method: &'static str,
}

impl Route {
    /// The span name: `<package.Service>/<Method>`.
    pub(crate) fn name(&self) -> String {
        format!("{}/{}", self.service, self.method)
    }

    /// The attributes of the spans the RPC opens.
    pub(crate) fn attributes(&self) -> Attributes {
        Attributes::from([
            ("rpc.system".to_owned(), json!("connect_rpc")),
            ("rpc.service".to_owned(), json!(self.service)),
            ("rpc.method".to_owned(), json!(self.method)),
        ])
    }
}

/// What one RPC attempt-set runs under.
pub(crate) struct Plan<'a> {
    pub(crate) env: &'a CallEnv,
    pub(crate) route: Route,
    /// The context the RPC's span opens under.
    pub(crate) parent: &'a CallCtx,
    /// When the budget runs out; fixed by the caller, once.
    pub(crate) deadline: rt::Instant,
    /// Repeats the method's idempotency permits.
    pub(crate) retries: u8,
    /// Whether a changed identity is the batch's own refusal.
    pub(crate) batch: bool,
    /// Whether this is a stream's open: the server is told no deadline, since
    /// the one it would apply bounds the whole stream, not just the open. The
    /// open stays inside the budget.
    pub(crate) stream: bool,
    /// Whether the env's sink sees the RPC's span. A call whose root span the
    /// sink did not record opens its spans unreported: the request still
    /// carries a trace context, and the host never hears of the children.
    pub(crate) record: bool,
    pub(crate) abort: Option<&'a AbortFlag>,
    /// The identity a whole list walk runs as; `None` when the RPC captures
    /// its own.
    pub(crate) walk: Option<&'a WalkGuard>,
}

/// What a replaying caller keeps across the sends of one run.
struct Replay<'r> {
    /// The credential the latest send carried.
    carried: &'r Mutex<Option<Credential>>,
    /// Set once the call's one renewal is used.
    spent: &'r AtomicBool,
    /// A renewed credential the first send carries.
    renewed: Option<Credential>,
}

impl Plan<'_> {
    /// Runs the RPC as one `client` span: every attempt, the whole of it
    /// inside the budget, abandonable by the abort.
    ///
    /// A refusal of the core comes back as a `ConnectError` carrying its
    /// `ErrorInfo`, which `Error::from` turns into the vocabulary's error.
    pub(crate) async fn run<T, F, Fut>(self, send: F) -> Result<T, ConnectError>
    where
        F: FnMut(Attempt) -> Fut,
        Fut: Future<Output = Result<T, ConnectError>>,
    {
        self.run_with(send, &Mutex::new(None), &AtomicBool::new(false), None)
            .await
    }

    /// As [`Plan::run`], for a caller that keeps what the sends carried and
    /// may replay the RPC: `carried` is left holding the credential the latest
    /// send carried, and a `renewed` credential is what the first send
    /// carries, with the renewal already spent. `spent` is set once the call's
    /// one renewal is used; set on entry, no send of this run asks again.
    pub(crate) async fn run_with<T, F, Fut>(
        self,
        send: F,
        carried: &Mutex<Option<Credential>>,
        spent: &AtomicBool,
        renewed: Option<Credential>,
    ) -> Result<T, ConnectError>
    where
        F: FnMut(Attempt) -> Fut,
        Fut: Future<Output = Result<T, ConnectError>>,
    {
        let env = self.env;
        let tracer = if self.record {
            env.tracer.clone()
        } else {
            Tracer::default()
        };
        let span = tracer.start(
            &self.route.name(),
            SpanKind::Client,
            self.parent,
            self.route.attributes(),
        );
        let local = WalkGuard::default();
        let walk = self.walk.unwrap_or(&local);
        // Set when a shared renewal already told the host of this refusal.
        let told = AtomicBool::new(false);
        let outcome = guarded(
            self.deadline,
            self.abort,
            self.attempts(
                &span,
                walk,
                &told,
                send,
                Replay {
                    carried,
                    spent,
                    renewed,
                },
            ),
        )
        .await;
        match &outcome {
            Ok(_) => span.end(SpanStatus::Ok),
            Err(error) if refusal::is(error, CANCELED) => span.end(SpanStatus::Unset),
            Err(error) => span.fail(&Error::from(error.clone())),
        }
        if let Err(error) = &outcome {
            if !told.load(Ordering::SeqCst) {
                env.refused(error, Some(walk), true);
            }
        }
        outcome
    }

    /// Every send of the RPC. The identity is read first (a list walk reads
    /// it once, on its first read), checked again before each send and retry,
    /// and checked once more after an answer arrived, before it is delivered.
    ///
    /// A server that refuses the credential as unauthenticated is asked once
    /// to have it renewed, together with every other call refused on the same
    /// credential. A renewal for the same caller is sent first, apart from the
    /// retry budget; one that has nothing newer, or another caller's, ends the
    /// call with the server's refusal or `IDENTITY_CHANGED`. `told` is set
    /// when the renewal already reported the refusal. `carried` holds the
    /// credential the latest send carried: what a refusal refused.
    async fn attempts<T, F, Fut>(
        &self,
        span: &ActiveSpan,
        walk: &WalkGuard,
        told: &AtomicBool,
        send: F,
        replay: Replay<'_>,
    ) -> Result<T, ConnectError>
    where
        F: FnMut(Attempt) -> Fut,
        Fut: Future<Output = Result<T, ConnectError>>,
    {
        let Replay {
            carried,
            spent,
            mut renewed,
        } = replay;
        let source: &dyn CredentialSource = &*self.env.credentials;
        let guard = walk.get(source, self.batch).await?;
        self.env.record(walk);
        let trace = trace_headers(span);
        let stated = (!self.stream).then_some(Deadline(self.deadline));
        // The sends take turns, but each one is made after an await.
        let send = Mutex::new(send);
        // A seeded renewal is the one this call may ask for, already spent.
        if renewed.is_some() {
            spent.store(true, Ordering::SeqCst);
        }
        loop {
            // Only the replay's first send carries the renewed credential;
            // a retry reads the source again.
            let mut first = renewed.take();
            let answer = fetch_with_retry(
                || {
                    let first = first.take();
                    let (guard, send, trace, carried) = (&guard, &send, &trace, carried);
                    async move {
                        let credential = match first {
                            Some(credential) => credential,
                            None => guard.verify(source).await?,
                        };
                        *carried.lock().unwrap_or_else(PoisonError::into_inner) =
                            Some(credential.clone());
                        let pending = {
                            let mut send = send.lock().unwrap_or_else(PoisonError::into_inner);
                            (*send)(Attempt::new(credential, trace, stated))
                        };
                        pending.await
                    }
                },
                self.retries,
                Some(self.deadline),
            )
            .await;
            match answer {
                Err(refused)
                    if is_refused_credential(&refused) && !spent.swap(true, Ordering::SeqCst) =>
                {
                    let held = carried
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .take();
                    // No send carried a credential: nothing renewable was refused.
                    let Some(held) = held else {
                        return Err(refused);
                    };
                    let flights = &self.env.flights;
                    let refreshed = flights.refresh(&guard, &self.env.credentials, &held).await;
                    match refreshed {
                        Err(error) => {
                            // The caller's credential cannot be renewed: its
                            // slot goes, and the host is not told twice.
                            self.env.clear_slot(walk);
                            told.store(true, Ordering::SeqCst);
                            return Err(error);
                        }
                        Ok(Refreshed::Replay(credential)) => renewed = Some(credential),
                        Ok(Refreshed::Ended { error, first }) => {
                            // The renewal is shared: every waiter clears its
                            // own walk's cache slot, only the first tells the
                            // host.
                            let error = error.unwrap_or(refused);
                            self.env.clear_slot(walk);
                            self.env.refused(&error, Some(walk), first);
                            told.store(true, Ordering::SeqCst);
                            return Err(error);
                        }
                    }
                }
                answer => {
                    let answer = answer?;
                    guard.verify(source).await?;
                    return Ok(answer);
                }
            }
        }
    }
}

/// Whether `error` is a server refusing the credential it was sent.
pub(super) fn is_refused_credential(error: &ConnectError) -> bool {
    error.code == ErrorCode::Unauthenticated && !refusal::is_identity_changed(error)
}

/// `work` inside the deadline, abandoned when the abort fires. Losing either
/// race drops `work`, which aborts whatever it has in flight.
pub(super) async fn guarded<T>(
    deadline: rt::Instant,
    abort: Option<&AbortFlag>,
    work: impl Future<Output = Result<T, ConnectError>>,
) -> Result<T, ConnectError> {
    let left = deadline.saturating_duration_since(rt::Instant::now());
    let bounded = async {
        rt::timeout(left, work)
            .await
            .unwrap_or_else(|_| Err(refusal::budget_elapsed()))
    };
    let Some(abort) = abort else {
        return bounded.await;
    };
    let bounded = pin!(bounded);
    match select(abort.aborted(), bounded).await {
        Either::Left(_) => Err(refusal::canceled()),
        Either::Right((outcome, _)) => outcome,
    }
}

impl CallEnv {
    /// Drops the revision cache slot of the identity `walk` captured; nothing
    /// when it captured none.
    pub(super) fn clear_slot(&self, walk: &WalkGuard) {
        if let (Some(cache), Some(identity)) = (&self.cache, walk.cache_identity()) {
            cache.clear_identity(identity.as_str());
        }
    }

    /// Notes that `walk`'s identity filled a slot of the env's session.
    fn record(&self, walk: &WalkGuard) {
        if let (Some(session), Some(identity)) = (&self.session, walk.cache_identity()) {
            session.record(&identity);
        }
    }

    /// What a failed RPC touches: a changed identity or an unauthenticated
    /// refusal drops the revision cache slot of the call's own identity
    /// and no other (nothing when its credential was never read), and an
    /// unauthenticated refusal from a server tells the host when `notify`.
    pub(super) fn refused(&self, error: &ConnectError, walk: Option<&WalkGuard>, notify: bool) {
        let identity_changed = refusal::is_identity_changed(error);
        let unauthenticated = error.code == ErrorCode::Unauthenticated && !identity_changed;
        if identity_changed || unauthenticated {
            if let Some(walk) = walk {
                self.clear_slot(walk);
            }
        }
        if unauthenticated && notify {
            if let Some(notify) = &self.unauthenticated {
                notify();
            }
        }
    }
}

/// The trace context a request carries: the `traceparent` of the span it runs
/// under and, when there is one, its `tracestate`.
fn trace_headers(span: &ActiveSpan) -> Vec<HeaderPair> {
    let mut headers = vec![("traceparent".to_owned(), span.traceparent().into_bytes())];
    if let Some(state) = span.tracestate() {
        headers.push(("tracestate".to_owned(), state.as_bytes().to_vec()));
    }
    headers
}
