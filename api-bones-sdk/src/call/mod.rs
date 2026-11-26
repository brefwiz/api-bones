// SPDX-License-Identifier: MIT
//! The call helper every Layer 2 method runs through.
//!
//! One helper decides, once and in Rust, what every language binding would
//! otherwise decide again:
//!
//! - **The call budget.** [`CALL_BUDGET`] is one fixed platform constant, with
//!   no policy field and no caller knob. The deadline is computed once, when
//!   the call starts, and that one instant feeds the [`Deadline`] a request
//!   carries, the [`rt::timeout`] around the whole call, and the `deadline` of
//!   the retry loop. A unary call and a batch own the whole budget. A list
//!   owns one budget per page fetch, not one for the walk. A server stream
//!   owns it only until its response head arrives; its lifetime afterwards
//!   belongs to the stream, never to a timeout.
//! - **The caller's identity.** It is read when the call starts, and checked
//!   again before every send and retry and once more after an answer arrived,
//!   before it is delivered. A change refuses the call and clears the revision
//!   cache; the credential a send carries is the one read just before it. A
//!   list walk reads it once, when the walk starts, and every page is
//!   checked against that one identity, so the walk never continues as
//!   another; a page that was fetched ahead, or replayed from the revision
//!   cache, is checked again before it is delivered. A server stream reads
//!   the identity at open and checks it again before each item is delivered.
//!   A change ends the stream with `IDENTITY_CHANGED`.
//! - **The trace context.** The call reads [`CallCtx::current`] synchronously
//!   when it starts, opens its spans against the env's sink, and sends the
//!   `traceparent` (and `tracestate`) of the span it opened.
//! - **Abort.** An [`AbortFlag`] drops the call's future and fails it with
//!   `canceled`.
//!
//! The helper does not build requests: the caller's fetch closure gets an
//! [`Attempt`] and makes the one RPC with it.
//!
//! [`Deadline`]: api_bones_transport::Deadline
//! [`rt::timeout`]: api_bones_rt::timeout

mod abort;
mod attempt;
#[cfg(any(test, all(feature = "wasm", target_arch = "wasm32")))]
mod host;
mod identity;
mod refusal;
mod run;

use std::sync::Arc;
use std::time::Duration;

use api_bones::connect::{Idempotency, retries_for};
use api_bones_batch::{Anonymous, CredentialSource};
use api_bones_page::{
    ListCall as PageCall, MetadataFetch, Page, PageOptions, PagedRequest, PagedResponse,
    RevisionCache,
};
use api_bones_rt::{BoxFuture, MaybeSend, MaybeSync, SharedFn};
use connectrpc::ConnectError;
use connectrpc::client::ClientConfig;
use futures_util::{Stream as FutureStream, StreamExt as _, stream};

pub use abort::{AbortFlag, Aborted};
pub use api_bones_batch::not_modified;
pub use api_bones_page::{CanonicalPagedRequest, ListMetadata, ListResponse};
pub use attempt::{Attempt, WithDeadline};

use crate::ctx::CallCtx;
use crate::error::Error;
use crate::handle::{Batch, Handle, ListCall, Stream};
use crate::span::{ActiveSpan, SpanKind, Tracer};
use identity::WalkGuard;
use run::{Plan, Route};

/// The whole-call time limit, retries included. A fixed platform constant:
/// there is no policy field and no consumer knob.
pub const CALL_BUDGET: Duration = Duration::from_secs(30);

/// A callback with no arguments, shared by the clones of an env.
#[cfg(not(target_arch = "wasm32"))]
pub type Notify = Arc<dyn Fn() + Send + Sync>;
/// A callback with no arguments, shared by the clones of an env.
#[cfg(target_arch = "wasm32")]
pub type Notify = Arc<dyn Fn()>;

/// A page fetch: the request of one page and the [`Attempt`] to send it with.
/// The answer carries whether the server confirmed the cached revision, read
/// with [`not_modified`] from the response headers.
pub type PageSend<Req, Resp> =
    SharedFn<(Req, Attempt), BoxFuture<'static, Result<ListResponse<Resp>, ConnectError>>>;

