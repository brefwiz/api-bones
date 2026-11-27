// SPDX-License-Identifier: MIT
//! [`ListCall`]: one List RPC as a stream of pages or of items.

use api_bones::connect::{Idempotency, fetch_with_retry, retries_for};
use api_bones_rt::{BoxFuture, MaybeSend, MaybeSync, SharedFn};
use connectrpc::ConnectError;
use futures_core::Stream;
use futures_util::StreamExt as _;
use sha2::{Digest, Sha256};
use std::sync::Arc;

use crate::error::PageError;
use crate::prefetch::Prefetch;

/// The transport call for one page: a request in, a response out.
pub type Fetch<Req, Resp> = SharedFn<Req, BoxFuture<'static, Result<Resp, ConnectError>>>;

/// Metadata supplied to generated unary List transport glue.
#[derive(Clone, Default)]
pub struct ListMetadata {
    /// Always send `bones-list-cache: 1` for this transport.
    pub cache_opt_in: bool,
    /// Quoted revision for a complete live chain, on page one only.
    pub if_none_match: Option<String>,
}

/// Generated transport response, retaining the List protocol marker.
pub struct ListResponse<Resp> {
    /// Generated response body.
    pub body: Resp,
    /// Whether `bones-not-modified: 1` was received.
    pub not_modified: bool,
}

/// Generated unary adapter preserving request and response metadata.
#[cfg(not(target_arch = "wasm32"))]
pub type MetadataFetch<Req, Resp> = Arc<
    dyn Fn(Req, ListMetadata) -> BoxFuture<'static, Result<ListResponse<Resp>, ConnectError>>
        + Send
        + Sync,
>;
/// Generated unary adapter preserving request and response metadata.
#[cfg(target_arch = "wasm32")]
pub type MetadataFetch<Req, Resp> =
    Arc<dyn Fn(Req, ListMetadata) -> BoxFuture<'static, Result<ListResponse<Resp>, ConnectError>>>;

/// The identity a cached chain belongs to: never empty, so no cache method
/// is ever keyed on an unnamed caller.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheIdentity(Arc<str>);

impl CacheIdentity {
    /// The identity named `name`; `None` when `name` is empty.
    #[must_use]
    pub fn new(name: impl Into<Arc<str>>) -> Option<Self> {
        let name = name.into();
        (!name.is_empty()).then_some(Self(name))
    }

    /// The identity's name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Resolves the identity a cached chain belongs to, when a walk first needs
/// it and again at every page boundary. A failure ends the walk: it is never
/// continued uncached.
pub type IdentityFn = SharedFn<(), BoxFuture<'static, Result<CacheIdentity, ConnectError>>>;

/// Whether to consult and rebuild a revision chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CacheMode {
    /// Revalidate a complete live chain.
    #[default]
    Use,
    /// Fetch normally without consulting or updating the cache.
    Bypass,
}

struct CacheBinding {
    cache: crate::RevisionCache,
    identity: IdentityFn,
    key: [u8; 32],
}

/// How a collection size was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TotalKind {
    /// An estimate within the request's filter and caller authorization; may be off.
    Estimate,
    /// An exact count within the request's filter and caller authorization.
    Exact,
}

/// The size visible under the request's filter and caller authorization,
/// present only when the method opted in, with the same scope as the revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Total {
    /// The size.
    pub value: u64,
    /// How it was obtained.
    pub kind: TotalKind,
}

/// The `bones.v1.PageResponse` of one response.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageInfo {
    /// Cursor of the next page; empty at the end.
    pub next_cursor: String,
    /// Opaque revision of the list.
    pub revision: String,
    /// Collection size, when the method opted in.
    pub total: Option<Total>,
}

/// A request that carries a `bones.v1.PageRequest`.
pub trait PagedRequest: Clone + MaybeSend + 'static {
    /// Set the cursor (empty = first page) and the page-size hint.
    fn set_page(&mut self, cursor: &str, page_size: Option<u32>);
}

/// A paged request that encodes itself for the revision cache key.
pub trait CanonicalPagedRequest {
    /// The wire encoding of every request field. The caller clears the page
    /// first, so a cursor never reaches the key.
    fn encode(&self) -> Vec<u8>;
}

/// A response that carries one repeated field and a `bones.v1.PageResponse`.
pub trait PagedResponse: MaybeSend + 'static {
    /// One element of the repeated field.
    type Item: MaybeSend + MaybeSync + 'static;

    /// Take the items and the page info out of the response.
    fn split(self) -> (Vec<Self::Item>, PageInfo);

    /// The protobuf-encoded length of one item, the measure the revision
    /// cache holds chains by.
    fn encoded_len(item: &Self::Item) -> usize;
}

