// SPDX-License-Identifier: MIT
//! [`ListCall`]: one List RPC as a stream of pages or of items.

use api_bones::connect::{Idempotency, fetch_with_retry, retries_for};
use connectrpc::ConnectError;
use futures_core::Stream;
use futures_util::StreamExt as _;
use sha2::{Digest, Sha256};
use std::pin::Pin;
use std::sync::Arc;

use crate::error::PageError;
use crate::prefetch::Prefetch;

/// The transport call for one page: a request in, a response out.
pub type Fetch<Req, Resp> = Arc<
    dyn Fn(Req) -> Pin<Box<dyn Future<Output = Result<Resp, ConnectError>> + Send>> + Send + Sync,
>;

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
pub type MetadataFetch<Req, Resp> = Arc<
    dyn Fn(
            Req,
            ListMetadata,
        )
            -> Pin<Box<dyn Future<Output = Result<ListResponse<Resp>, ConnectError>> + Send>>
        + Send
        + Sync,
>;

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
    identity: Arc<dyn Fn() -> String + Send + Sync>,
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
pub trait PagedRequest: Clone + Send + 'static {
    /// Set the cursor (empty = first page) and the page-size hint.
    fn set_page(&mut self, cursor: &str, page_size: Option<u32>);
}

/// A response that carries one repeated field and a `bones.v1.PageResponse`.
pub trait PagedResponse: Send + 'static {
    /// One element of the repeated field.
    type Item: Send + Sync + 'static;

    /// Take the items and the page info out of the response.
    fn split(self) -> (Vec<Self::Item>, PageInfo);
}

/// Per-iteration options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageOptions {
    /// Revision cache policy.
    pub cache: CacheMode,
    /// Page-size hint; the server clamps it. `None` or zero = server default.
    pub page_size: Option<u32>,
    /// Pages fetched ahead of the consumer: 0 or 1.
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
        identity: Arc<dyn Fn() -> String + Send + Sync>,
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
    pub fn pages(self) -> impl Stream<Item = Result<Page<Resp::Item>, PageError>> + Send + 'static {
        self.raw_pages()
    }

    /// Every item, following `next_cursor` to the end.
    pub fn items(self) -> impl Stream<Item = Result<Resp::Item, PageError>> + Send + 'static
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

    fn raw_pages(self) -> impl Stream<Item = Result<Page<Resp::Item>, PageError>> + Send + 'static {
        let cache_enabled = self.first.is_none() && self.options.cache == CacheMode::Use;
        let mut cache = self.cache;
        if let Some(binding) = &mut cache {
            let mut digest = Sha256::new();
            digest.update(binding.key);
            digest.update(self.options.page_size.unwrap_or_default().to_le_bytes());
            binding.key = digest.finalize().into();
        }
        let identity = cache.as_ref().map(|binding| (binding.identity)());
        if let (Some(binding), Some(identity)) = (&cache, &identity) {
            binding.cache.identity(identity);
        }
        let replay = cache
            .as_ref()
            .filter(|_| cache_enabled)
            .and_then(|binding| binding.cache.replay(self.method, binding.key));
        let generation = cache.as_ref().map(|binding| binding.cache.generation());
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
            identity,
            replay,
            replaying: false,
            chain: Vec::new(),
            generation,
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
    identity: Option<String>,
    replay: Option<Vec<Page<Resp::Item>>>,
    replaying: bool,
    chain: Vec<crate::cache::Cached<Resp::Item>>,
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
        if let Some(error) = self.guard() {
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
                            tokio::time::sleep,
                        )
                        .await;
                        if let Some(error) = self.guard() {
                            return Some(Err(error));
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
        if let Some(error) = self.guard() {
            return Some(Err(error));
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
                self.chain.push(crate::cache::Cached::of(&page));
                if self.cursor.is_none() && complete && self.cache_enabled {
                    if let (Some(binding), Some(identity)) = (&self.cache, &self.identity) {
                        binding.cache.store(
                            identity,
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
                    if let Some(binding) = &self.cache {
                        binding.cache.clear();
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

    fn guard(&mut self) -> Option<PageError> {
        let binding = self.cache.as_ref()?;
        let identity = (binding.identity)();
        binding.cache.identity(&identity);
        if self.identity.as_deref() == Some(identity.as_str())
            && self.generation == Some(binding.cache.generation())
        {
            return None;
        }
        self.cursor = None;
        self.replaying = false;
        self.replay = None;
        self.pending = None;
        self.measurement.finish(false, false);
        Some(PageError::from_connect(
            ConnectError::unauthenticated("list cache identity or session changed"),
            self.pages,
            self.items,
        ))
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
    fetch_with_retry(move || fetch(request.clone()), retries, tokio::time::sleep).await
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::*;

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
                let mut stream: Pin<Box<dyn Stream<Item = Result<Vec<u32>, PageError>> + Send>> =
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
            let started = tokio::time::Instant::now();
            let page = ListCall::generated(
                "/contract.v1.Numbers/List",
                idempotency,
                fetch,
                Request,
                crate::RevisionCache::default(),
                Arc::new(|| "principal".to_owned()),
                &[],
            )
            .first_page()
            .await;
            assert_eq!(attempts.load(Ordering::SeqCst), attempts_expected);
            if attempts_expected == 3 {
                assert_eq!(&*page.expect("metadata fetch retried").items, &[1]);
                assert_eq!(started.elapsed(), Duration::from_millis(150));
            } else {
                assert!(page.is_err());
                assert_eq!(started.elapsed(), Duration::ZERO);
            }
        }
    }
}