/// What every call of one client shares: where its credential comes from, where
/// its spans go and what a failure of the credential touches.
#[derive(Clone)]
pub struct CallEnv {
    pub(crate) credentials: Arc<dyn CredentialSource>,
    pub(crate) tracer: Tracer,
    pub(crate) cache: Option<RevisionCache>,
    /// The server the env's calls go to, as [`CallEnv::at`] parsed it. The
    /// revision cache is scoped to it; an env without one caches nothing.
    pub(crate) origin: Option<Arc<str>>,
    pub(crate) unauthenticated: Option<Notify>,
}

impl CallEnv {
    /// An env whose calls are made as `credentials` says, re-read before
    /// every send.
    #[must_use]
    pub fn new(credentials: impl CredentialSource) -> Self {
        Self {
            credentials: Arc::new(credentials),
            tracer: Tracer::default(),
            cache: None,
            origin: None,
            unauthenticated: None,
        }
    }

    /// An env whose calls present no credential.
    #[must_use]
    pub fn anonymous() -> Self {
        Self::new(Anonymous)
    }

    /// Reports the env's spans to `tracer`.
    #[must_use]
    pub fn with_tracer(mut self, tracer: Tracer) -> Self {
        self.tracer = tracer;
        self
    }

    /// Lets a list replay pages from `cache` once the server confirms their
    /// revision. The cache holds one slot per caller identity, so envs of
    /// different callers may share it; a call that finds its identity changed
    /// or is refused as unauthenticated drops its own caller's slot. A list
    /// caches only on an env bound to a server with [`CallEnv::at`].
    #[must_use]
    pub fn with_revision_cache(mut self, cache: RevisionCache) -> Self {
        self.cache = Some(cache);
        self
    }

    /// The client configuration of the server at `base_url`, and this env
    /// bound to that server: the revision cache is scoped to the server, so
    /// two deployments sharing identities and revisions never replay each
    /// other's pages.
    ///
    /// # Errors
    /// `invalid_argument` when `base_url` is not a URI.
    pub fn at(&self, base_url: &str) -> Result<(ClientConfig, Self), Error> {
        let uri: http::Uri = base_url
            .parse()
            .map_err(|_| Error::invalid_argument("base_url is not a URI"))?;
        let mut env = self.clone();
        env.origin = Some(Arc::from(uri.to_string()));
        Ok((ClientConfig::new(uri), env))
    }

    /// Calls `notify` whenever a server refuses a call as unauthenticated.
    #[must_use]
    pub fn on_unauthenticated(mut self, notify: Notify) -> Self {
        self.unauthenticated = Some(notify);
        self
    }

    /// Starts a call of `service`'s `method` (for example
    /// `"acme.v1.Widgets"` and `"GetWidget"`).
    ///
    /// The caller's trace context is read now, from [`CallCtx::current`], and
    /// the call's deadline is fixed now: the budget starts counting here.
    #[must_use]
    pub fn call(&self, service: &'static str, method: &'static str) -> Call {
        Call {
            env: self.clone(),
            route: Route { service, method },
            ctx: CallCtx::current(),
            deadline: api_bones_rt::Instant::now() + CALL_BUDGET,
            abort: None,
        }
    }
}

/// One call, started by [`CallEnv::call`] and finished by exactly one of
/// `unary`, `stream`, `batch` or `list`.
pub struct Call {
    env: CallEnv,
    route: Route,
    ctx: CallCtx,
    deadline: api_bones_rt::Instant,
    abort: Option<AbortFlag>,
}

impl Call {
    /// Abandons the call when `abort` fires.
    #[must_use]
    pub fn with_abort(mut self, abort: AbortFlag) -> Self {
        self.abort = Some(abort);
        self
    }

    /// One request, one response, inside the whole-call budget.
    ///
    /// # Errors
    /// The server's failure, or a refusal of the core: `CALL_BUDGET_ELAPSED`,
    /// `IDENTITY_CHANGED` or `CANCELED`.
    pub async fn unary<T, F, Fut>(self, idempotency: Idempotency, send: F) -> Result<T, Error>
    where
        F: FnMut(Attempt) -> Fut,
        Fut: Future<Output = Result<T, ConnectError>>,
    {
        Plan {
            env: &self.env,
            route: self.route,
            parent: &self.ctx,
            deadline: self.deadline,
            retries: retries_for(idempotency),
            batch: false,
            stream: false,
            abort: self.abort.as_ref(),
            walk: None,
        }
        .run(send)
        .await
        .map_err(Error::from)
    }