/// Per-iteration options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageOptions {
    /// Revision cache policy.
    pub cache: CacheMode,
    /// Page-size hint; the server clamps it. `None` or zero = server default.
    pub page_size: Option<u32>,
    /// Pages fetched ahead of the consumer: 0 reads none, any larger value
    /// reads one ahead.
    pub prefetch: u8,
}

impl Default for PageOptions {
    fn default() -> Self {
        Self {
            cache: CacheMode::Use,
            page_size: None,
            prefetch: 1,
        }
    }
}

/// One delivered page.
#[derive(Debug, Clone)]
pub struct Page<T> {
    /// Replayed after the server confirmed the chain revision.
    pub from_cache: bool,
    /// True exactly when the response carried a non-empty next cursor.
    pub has_next: bool,
    /// The page's items.
    pub items: Arc<[T]>,
    /// Opaque revision of the list.
    pub revision: String,
    /// Collection size, when the method opted in.
    pub total: Option<Total>,
}

type Outcome<Resp> = Result<Resp, ConnectError>;

/// One List RPC, ready to iterate.
pub struct ListCall<Req, Resp: PagedResponse> {
    method: &'static str,
    idempotency: Idempotency,
    fetch: Fetch<Req, Resp>,
    request: Req,
    options: PageOptions,
    first: Option<(Vec<Resp::Item>, PageInfo)>,
    metadata_fetch: Option<MetadataFetch<Req, Resp>>,
    cache: Option<CacheBinding>,
    first_only: bool,
}

impl<Req: PagedRequest, Resp: PagedResponse> ListCall<Req, Resp> {
    /// A call that fetches `request` through `fetch`. `idempotency` is the
    /// RPC's declared contract; only `NoSideEffects` and `Idempotent` fetches
    /// are ever repeated.
    #[must_use]
    pub fn new(
        method: &'static str,
        idempotency: Idempotency,
        fetch: Fetch<Req, Resp>,
        request: Req,
    ) -> Self {
        Self {
            method,
            idempotency,
            fetch,
            request,
            options: PageOptions::default(),
            first: None,
            metadata_fetch: None,
            cache: None,
            first_only: false,
        }
    }

    /// Construct emitted SDK glue with its client-owned cache, credential
    /// identity source and canonical request encoding (cursor excluded).
    /// Applications call the generated List method rather than this adapter.
    #[must_use]
    pub fn generated(
        method: &'static str,
        idempotency: Idempotency,
        fetch: MetadataFetch<Req, Resp>,
        request: Req,
        cache: crate::RevisionCache,
        identity: IdentityFn,
        canonical_request: &[u8],
    ) -> Self {
        let plain = fetch.clone();
        let mut call = Self::new(
            method,
            idempotency,
            Arc::new(move |request| {
                let future = plain(
                    request,
                    ListMetadata {
                        cache_opt_in: true,
                        if_none_match: None,
                    },
                );
                Box::pin(async move { future.await.map(|response| response.body) })
            }),
            request,
        );
        call.metadata_fetch = Some(fetch);
        call.cache = Some(CacheBinding {
            cache,
            identity,
            key: Sha256::digest(canonical_request).into(),
        });
        call
    }

    /// A call that continues a list whose first page is already in hand, as a
    /// grouped batch returns it: `first` is delivered as page one, then the
    /// iteration follows `next_cursor` (empty = the list ended there).
    #[must_use]
    pub fn from_first_page(
        method: &'static str,
        idempotency: Idempotency,
        fetch: Fetch<Req, Resp>,
        request: Req,
        first: Page<Resp::Item>,
        next_cursor: String,
    ) -> Self
    where
        Resp::Item: Clone,
    {
        let mut call = Self::new(method, idempotency, fetch, request);
        call.first = Some((
            first.items.to_vec(),
            PageInfo {
                next_cursor,
                revision: first.revision,
                total: first.total,
            },
        ));
        call
    }

    /// Deliver a grouped batch's first page, bypassing revision-cache reads
    /// and writes for the entire continuation chain.
    #[must_use]
    pub fn seed_first_page(mut self, first: &Page<Resp::Item>, next_cursor: String) -> Self
    where
        Resp::Item: Clone,
    {
        let info = PageInfo {
            next_cursor,
            revision: first.revision.clone(),
            total: first.total,
        };
        self.first = Some((first.items.to_vec(), info));
        self
    }

    /// Replace the iteration options.
    #[must_use]
    pub fn options(mut self, options: PageOptions) -> Self {
        self.options = options;
        self
    }

