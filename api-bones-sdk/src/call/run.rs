// SPDX-License-Identifier: MIT
//! One RPC under the call helper: its span, its budget, its identity checks.

use std::pin::pin;

use api_bones::connect::fetch_with_retry;
use api_bones_batch::{CredentialSource, HeaderPair};
use api_bones_rt as rt;
use api_bones_transport::Deadline;
use connectrpc::{ConnectError, ErrorCode};
use futures_util::future::{Either, select};
use serde_json::json;

use super::abort::AbortFlag;
use super::attempt::Attempt;
use super::identity::{IdentityGuard, WalkGuard};
use super::{CallEnv, refusal};
use crate::ctx::CallCtx;
use crate::error::{CANCELED, Error};
use crate::span::{ActiveSpan, Attributes, SpanKind, SpanStatus};

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
    pub(crate) abort: Option<&'a AbortFlag>,
    /// The identity a whole list walk runs as; `None` when the RPC captures
    /// its own.
    pub(crate) walk: Option<&'a WalkGuard>,
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
        let env = self.env;
        let span = env.tracer.start(
            &self.route.name(),
            SpanKind::Client,
            self.parent,
            self.route.attributes(),
        );
        let outcome = self.abortable(&span, send).await;
        match &outcome {
            Ok(_) => span.end(SpanStatus::Ok),
            Err(error) if refusal::is(error, CANCELED) => span.end(SpanStatus::Unset),
            Err(error) => span.fail(&Error::from(error.clone())),
        }
        if let Err(error) = &outcome {
            env.refused(error);
        }
        outcome
    }

    /// The attempt-set raced against the abort. Losing the race drops the
    /// attempt-set's future, which aborts the request in flight.
    async fn abortable<T, F, Fut>(&self, span: &ActiveSpan, send: F) -> Result<T, ConnectError>
    where
        F: FnMut(Attempt) -> Fut,
        Fut: Future<Output = Result<T, ConnectError>>,
    {
        let bounded = self.bounded(span, send);
        let Some(abort) = self.abort else {
            return bounded.await;
        };
        let bounded = pin!(bounded);
        match select(abort.aborted(), bounded).await {
            Either::Left(_) => Err(refusal::canceled()),
            Either::Right((outcome, _)) => outcome,
        }
    }

    /// The attempt-set inside the call's one deadline.
    async fn bounded<T, F, Fut>(&self, span: &ActiveSpan, send: F) -> Result<T, ConnectError>
    where
        F: FnMut(Attempt) -> Fut,
        Fut: Future<Output = Result<T, ConnectError>>,
    {
        let left = self.deadline.saturating_duration_since(rt::Instant::now());
        rt::timeout(left, self.attempts(span, send))
            .await
            .unwrap_or_else(|_| Err(refusal::budget_elapsed()))
    }

    /// Every send of the RPC. The identity is read first (a list walk reads
    /// it once, on its first page), checked again
    /// before each send and retry, and checked once more after an answer
    /// arrived, before it is delivered.
    async fn attempts<T, F, Fut>(&self, span: &ActiveSpan, mut send: F) -> Result<T, ConnectError>
    where
        F: FnMut(Attempt) -> Fut,
        Fut: Future<Output = Result<T, ConnectError>>,
    {
        let source: &dyn CredentialSource = &*self.env.credentials;
        let own;
        let guard = if let Some(walk) = self.walk {
            walk.get(source, self.batch)?
        } else {
            own = IdentityGuard::capture(source, self.batch)?;
            &own
        };
        let trace = trace_headers(span);
        let deadline = Deadline(self.deadline);
        let answer = fetch_with_retry(
            || {
                let pending = guard
                    .verify(source)
                    .map(|credential| send(Attempt::new(credential, &trace, deadline)));
                async move { pending?.await }
            },
            self.retries,
            Some(self.deadline),
        )
        .await?;
        guard.verify(source)?;
        Ok(answer)
    }
}

impl CallEnv {
    /// What a failed RPC touches: a changed identity or an unauthenticated
    /// refusal drops the revision cache, and an unauthenticated refusal from
    /// a server tells the host.
    pub(super) fn refused(&self, error: &ConnectError) {
        let identity_changed = refusal::is_identity_changed(error);
        let unauthenticated = error.code == ErrorCode::Unauthenticated && !identity_changed;
        if identity_changed || unauthenticated {
            if let Some(cache) = &self.cache {
                cache.clear();
            }
        }
        if unauthenticated {
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