    /// A server stream. The budget bounds the call only until the response
    /// head arrives; the items that follow are bound by nothing but the
    /// stream's own lifetime and an abort. The open states no timeout to the
    /// server, which would apply it to the whole stream. The identity, unlike
    /// the budget, is checked on every item.
    ///
    /// # Errors
    /// As [`Call::unary`], for the open; the stream's own failures arrive
    /// through the handle.
    pub async fn stream<Item, S, F, Fut>(
        self,
        idempotency: Idempotency,
        open: F,
    ) -> Result<Stream<Item>, Error>
    where
        Item: MaybeSend + 'static,
        S: FutureStream<Item = Result<Item, ConnectError>> + MaybeSend + 'static,
        F: FnMut(Attempt) -> Fut,
        Fut: Future<Output = Result<S, ConnectError>>,
    {
        let walk = self.walk_span();
        let walk_ctx = walk.ctx();
        // The open captures the identity; every item is checked against it.
        let guard = Arc::new(WalkGuard::default());
        let head = Plan {
            env: &self.env,
            route: self.route,
            parent: &walk_ctx,
            deadline: self.deadline,
            retries: retries_for(idempotency),
            batch: false,
            stream: true,
            abort: self.abort.as_ref(),
            walk: Some(guard.as_ref()),
        }
        .run(open)
        .await;
        match head {
            Ok(items) => {
                let items: api_bones_rt::BoxStream<'static, Result<Item, Error>> =
                    Box::pin(items.map(|item| item.map_err(Error::from)));
                let items = identity::checked(items, self.env.clone(), guard);
                Ok(self.handle(items, walk))
            }
            Err(error) => {
                let error = Error::from(error);
                walk.fail(&error);
                Err(error)
            }
        }
    }

    /// A batch: the whole call, retries included, runs inside one budget and
    /// is delivered as one chunk of per-item results in request order.
    ///
    /// The budget counts from [`CallEnv::call`]; the work itself starts at the
    /// first `next()` on the handle. A changed identity refuses the chunk with
    /// `BATCH_IDENTITY_CHANGED`.
    #[must_use]
    pub fn batch<T, F, Fut>(self, idempotency: Idempotency, run: F) -> Batch<T>
    where
        T: MaybeSend + 'static,
        F: FnMut(Attempt) -> Fut + MaybeSend + 'static,
        Fut: Future<Output = Result<Vec<Result<T, ConnectError>>, ConnectError>>
            + MaybeSend
            + 'static,
    {
        let walk = self.walk_span();
        let ctx = walk.ctx();
        let Self {
            env,
            route,
            deadline,
            abort,
            ..
        } = self;
        let retries = retries_for(idempotency);
        let chunk = async move {
            Plan {
                env: &env,
                route,
                parent: &ctx,
                deadline,
                retries,
                batch: true,
                stream: false,
                abort: abort.as_ref(),
                walk: None,
            }
            .run(run)
            .await
            .map(|results| {
                results
                    .into_iter()
                    .map(|result| result.map_err(Error::from))
                    .collect::<Vec<_>>()
            })
            .map_err(Error::from)
        };
        Handle::new(Box::pin(stream::once(chunk)), walk)
    }