    /// The pages fetched ahead of the consumer: 0 reads only the pages asked
    /// for, any larger value reads one ahead.
    #[must_use]
    pub fn prefetch(mut self, pages: u8) -> Self {
        self.options.prefetch = pages.min(1);
        self
    }

    /// One call, no following of `next_cursor`.
    ///
    /// # Errors
    /// The call's failure, after delivering nothing.
    ///
    /// # Panics
    /// Panics if the page stream ends before its first result.
    pub async fn first_page(mut self) -> Result<Page<Resp::Item>, PageError> {
        self.first_only = true;
        self.options.prefetch = 0;
        Box::pin(self.raw_pages())
            .next()
            .await
            .expect("first page fetch always answers or fails")
    }

    /// Every page, following `next_cursor` to the end. A failure ends the
    /// stream with one error after the pages already delivered.
    pub fn pages(
        self,
    ) -> impl Stream<Item = Result<Page<Resp::Item>, PageError>> + MaybeSend + 'static {
        self.raw_pages()
    }

    /// Every item, following `next_cursor` to the end.
    pub fn items(self) -> impl Stream<Item = Result<Resp::Item, PageError>> + MaybeSend + 'static
    where
        Resp::Item: Clone,
    {
        self.raw_pages()
            .flat_map(|result| {
                let entries: Vec<Result<Resp::Item, PageError>> = match result {
                    Ok(page) => page.items.iter().cloned().map(Ok).collect(),
                    Err(error) => vec![Err(error)],
                };
                futures_util::stream::iter(entries)
            })
            .fuse()
    }

    fn raw_pages(
        self,
    ) -> impl Stream<Item = Result<Page<Resp::Item>, PageError>> + MaybeSend + 'static {
        let cache_enabled = self.first.is_none() && self.options.cache == CacheMode::Use;
        let mut cache = self.cache;
        if let Some(binding) = &mut cache {
            let mut digest = Sha256::new();
            digest.update(binding.key);
            digest.update(self.options.page_size.unwrap_or_default().to_le_bytes());
            binding.key = digest.finalize().into();
        }
        let retries = retries_for(self.idempotency);
        let state = Walk {
            cache_enabled,
            first_only: self.first_only,
            measurement: crate::span::Measurement::new(self.method),
            fetch: self.fetch,
            request: self.request,
            retries,
            options: self.options,
            pending: None,
            cursor: Some(String::new()),
            first: self.first,
            pages: 0,
            items: 0,
            method: self.method,
            metadata_fetch: self.metadata_fetch,
            cache,
            identity: None,
            replay: None,
            replaying: false,
            chain: crate::cache::ChainBuilder::default(),
            generation: None,
        };
        futures_util::stream::unfold(state, |mut walk| async move {
            let result = walk.advance().await?;
            Some((result, walk))
        })
        .fuse()
    }
}

/// The iteration state between two pages.
struct Walk<Req, Resp: PagedResponse> {
    cache_enabled: bool,
    first_only: bool,
    generation: Option<u64>,
    method: &'static str,
    metadata_fetch: Option<MetadataFetch<Req, Resp>>,
    cache: Option<CacheBinding>,
    /// The identity the walk started as; `None` until the first page boundary
    /// resolves it.
    identity: Option<CacheIdentity>,
    replay: Option<Vec<Page<Resp::Item>>>,
    replaying: bool,
    chain: crate::cache::ChainBuilder<Resp::Item>,
    measurement: crate::span::Measurement,
    fetch: Fetch<Req, Resp>,
    request: Req,
    retries: u8,
    options: PageOptions,
    /// The fetch of the cursor in `cursor`, already running.
    pending: Option<Prefetch<Outcome<Resp>>>,
    /// The cursor of the next page; `None` once the list ended or failed.
    cursor: Option<String>,
    /// A first page handed in by the caller, delivered before any fetch.
    first: Option<(Vec<Resp::Item>, PageInfo)>,
    pages: u32,
    items: u64,
}

impl<Req: PagedRequest, Resp: PagedResponse> Walk<Req, Resp> {
    async fn advance(&mut self) -> Option<Result<Page<Resp::Item>, PageError>> {
        if self.cursor.is_none() && !self.replaying {
            return None;
        }
        if let Some(error) = self.guard().await {
            return Some(Err(error));
        }
        if self.replaying {
            let pages = self.replay.as_mut().expect("confirmed replay");
            if pages.is_empty() {
                self.replaying = false;
                return None;
            }
            let page = pages.remove(0);
            self.pages += 1;
            self.items += page.items.len() as u64;
            self.measurement.page(!page.items.is_empty());
            if pages.is_empty() {
                self.measurement.finish(true, false);
            }
            return Some(Ok(page));
        }
        let cursor = self.cursor.take()?;
        let delivered = if let Some(first) = self.first.take() {
            Ok(first)
        } else {
            let outcome = match self.pending.take() {
                Some(prefetch) => {
                    self.measurement.prefetch_hits += 1;
                    prefetch.finish().await.unwrap_or_else(|| {
                        Err(ConnectError::internal(
                            "page fetch ended before it answered",
                        ))
                    })
                }
                None => {
                    if let Some(fetch) = &self.metadata_fetch {
                        let revision = self
                            .replay
                            .as_ref()
                            .and_then(|pages| pages.first())
                            .map(|page| format!("\"{}\"", page.revision));
                        let request = self.page_request(&cursor);
                        let response = fetch_with_retry(
                            move || {
                                fetch(
                                    request.clone(),
                                    ListMetadata {
                                        cache_opt_in: true,
                                        if_none_match: revision.clone(),
                                    },
                                )
                            },
                            self.retries,
                            None,
                        )
                        .await;
                        // A failed answer keeps its own code; only an answer
                        // that would be delivered is checked.
                        if response.is_ok() {
                            if let Some(error) = self.guard().await {
                                return Some(Err(error));
                            }
                        }
                        match response {
                            Ok(response) if response.not_modified && self.replay.is_some() => {
                                self.replaying = true;
                                self.measurement.not_modified_hits += 1;
                                let pages = self.replay.as_mut().expect("replayable chain");
                                let page = pages.remove(0);
                                self.pages += 1;
                                self.items += page.items.len() as u64;
                                self.measurement.page(!page.items.is_empty());
                                if pages.is_empty() || self.first_only {
                                    self.measurement.finish(true, false);
                                }
                                return Some(Ok(page));
                            }
                            Ok(response) if response.not_modified => Err(ConnectError::internal(
                                "list response marked not modified without replayable pages",
                            )),
                            Ok(response) => {
                                self.replay = None;
                                Ok(response.body)
                            }
                            Err(error) => Err(error),
                        }
                    } else {
                        fetch_page(self.fetch.clone(), self.page_request(&cursor), self.retries)
                            .await
                    }
                }
            };
            outcome.map(PagedResponse::split)
        };
        if delivered.is_ok() {
            if let Some(error) = self.guard().await {
                return Some(Err(error));
            }
        }
        Some(self.deliver_page(delivered))
    }

    fn deliver_page(
        &mut self,
        delivered: Result<(Vec<Resp::Item>, PageInfo), ConnectError>,
    ) -> Result<Page<Resp::Item>, PageError> {
        match delivered {
            Ok((items, info)) => {
                self.measurement.page(!items.is_empty());
                self.pages += 1;
                self.items += items.len() as u64;
                if !info.next_cursor.is_empty() && !self.first_only {
                    if self.options.prefetch > 0 {
                        let ahead = self.page_request(&info.next_cursor);
                        self.pending = Some(Prefetch::spawn(fetch_page(
                            self.fetch.clone(),
                            ahead,
                            self.retries,
                        )));
                    }
                    self.cursor = Some(info.next_cursor.clone());
                } else {
                    self.measurement.finish(true, false);
                }
                let complete = info.next_cursor.is_empty();
                let page = page_of(items, info);
                if self.cache_enabled {
                    let bytes = page.items.iter().fold(0_usize, |sum, item| {
                        sum.saturating_add(Resp::encoded_len(item))
                    });
                    self.chain.push(&page, bytes);
                }
                if self.cursor.is_none() && complete && self.cache_enabled {
                    if let (Some(binding), Some(identity)) = (&self.cache, &self.identity) {
                        binding.cache.store(
                            identity.as_str(),
                            self.generation,
                            self.method,
                            binding.key,
                            std::mem::take(&mut self.chain),
                        );
                    }
                }
                Ok(page)
            }
            Err(error) => {
                if error.code == connectrpc::ErrorCode::Unauthenticated {
                    if let (Some(binding), Some(identity)) = (&self.cache, &self.identity) {
                        binding.cache.clear_identity(identity.as_str());
                    }
                }
                self.measurement.finish(false, false);
                Err(PageError::from_connect(error, self.pages, self.items))
            }
        }
    }

    fn page_request(&self, cursor: &str) -> Req {
        page_request(&self.request, cursor, self.options.page_size)
    }