    /// A list: one [`Page`] per `next()`. Each page
    /// fetch owns a whole budget of its own, so a long walk is never cut off
    /// as a whole, and each is a `client` span under the walk's `internal` one.
    ///
    /// The whole walk runs as one identity, the one its first page started
    /// as: a later page that finds another identity is refused with
    /// `IDENTITY_CHANGED`, which ends the walk. A page fetched ahead is checked
    /// against that identity again when it is about to be delivered, so it is
    /// never delivered to a caller who switched after it was fetched.
    ///
    /// An env that carries a revision cache makes the walk revalidate: the
    /// first page asks the server to confirm the cached revision, and a
    /// confirmation replays the cached pages (`from_cache`). The cache is
    /// scoped to the server, the route, the request without its page, and the
    /// identity.
    #[must_use]
    pub fn list<Req, Resp>(
        self,
        idempotency: Idempotency,
        request: Req,
        send: PageSend<Req, Resp>,
        options: PageOptions,
    ) -> ListCall<Resp::Item>
    where
        Req: PagedRequest + CanonicalPagedRequest,
        Resp: PagedResponse,
        Resp::Item: MaybeSend + MaybeSync + 'static,
    {
        let key = self.cache_key(&request);
        let walk = self.walk_span();
        let ctx = walk.ctx();
        let Self {
            env, route, abort, ..
        } = self;
        let retries = retries_for(idempotency);
        // One identity for the whole walk, captured when it starts.
        let identity = Arc::new(WalkGuard::default());
        let checks = (env.clone(), Arc::clone(&identity));
        let cache_identity = {
            let env = env.clone();
            let identity = Arc::clone(&identity);
            Arc::new(move || identity.cache_identity(&*env.credentials))
        };
        let cache = env.cache.clone();
        let fetch = Arc::new(
            move |request: Req,
                  list: Option<ListMetadata>|
                  -> BoxFuture<'static, Result<ListResponse<Resp>, ConnectError>> {
                let env = env.clone();
                let ctx = ctx.clone();
                let abort = abort.clone();
                let identity = Arc::clone(&identity);
                let send = Arc::clone(&send);
                Box::pin(async move {
                    // Each page starts its own budget.
                    Plan {
                        env: &env,
                        route,
                        parent: &ctx,
                        deadline: api_bones_rt::Instant::now() + CALL_BUDGET,
                        retries,
                        batch: false,
                        stream: false,
                        abort: abort.as_ref(),
                        walk: Some(identity.as_ref()),
                    }
                    // The attempt closure owns the request: borrowing it across
                    // the await would need `Req: Sync`, which a request only
                    // promises to be `Send`.
                    .run(move |attempt| {
                        let attempt = match &list {
                            Some(list) => attempt.with_list(list),
                            None => attempt,
                        };
                        send((request.clone(), attempt))
                    })
                    .await
                })
            },
        );
        let pages = match (cache, key) {
            (Some(cache), Some(key)) => {
                let revalidating: MetadataFetch<Req, Resp> =
                    Arc::new(move |request, list| fetch(request, Some(list)));
                PageCall::generated(
                    route.method,
                    idempotency,
                    revalidating,
                    request,
                    cache,
                    cache_identity,
                    &key,
                )
            }
            _ => PageCall::new(
                route.method,
                idempotency,
                Arc::new(move |request| {
                    let page = fetch(request, None);
                    Box::pin(async move { page.await.map(|answer| answer.body) })
                }),
                request,
            ),
        }
        .options(options);
        // A page fetched ahead was fetched as whoever the caller was then.
        let pages: api_bones_rt::BoxStream<'static, Result<Page<Resp::Item>, Error>> =
            Box::pin(pages.pages().map(|page| page.map_err(Error::from)));
        let (env, identity) = checks;
        Handle::new(identity::checked(pages, env, identity), walk)
    }

    /// What a revision cache entry of this list is keyed on: the length-prefixed
    /// origin, the route, then the request with its page cleared. The zero byte
    /// ends the route name, so no request encoding can extend it into another
    /// route. An env bound to no origin has no key: its lists are not cached.
    fn cache_key<Req: PagedRequest + CanonicalPagedRequest>(
        &self,
        request: &Req,
    ) -> Option<Vec<u8>> {
        self.env.cache.as_ref()?;
        let origin = self.env.origin.as_deref()?;
        let mut bare = request.clone();
        bare.set_page("", None);
        let mut key = (origin.len() as u64).to_le_bytes().to_vec();
        key.extend_from_slice(origin.as_bytes());
        key.extend_from_slice(self.route.name().as_bytes());
        key.push(0);
        key.extend_from_slice(&bare.encode());
        Some(key)
    }

    /// The `internal` span of everything the call does.
    fn walk_span(&self) -> ActiveSpan {
        self.env.tracer.start(
            &self.route.name(),
            SpanKind::Internal,
            &self.ctx,
            self.route.attributes(),
        )
    }

    /// A handle over `items`, cancelable by the call's abort.
    fn handle<Item: MaybeSend + 'static>(
        &self,
        items: api_bones_rt::BoxStream<'static, Result<Item, Error>>,
        walk: ActiveSpan,
    ) -> Stream<Item> {
        match &self.abort {
            Some(abort) => Handle::new(abort::abortable(items, abort.clone()), walk),
            None => Handle::new(items, walk),
        }
    }
}