    /// Resolves the caller's identity at a page boundary. The first boundary
    /// adopts it, with its replayable chain and generation; later ones refuse
    /// the walk when the identity or the cache generation moved, or when the
    /// identity can no longer be resolved.
    async fn guard(&mut self) -> Option<PageError> {
        let resolving = (self.cache.as_ref()?.identity)(());
        let Ok(current) = resolving.await else {
            return Some(self.refuse(true));
        };
        let binding = self.cache.as_ref()?;
        binding.cache.identity(current.as_str());
        let Some(started_as) = self.identity.clone() else {
            self.replay = if self.cache_enabled {
                binding
                    .cache
                    .replay(current.as_str(), self.method, binding.key)
            } else {
                None
            };
            self.generation = binding.cache.generation(current.as_str());
            self.identity = Some(current);
            return None;
        };
        let moved = started_as != current;
        if !moved && self.generation == binding.cache.generation(current.as_str()) {
            return None;
        }
        Some(self.refuse(moved))
    }

    /// Ends the walk as unauthenticated. A walk that outlives its caller drops
    /// the chains of the identity it started as, and of no other.
    fn refuse(&mut self, drop_started_as: bool) -> PageError {
        if drop_started_as && let (Some(binding), Some(started_as)) = (&self.cache, &self.identity)
        {
            binding.cache.clear_identity(started_as.as_str());
        }
        self.cursor = None;
        self.replaying = false;
        self.replay = None;
        self.pending = None;
        self.measurement.finish(false, false);
        PageError::from_connect(
            ConnectError::unauthenticated("list cache identity or session changed"),
            self.pages,
            self.items,
        )
    }
}

fn page_request<Req: PagedRequest>(base: &Req, cursor: &str, page_size: Option<u32>) -> Req {
    let mut request = base.clone();
    request.set_page(cursor, page_size.filter(|size| *size > 0));
    request
}

fn page_of<T>(items: Vec<T>, info: PageInfo) -> Page<T> {
    Page {
        from_cache: false,
        has_next: !info.next_cursor.is_empty(),
        items: items.into(),
        revision: info.revision,
        total: info.total,
    }
}

async fn fetch_page<Req: Clone, Resp>(
    fetch: Fetch<Req, Resp>,
    request: Req,
    retries: u8,
) -> Outcome<Resp> {
    fetch_with_retry(move || fetch(request.clone()), retries, None).await
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::*;
    use crate::PageErrorKind;

    /// An identity hook that always answers `who`.
    fn named(who: &'static str) -> IdentityFn {
        Arc::new(move |()| {
            Box::pin(async move { Ok(CacheIdentity::new(who).expect("a non-empty identity")) })
        })
    }

    #[derive(Clone)]
    struct Request;

    impl PagedRequest for Request {
        fn set_page(&mut self, cursor: &str, page_size: Option<u32>) {
            assert_eq!(cursor, "");
            assert_eq!(page_size, None);
        }
    }

    struct Response;

    impl PagedResponse for Response {
        type Item = u32;

        fn split(self) -> (Vec<Self::Item>, PageInfo) {
            (vec![1], PageInfo::default())
        }

        fn encoded_len(_: &Self::Item) -> usize {
            1
        }
    }

    #[test]
    fn the_prefetch_setter_reads_none_or_one_page_ahead() {
        let call = || {
            let fetch: Fetch<Request, Response> = Arc::new(|_| Box::pin(async { Ok(Response) }));
            ListCall::new(
                "/contract.v1.Numbers/List",
                Idempotency::NoSideEffects,
                fetch,
                Request,
            )
        };
        assert_eq!(call().options.prefetch, 1);
        assert_eq!(call().prefetch(0).options.prefetch, 0);
        assert_eq!(call().prefetch(1).options.prefetch, 1);
        assert_eq!(call().prefetch(9).options.prefetch, 1);
        assert_eq!(call().prefetch(0).prefetch(1).options.prefetch, 1);
    }

    #[tokio::test]
    async fn page_and_item_streams_stay_terminated_without_fetching_again() {
        for fails in [false, true] {
            for items in [false, true] {
                let attempts = Arc::new(AtomicUsize::new(0));
                let observed = attempts.clone();
                let fetch: Fetch<Request, Response> = Arc::new(move |_| {
                    observed.fetch_add(1, Ordering::SeqCst);
                    Box::pin(async move {
                        if fails {
                            Err(ConnectError::internal("terminal page failure"))
                        } else {
                            Ok(Response)
                        }
                    })
                });
                let call = ListCall::new(
                    "/contract.v1.Numbers/List",
                    Idempotency::NoSideEffects,
                    fetch,
                    Request,
                );
                let mut stream: api_bones_rt::BoxStream<'static, Result<Vec<u32>, PageError>> =
                    if items {
                        Box::pin(call.items().map(|result| result.map(|item| vec![item])))
                    } else {
                        Box::pin(
                            call.pages()
                                .map(|result| result.map(|page| page.items.to_vec())),
                        )
                    };
                let result = stream.next().await.expect("one page result");
                if fails {
                    assert!(result.is_err());
                } else {
                    assert_eq!(result.expect("successful page"), vec![1]);
                }
                for _ in 0..3 {
                    assert!(stream.next().await.is_none());
                    assert_eq!(attempts.load(Ordering::SeqCst), 1);
                }
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn metadata_fetch_retries_unavailable_only_for_retryable_methods() {
        for (idempotency, attempts_expected) in [
            (Idempotency::NoSideEffects, 3),
            (Idempotency::Idempotent, 3),
            (Idempotency::NonIdempotent, 1),
            (Idempotency::Unspecified, 1),
        ] {
            let attempts = Arc::new(AtomicUsize::new(0));
            let observed = attempts.clone();
            let fetch: MetadataFetch<Request, Response> = Arc::new(move |_, metadata| {
                assert!(metadata.cache_opt_in);
                assert!(metadata.if_none_match.is_none());
                let attempt = observed.fetch_add(1, Ordering::SeqCst);
                Box::pin(async move {
                    if attempt < 2 {
                        Err(ConnectError::unavailable("temporarily unavailable"))
                    } else {
                        Ok(ListResponse {
                            body: Response,
                            not_modified: false,
                        })
                    }
                })
            });
            let started = api_bones_rt::Instant::now();
            let page = ListCall::generated(
                "/contract.v1.Numbers/List",
                idempotency,
                fetch,
                Request,
                crate::RevisionCache::default(),
                named("principal"),
                &[],
            )
            .first_page()
            .await;
            assert_eq!(attempts.load(Ordering::SeqCst), attempts_expected);
            if attempts_expected == 3 {
                assert_eq!(&*page.expect("metadata fetch retried").items, &[1]);
                // Two backoffs: 50 ms and 100 ms, each plus jitter of up to the
                // same again.
                let elapsed = started.elapsed();
                assert!(
                    (Duration::from_millis(100)..Duration::from_millis(600)).contains(&elapsed),
                    "{elapsed:?} is outside the jittered backoff bounds"
                );
            } else {
                assert!(page.is_err());
                assert_eq!(started.elapsed(), Duration::ZERO);
            }
        }
    }

    #[tokio::test]
    async fn a_failed_answer_keeps_its_own_code_when_the_cache_was_cleared_meanwhile() {
        let cache = crate::RevisionCache::default();
        let cleared = cache.clone();
        let fetch: MetadataFetch<Request, Response> = Arc::new(move |_, _| {
            // The session ended while the page was in flight.
            cleared.clear();
            Box::pin(async { Err(ConnectError::permission_denied("not yours")) })
        });
        let failure = ListCall::generated(
            "/contract.v1.Numbers/List",
            Idempotency::NoSideEffects,
            fetch,
            Request,
            cache,
            named("principal"),
            &[],
        )
        .first_page()
        .await
        .expect_err("the page failed");
        match failure.kind {
            PageErrorKind::Rpc(error) => {
                assert_eq!(error.code, connectrpc::ErrorCode::PermissionDenied);
            }
            other => panic!("expected the page's own failure, got {other:?}"),
        }
    }

    #[derive(Clone)]
    struct Cursor(String);

    impl PagedRequest for Cursor {
        fn set_page(&mut self, cursor: &str, _page_size: Option<u32>) {
            self.0 = cursor.to_owned();
        }
    }

    /// Two pages: the first names the second, the second ends the list.
    struct TwoPages(String);

    impl PagedResponse for TwoPages {
        type Item = u32;

        fn split(self) -> (Vec<Self::Item>, PageInfo) {
            let (item, next_cursor) = if self.0.is_empty() {
                (1, "second")
            } else {
                (2, "")
            };
            (
                vec![item],
                PageInfo {
                    next_cursor: next_cursor.to_owned(),
                    revision: "r1".to_owned(),
                    total: None,
                },
            )
        }

        fn encoded_len(_: &Self::Item) -> usize {
            1
        }
    }

    /// `TwoPages` whose every item encodes to the whole per-chain budget.
    struct BigPages(TwoPages);

    impl PagedResponse for BigPages {
        type Item = u32;

        fn split(self) -> (Vec<Self::Item>, PageInfo) {
            self.0.split()
        }

        fn encoded_len(_: &Self::Item) -> usize {
            crate::cache::HOLD_CHAIN_BYTES
        }
    }

    fn walk_as(
        cache: &crate::RevisionCache,
        who: &'static str,
    ) -> impl Stream<Item = Result<crate::Page<u32>, PageError>> + MaybeSend + 'static {
        let fetch: MetadataFetch<Cursor, TwoPages> = Arc::new(|request, _| {
            Box::pin(async move {
                Ok(ListResponse {
                    body: TwoPages(request.0),
                    not_modified: false,
                })
            })
        });
        ListCall::generated(
            "/contract.v1.Numbers/List",
            Idempotency::NoSideEffects,
            fetch,
            Cursor(String::new()),
            cache.clone(),
            named(who),
            &[],
        )
        .pages()
    }

    #[tokio::test]
    async fn a_walk_over_the_chain_budget_finishes_without_caching() {
        let cache = crate::RevisionCache::default();
        let fetch: MetadataFetch<Cursor, BigPages> = Arc::new(|request, _| {
            Box::pin(async move {
                Ok(ListResponse {
                    body: BigPages(TwoPages(request.0)),
                    not_modified: false,
                })
            })
        });
        let mut walk = Box::pin(
            ListCall::generated(
                "/contract.v1.Numbers/List",
                Idempotency::NoSideEffects,
                fetch,
                Cursor(String::new()),
                cache.clone(),
                named("alice"),
                &[],
            )
            .pages(),
        );
        let mut delivered = 0;
        while let Some(page) = walk.next().await {
            page.expect("delivered");
            delivered += 1;
        }
        assert_eq!(delivered, 2);
        assert!(cache.is_empty());
    }

    #[tokio::test]
    async fn overlapping_walks_of_two_identities_both_finish_and_both_cache() {
        let cache = crate::RevisionCache::default();
        let mut anonymous = Box::pin(walk_as(&cache, "anonymous"));
        let mut signed_in = Box::pin(walk_as(&cache, "signed-in"));
        assert!(cache.generation("anonymous").is_none());
        let mut delivered = Vec::new();
        for walk in [&mut anonymous, &mut signed_in] {
            delivered.push(walk.next().await.expect("a page").expect("delivered"));
        }
        let anonymous_slot = cache.generation("anonymous");
        let signed_in_slot = cache.generation("signed-in");
        assert!(anonymous_slot.is_some() && signed_in_slot.is_some());
        assert_ne!(anonymous_slot, signed_in_slot);
        for walk in [&mut anonymous, &mut signed_in] {
            delivered.push(walk.next().await.expect("a page").expect("delivered"));
        }
        assert_eq!(delivered.len(), 4);
        assert!(anonymous.next().await.is_none());
        assert!(signed_in.next().await.is_none());
        assert_eq!(cache.generation("anonymous"), anonymous_slot);
        assert_eq!(cache.generation("signed-in"), signed_in_slot);
        for who in ["anonymous", "signed-in"] {
            let chain = cache
                .replay::<u32>(
                    who,
                    "/contract.v1.Numbers/List",
                    // The walk's key: the empty canonical request, then the page size.
                    {
                        let mut digest = Sha256::new();
                        digest.update(Sha256::digest(b""));
                        digest.update(0_u32.to_le_bytes());
                        digest.finalize().into()
                    },
                )
                .expect("the finished walk stored its chain");
            assert_eq!(chain.len(), 2);
        }
    }

    #[tokio::test]
    async fn a_walk_that_outlives_its_caller_drops_only_its_own_identity_slot() {
        let cache = crate::RevisionCache::default();
        let key = {
            let mut digest = Sha256::new();
            digest.update(Sha256::digest(b""));
            digest.update(0_u32.to_le_bytes());
            digest.finalize().into()
        };
        let mut held = Vec::new();
        for who in ["alice", "bob"] {
            let mut walk = Box::pin(walk_as(&cache, who));
            while let Some(page) = walk.next().await {
                held.push(page.expect("delivered"));
            }
        }
        // `alice` pauses mid-replay, then the caller becomes `carol`.
        let caller = Arc::new(std::sync::Mutex::new("alice".to_owned()));
        let source = Arc::clone(&caller);
        let fetch: MetadataFetch<Cursor, TwoPages> = Arc::new(|request, metadata| {
            Box::pin(async move {
                Ok(ListResponse {
                    body: TwoPages(request.0),
                    not_modified: metadata.if_none_match.is_some(),
                })
            })
        });
        let mut replay = Box::pin(
            ListCall::generated(
                "/contract.v1.Numbers/List",
                Idempotency::NoSideEffects,
                fetch,
                Cursor(String::new()),
                cache.clone(),
                Arc::new(move |()| {
                    let caller = source.lock().expect("caller").clone();
                    Box::pin(async move { Ok(CacheIdentity::new(caller).expect("a caller")) })
                }),
                &[],
            )
            .pages(),
        );
        assert!(
            replay
                .next()
                .await
                .expect("a page")
                .expect("delivered")
                .from_cache
        );
        *caller.lock().expect("caller") = "carol".to_owned();
        let refusal = replay
            .next()
            .await
            .expect("guard result")
            .expect_err("refused");
        assert!(matches!(
            refusal.kind,
            PageErrorKind::Rpc(error) if error.code == connectrpc::ErrorCode::Unauthenticated
        ));
        let path = "/contract.v1.Numbers/List";
        assert!(cache.replay::<u32>("alice", path, key).is_none());
        assert!(cache.replay::<u32>("bob", path, key).is_some());
        drop(held);
    }

    #[test]
    fn an_empty_identity_is_never_named() {
        assert!(CacheIdentity::new("").is_none());
        assert!(CacheIdentity::new(String::new()).is_none());
        assert_eq!(
            CacheIdentity::new("alice").expect("named").as_str(),
            "alice"
        );
    }

    /// A hook that answers `who` for its first `answers` calls, then fails.
    fn failing_after(answers: usize, who: &'static str) -> (Arc<AtomicUsize>, IdentityFn) {
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&calls);
        let hook: IdentityFn = Arc::new(move |()| {
            let call = counted.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                if call < answers {
                    Ok(CacheIdentity::new(who).expect("named"))
                } else {
                    Err(ConnectError::unavailable("the credential cannot be read"))
                }
            })
        });
        (calls, hook)
    }

    fn two_page_call(
        cache: &crate::RevisionCache,
        fetches: &Arc<AtomicUsize>,
        identity: IdentityFn,
    ) -> ListCall<Cursor, TwoPages> {
        let counted = Arc::clone(fetches);
        let fetch: MetadataFetch<Cursor, TwoPages> = Arc::new(move |request, _| {
            counted.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Ok(ListResponse {
                    body: TwoPages(request.0),
                    not_modified: false,
                })
            })
        });
        ListCall::generated(
            "/contract.v1.Numbers/List",
            Idempotency::NoSideEffects,
            fetch,
            Cursor(String::new()),
            cache.clone(),
            identity,
            &[],
        )
    }

    #[tokio::test]
    async fn the_identity_is_resolved_at_the_first_poll_never_at_construction() {
        let cache = crate::RevisionCache::default();
        let fetches = Arc::new(AtomicUsize::new(0));
        let (calls, hook) = failing_after(usize::MAX, "alice");
        let mut walk = Box::pin(two_page_call(&cache, &fetches, hook).pages());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(cache.generation("alice").is_none());
        walk.next().await.expect("a page").expect("delivered");
        assert!(calls.load(Ordering::SeqCst) >= 1);
        assert!(cache.generation("alice").is_some());
    }

    #[tokio::test]
    async fn an_identity_that_cannot_be_resolved_fails_the_walk_before_any_fetch() {
        let cache = crate::RevisionCache::default();
        let fetches = Arc::new(AtomicUsize::new(0));
        let (_, hook) = failing_after(0, "alice");
        let mut walk = Box::pin(two_page_call(&cache, &fetches, hook).pages());
        let refusal = walk.next().await.expect("a refusal").expect_err("refused");
        assert!(matches!(
            refusal.kind,
            PageErrorKind::Rpc(error) if error.code == connectrpc::ErrorCode::Unauthenticated
        ));
        assert!(walk.next().await.is_none());
        assert_eq!(fetches.load(Ordering::SeqCst), 0);
        assert!(cache.is_empty());
    }

    #[tokio::test]
    async fn an_identity_lost_mid_walk_drops_the_chains_it_started_as() {
        let cache = crate::RevisionCache::default();
        let key = {
            let mut digest = Sha256::new();
            digest.update(Sha256::digest(b""));
            digest.update(0_u32.to_le_bytes());
            digest.finalize().into()
        };
        let finished = Arc::new(AtomicUsize::new(0));
        let mut done = Box::pin(two_page_call(&cache, &finished, named("alice")).pages());
        while let Some(page) = done.next().await {
            page.expect("delivered");
        }
        assert!(
            cache
                .replay::<u32>("alice", "/contract.v1.Numbers/List", key)
                .is_some()
        );
        let fetches = Arc::new(AtomicUsize::new(0));
        // A page boundary resolves the identity before the fetch and once the
        // page is in hand; the third answer carries the first page out and the
        // next one fails.
        let (_, hook) = failing_after(3, "alice");
        let mut walk = Box::pin(two_page_call(&cache, &fetches, hook).pages());
        walk.next().await.expect("a page").expect("delivered");
        let refusal = walk.next().await.expect("a refusal").expect_err("refused");
        assert_eq!(refusal.delivered_pages, 1);
        assert!(
            cache
                .replay::<u32>("alice", "/contract.v1.Numbers/List", key)
                .is_none()
        );
        assert!(walk.next().await.is_none());
    }
}
