// SPDX-License-Identifier: MIT
//! Credential-bound unary paging on the platform client.

use std::sync::Arc;

use api_bones::connect::Idempotency;
use api_bones_page::{
    IdentityFn, ListCall, ListResponse, MetadataFetch, PagedRequest, PagedResponse,
};
use api_bones_rt::BoxFuture;
use connectrpc::ConnectError;
use connectrpc::client::CallOptions;
use futures_util::future::{FutureExt as _, Shared};
use http::HeaderMap;
use http::header::{HeaderName, IF_NONE_MATCH};

use crate::key::{CallHeaders, CredentialGuard};
use crate::{Batched, Boxed, header_hash};

/// Server answer header: the cached revision chain is still current.
pub const BONES_NOT_MODIFIED: HeaderName = HeaderName::from_static("bones-not-modified");
/// Client request header: the caller holds a list revision cache.
pub const BONES_LIST_CACHE: HeaderName = HeaderName::from_static("bones-list-cache");

/// The generated unary List transport, preserving response metadata.
#[cfg(not(target_arch = "wasm32"))]
pub type ListFn<Req, Resp> =
    Arc<dyn Fn(Req, CallOptions) -> Boxed<Result<ListResponse<Resp>, ConnectError>> + Send + Sync>;
/// The generated unary List transport, preserving response metadata.
#[cfg(target_arch = "wasm32")]
pub type ListFn<Req, Resp> =
    Arc<dyn Fn(Req, CallOptions) -> Boxed<Result<ListResponse<Resp>, ConnectError>>>;

/// Whether the server confirmed the complete cached revision chain.
#[must_use]
pub fn not_modified(headers: &HeaderMap) -> bool {
    headers
        .get(&BONES_NOT_MODIFIED)
        .is_some_and(|value| value == "1")
}

/// The credential a list walk is bound to: read once, at the walk's first
/// poll, and shared by its identity hook and every page fetch.
type Capture = Shared<BoxFuture<'static, Result<CredentialGuard, ConnectError>>>;

impl<C> Batched<C> {
    /// Generated List adapter sharing the client's credential source and revision cache.
    #[doc(hidden)]
    #[must_use]
    pub fn __list<Req, Resp>(
        &self,
        method: &'static str,
        idempotency: Idempotency,
        request: Req,
        headers: &CallHeaders,
        canonical: &[u8],
        single: ListFn<Req, Resp>,
    ) -> ListCall<Req, Resp>
    where
        Req: PagedRequest,
        Resp: PagedResponse,
    {
        let wire = Arc::clone(&self.wire);
        let capture: Capture = {
            let wire = Arc::clone(&wire);
            let reading: BoxFuture<'static, Result<CredentialGuard, ConnectError>> =
                Box::pin(async move { CredentialGuard::capture(&wire.read().await?) });
            reading.shared()
        };
        let headers = headers.continuation();
        let mut canonical = canonical.to_vec();
        canonical.extend_from_slice(&header_hash(&headers.effective()));
        let identity_capture = capture.clone();
        let identity_wire = Arc::clone(&wire);
        let identity_cache = self.revisions.clone();
        let identity_source: IdentityFn = Arc::new(move |()| {
            let capture = identity_capture.clone();
            let wire = Arc::clone(&identity_wire);
            let cache = identity_cache.clone();
            Box::pin(async move {
                let named = async {
                    let guard = capture.await?;
                    wire.verify(&guard).await?;
                    guard
                        .cache_identity()
                        .ok_or_else(|| ConnectError::internal("no identity"))
                }
                .await;
                if named.is_err() {
                    cache.clear();
                }
                named
            })
        });
        let cache = self.revisions.clone();
        let fetch: MetadataFetch<Req, Resp> = Arc::new(move |request, metadata| {
            let wire = Arc::clone(&wire);
            let capture = capture.clone();
            let single = Arc::clone(&single);
            let mut headers = headers.clone();
            let cache = cache.clone();
            Box::pin(async move {
                let result = async {
                    let guard = capture.await?;
                    let credential = wire.verify(&guard).await?;
                    headers.validate(&credential.headers)?;
                    if metadata.cache_opt_in {
                        headers = headers.header(BONES_LIST_CACHE.as_str(), b"1".to_vec());
                    }
                    if let Some(revision) = metadata.if_none_match {
                        headers = headers.header(IF_NONE_MATCH.as_str(), revision.into_bytes());
                    }
                    let response =
                        single(request, headers.call_options(&credential.headers)?).await;
                    wire.verify(&guard).await?;
                    response
                }
                .await;
                if result
                    .as_ref()
                    .is_err_and(|error| error.code == connectrpc::ErrorCode::Unauthenticated)
                {
                    cache.clear();
                }
                result
            })
        });
        ListCall::generated(
            method,
            idempotency,
            fetch,
            request,
            self.revisions.clone(),
            identity_source,
            &canonical,
        )
    }
}

/// Implements the page cursor adapter for a generated request.
#[macro_export]
macro_rules! paged_request {
    ($request:ty, $page:ident $(,)?) => {
        impl $crate::__private::PagedRequest for $request {
            fn set_page(&mut self, cursor: &str, page_size: Option<u32>) {
                let mut page = self.$page.take().unwrap_or_default();
                cursor.clone_into(&mut page.cursor);
                page.page_size = page_size.unwrap_or_default();
                self.$page = page.into();
            }
        }
        impl $crate::__private::CanonicalPagedRequest for $request {
            fn encode(&self) -> Vec<u8> {
                $crate::__private::buffa::Message::encode_to_vec(self)
            }
        }
    };
}

/// Encoded protobuf lengths of one repeated-field element, by field kind.
pub mod item_len {
    use buffa::types;

    /// A message element.
    #[must_use]
    pub fn message<M: buffa::Message>(item: &M) -> usize {
        item.encoded_len() as usize
    }

    /// A `string` element.
    #[must_use]
    pub fn string(item: &str) -> usize {
        types::string_encoded_len(item)
    }

    /// A `bytes` element.
    #[must_use]
    pub fn bytes(item: &[u8]) -> usize {
        types::bytes_encoded_len(item)
    }

    /// An `int32` element.
    #[must_use]
    pub fn int32(item: &i32) -> usize {
        types::int32_encoded_len(*item)
    }

    /// An `int64` element.
    #[must_use]
    pub fn int64(item: &i64) -> usize {
        types::int64_encoded_len(*item)
    }

    /// A `uint32` element.
    #[must_use]
    pub fn uint32(item: &u32) -> usize {
        types::uint32_encoded_len(*item)
    }

    /// A `uint64` element.
    #[must_use]
    pub fn uint64(item: &u64) -> usize {
        types::uint64_encoded_len(*item)
    }

    /// A `sint32` element.
    #[must_use]
    pub fn sint32(item: &i32) -> usize {
        types::sint32_encoded_len(*item)
    }

    /// A `sint64` element.
    #[must_use]
    pub fn sint64(item: &i64) -> usize {
        types::sint64_encoded_len(*item)
    }

    /// A `bool` element.
    #[must_use]
    pub fn boolean(_: &bool) -> usize {
        1
    }

    /// A `fixed32`, `sfixed32` or `float` element.
    #[must_use]
    pub fn fixed32<T>(_: &T) -> usize {
        4
    }

    /// A `fixed64`, `sfixed64` or `double` element.
    #[must_use]
    pub fn fixed64<T>(_: &T) -> usize {
        8
    }
}

/// Implements the page response adapter for a generated response. `size`
/// names the `item_len` function of the repeated field's kind.
#[macro_export]
macro_rules! paged_response {
    ($response:ty, item: $item:ty, size: $size:path, items: $items:ident, page: $page:ident $(,)?) => {
        impl $crate::__private::PagedResponse for $response {
            type Item = $item;
            fn encoded_len(item: &Self::Item) -> usize {
                $size(item)
            }
            fn split(self) -> (Vec<Self::Item>, $crate::__private::PageInfo) {
                let page = self.$page.into_option().unwrap_or_default();
                let info = $crate::__private::PageInfo {
                    next_cursor: page.next_cursor,
                    revision: page.revision,
                    total: page
                        .total
                        .into_option()
                        .map(|total| $crate::__private::Total {
                            value: total.value,
                            kind: if total.kind.to_i32() == 2 {
                                $crate::__private::TotalKind::Exact
                            } else {
                                $crate::__private::TotalKind::Estimate
                            },
                        }),
                };
                (self.$items, info)
            }
        }
    };
}

/// Declares generated List methods on the existing platform client.
#[macro_export]
macro_rules! paged_method {
    (ext: $ext:ident, client: $client:ty, lists: [$({
        method: $method:literal,
        name: $name:ident,
        with_options: $options:ident,
        idempotency: $idempotency:expr,
        request: $request:ty,
        response: $response:ty $(,)?
    }),* $(,)?] $(,)?) => {
        /// Unary page iterators on the platform client.
        pub trait $ext {
            $(
                /// Iterate the list using the client's credential source and cache.
                fn $name(&self, request: $request) -> $crate::__private::ListCall<$request, $response>;
                /// Iterate the list with caller headers and a deadline.
                fn $options(&self, request: $request, headers: $crate::CallHeaders) -> $crate::__private::ListCall<$request, $response>;
            )*
        }
        impl $ext for $crate::Batched<$client> {
            $(
                fn $name(&self, request: $request) -> $crate::__private::ListCall<$request, $response> {
                    self.$options(request, $crate::CallHeaders::new())
                }
                fn $options(&self, request: $request, headers: $crate::CallHeaders) -> $crate::__private::ListCall<$request, $response> {
                    let canonical = {
                        let mut bare = ::std::clone::Clone::clone(&request);
                        $crate::__private::PagedRequest::set_page(&mut bare, "", None);
                        $crate::__private::CanonicalPagedRequest::encode(&bare)
                    };
                    let client = self.__inner();
                    self.__list($method, $idempotency, request, &headers, &canonical, ::std::sync::Arc::new(move |request, options| {
                        let client = ::std::sync::Arc::clone(&client);
                        ::std::boxed::Box::pin(async move {
                            let response = client.$options(request, options).await?;
                            let not_modified = $crate::not_modified(response.headers());
                            Ok($crate::__private::ListResponse { body: response.into_owned(), not_modified })
                        })
                    }))
                }
            )*
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::cache_identity;
    use crate::{Credential, CredentialAsk, CredentialSource, Identity, Wire};
    use api_bones_page::{PageErrorKind, PageInfo, PageOptions};
    use futures_util::StreamExt as _;
    use std::collections::HashMap;
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    #[derive(Clone, Default)]
    struct Req(String);

    impl PagedRequest for Req {
        fn set_page(&mut self, cursor: &str, _page_size: Option<u32>) {
            cursor.clone_into(&mut self.0);
        }
    }

    struct Resp {
        next: String,
    }

    impl PagedResponse for Resp {
        type Item = u32;
        fn encoded_len(_: &u32) -> usize {
            1
        }

        fn split(self) -> (Vec<u32>, PageInfo) {
            (
                vec![7],
                PageInfo {
                    next_cursor: self.next,
                    revision: "r1".into(),
                    total: None,
                },
            )
        }
    }

    fn credential(subject: &str) -> Credential {
        Credential {
            identity: Identity {
                subject: subject.into(),
                org: "org".into(),
            },
            headers: vec![("authorization".into(), b"Bearer token".to_vec())],
        }
    }

    fn client(source: impl CredentialSource) -> Batched<()> {
        Batched {
            inner: Arc::new(()),
            wire: Arc::new(Wire::new(Arc::new(source), None)),
            coalescers: Arc::new(Mutex::new(HashMap::new())),
            methods: Arc::new(crate::descriptor::Methods::new()),
            revisions: api_bones_page::RevisionCache::default(),
        }
    }

    fn call(
        client: &Batched<()>,
        headers: &CallHeaders,
        send: ListFn<Req, Resp>,
    ) -> ListCall<Req, Resp> {
        client
            .__list(
                "p.S/List",
                Idempotency::NoSideEffects,
                Req::default(),
                headers,
                b"query",
                send,
            )
            .options(PageOptions {
                prefetch: 0,
                ..PageOptions::default()
            })
    }

    fn rpc_error(error: &api_bones_page::PageError) -> &ConnectError {
        match &error.kind {
            PageErrorKind::Rpc(error) => error,
            kind => panic!("expected RPC failure, got {kind:?}"),
        }
    }

    fn success() -> ListFn<Req, Resp> {
        Arc::new(|_, _| {
            Box::pin(async {
                Ok(ListResponse {
                    body: Resp {
                        next: String::new(),
                    },
                    not_modified: false,
                })
            })
        })
    }

    fn mutable_identity_client() -> (Batched<()>, Arc<Mutex<Credential>>) {
        let current = Arc::new(Mutex::new(credential("alice")));
        let source = Arc::clone(&current);
        let client = client(move |_: CredentialAsk| {
            let held = source.lock().expect("credential").clone();
            async move { Ok(held) }
        });
        (client, current)
    }

    fn counted_success(calls: &Arc<AtomicUsize>) -> ListFn<Req, Resp> {
        let calls = Arc::clone(calls);
        let send = success();
        Arc::new(move |request, options| {
            calls.fetch_add(1, Ordering::SeqCst);
            send(request, options)
        })
    }

    #[tokio::test]
    async fn cache_headers_replay_across_clones_and_observed_starts_fresh() {
        let client = client(|_: CredentialAsk| async { Ok(credential("alice")) });
        let seen = Arc::new(Mutex::new(Vec::new()));
        let send: ListFn<Req, Resp> = {
            let seen = Arc::clone(&seen);
            Arc::new(move |_, options| {
                let headers = options.headers().clone();
                seen.lock().expect("seen").push(headers.clone());
                Box::pin(async move {
                    Ok(ListResponse {
                        body: Resp {
                            next: String::new(),
                        },
                        not_modified: headers.contains_key(IF_NONE_MATCH),
                    })
                })
            })
        };
        let headers = CallHeaders::new()
            .header(IF_NONE_MATCH.as_str(), b"forged".to_vec())
            .header(BONES_LIST_CACHE.as_str(), b"0".to_vec());
        let held = call(&client, &headers, Arc::clone(&send))
            .pages()
            .collect::<Vec<_>>()
            .await;
        assert!(!client.revisions.is_empty());
        let replay = call(&client.clone(), &headers, Arc::clone(&send))
            .first_page()
            .await
            .expect("replay");
        assert!(replay.from_cache);
        assert!(Arc::ptr_eq(
            &replay.items,
            &held[0].as_ref().expect("held").items
        ));
        let other = call(
            &client,
            &headers.clone().header("x-filter", b"other".to_vec()),
            Arc::clone(&send),
        )
        .first_page()
        .await
        .expect("partition");
        assert!(!other.from_cache);
        let observed = client.observed(Arc::new(|_| {}));
        assert!(observed.revisions.is_empty());
        assert!(
            !call(&observed, &headers, send)
                .first_page()
                .await
                .expect("fresh")
                .from_cache
        );
        let seen = seen.lock().expect("seen");
        assert!(seen.iter().all(|headers| headers[&BONES_LIST_CACHE] == "1"));
        assert!(!seen[0].contains_key(IF_NONE_MATCH));
        assert_eq!(seen[1][IF_NONE_MATCH], "\"r1\"");
        assert!(!seen[2].contains_key(IF_NONE_MATCH));
        assert!(!seen[3].contains_key(IF_NONE_MATCH));
    }

    #[tokio::test]
    async fn identity_change_ends_chain_and_clears_cache() {
        let (client, current) = mutable_identity_client();
        let held = call(&client, &CallHeaders::new(), success())
            .first_page()
            .await
            .expect("seed");
        assert!(!client.revisions.is_empty());
        let send: ListFn<Req, Resp> = Arc::new(|_, _| {
            Box::pin(async {
                Ok(ListResponse {
                    body: Resp {
                        next: "next".into(),
                    },
                    not_modified: false,
                })
            })
        });
        let pages = call(&client, &CallHeaders::new(), send).pages();
        let mut pages = Box::pin(pages);
        assert!(pages.next().await.expect("first").is_ok());
        *current.lock().expect("credential") = credential("bob");
        let error = pages
            .next()
            .await
            .expect("error")
            .expect_err("identity changed");
        assert_eq!(
            rpc_error(&error).code,
            connectrpc::ErrorCode::Unauthenticated
        );
        assert!(client.revisions.is_empty());
        assert!(pages.next().await.is_none());
        assert_eq!(&*held.items, &[7]);
    }

    #[tokio::test]
    async fn per_call_credentials_are_refused_without_send() {
        let client = client(|_: CredentialAsk| async { Ok(credential("alice")) });
        let calls = Arc::new(AtomicUsize::new(0));
        let send = counted_success(&calls);
        let error = call(
            &client,
            &CallHeaders::new().header("authorization", b"Bearer other".to_vec()),
            send,
        )
        .first_page()
        .await
        .expect_err("per-call credential");
        assert_eq!(
            rpc_error(&error).code,
            connectrpc::ErrorCode::InvalidArgument
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_response_under_another_identity_is_discarded_and_clears_the_cache() {
        let (client, current) = mutable_identity_client();
        let held = call(&client, &CallHeaders::new(), success())
            .first_page()
            .await
            .expect("seed");
        let calls = Arc::new(AtomicUsize::new(0));
        let send: ListFn<Req, Resp> = {
            let current = Arc::clone(&current);
            let send = counted_success(&calls);
            Arc::new(move |request, options| {
                *current.lock().expect("credential") = credential("bob");
                send(request, options)
            })
        };
        let error = call(&client, &CallHeaders::new(), send)
            .first_page()
            .await
            .expect_err("changed identity");
        assert_eq!(
            rpc_error(&error).code,
            connectrpc::ErrorCode::Unauthenticated
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(client.revisions.is_empty());
        assert_eq!(&*held.items, &[7]);
    }

    #[tokio::test]
    async fn the_credential_is_read_at_the_first_poll_not_when_the_list_is_made() {
        let (client, current) = mutable_identity_client();
        let held = call(&client, &CallHeaders::new(), success())
            .first_page()
            .await
            .expect("seed as alice");
        let calls = Arc::new(AtomicUsize::new(0));
        let pending = call(&client, &CallHeaders::new(), counted_success(&calls));
        *current.lock().expect("credential") = credential("bob");
        let page = pending.first_page().await.expect("walked as bob");
        assert!(!page.from_cache);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(&*held.items, &[7]);
    }

    #[tokio::test]
    async fn identity_change_between_iteration_guard_and_send_refuses_dispatch() {
        let reads = Arc::new(AtomicUsize::new(0));
        let client = {
            let reads = Arc::clone(&reads);
            client(move |_: CredentialAsk| {
                // Reads: the walk's capture, the identity check, the fetch's check.
                let read = reads.fetch_add(1, Ordering::SeqCst);
                let who = credential(if read < 2 { "alice" } else { "bob" });
                async move { Ok(who) }
            })
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let send = counted_success(&calls);
        let error = call(&client, &CallHeaders::new(), send)
            .first_page()
            .await
            .expect_err("changed before send");
        assert_eq!(
            rpc_error(&error).code,
            connectrpc::ErrorCode::Unauthenticated
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(client.revisions.is_empty());
    }

    #[tokio::test]
    async fn credential_loss_or_source_failure_ends_chain() {
        for failure in [true, false] {
            let reads = Arc::new(AtomicUsize::new(0));
            let lost_from = Arc::new(AtomicUsize::new(usize::MAX));
            let client = {
                let reads = Arc::clone(&reads);
                let lost_from = Arc::clone(&lost_from);
                client(move |_: CredentialAsk| {
                    let read = reads.fetch_add(1, Ordering::SeqCst);
                    let answer = if read < lost_from.load(Ordering::SeqCst) {
                        Ok(credential("alice"))
                    } else if failure {
                        Err(ConnectError::unavailable("source unavailable"))
                    } else {
                        Ok(Credential::default())
                    };
                    async move { answer }
                })
            };
            let held = call(&client, &CallHeaders::new(), success())
                .first_page()
                .await
                .expect("seed");
            // The next walk captures alice and checks her once; its fetch finds
            // the credential gone.
            lost_from.store(reads.load(Ordering::SeqCst) + 2, Ordering::SeqCst);
            let error = call(&client, &CallHeaders::new(), success())
                .first_page()
                .await
                .expect_err("lost credential");
            assert_eq!(
                rpc_error(&error).code,
                connectrpc::ErrorCode::Unauthenticated
            );
            assert!(client.revisions.is_empty());
            assert_eq!(&*held.items, &[7]);
        }
    }

    #[tokio::test]
    async fn a_source_that_cannot_answer_at_the_first_poll_fails_the_walk_closed() {
        let client = client(|_: CredentialAsk| async {
            Err::<Credential, _>(ConnectError::unavailable("source unavailable"))
        });
        let calls = Arc::new(AtomicUsize::new(0));
        let error = call(&client, &CallHeaders::new(), counted_success(&calls))
            .first_page()
            .await
            .expect_err("no credential");
        assert_eq!(
            rpc_error(&error).code,
            connectrpc::ErrorCode::Unauthenticated
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn marker_and_identity_are_exact_and_unambiguous() {
        let mut headers = HeaderMap::new();
        assert!(!not_modified(&headers));
        headers.insert(BONES_NOT_MODIFIED, "true".parse().expect("header"));
        assert!(!not_modified(&headers));
        headers.insert(BONES_NOT_MODIFIED, "1".parse().expect("header"));
        assert!(not_modified(&headers));
        let name = |credential: &Credential| cache_identity(credential).expect("a name");
        assert_eq!(name(&Credential::default()).as_str(), "0:0:0:");
        assert_eq!(
            name(&credential("alice")).as_str(),
            "5:alice3:org1:13:authorization"
        );
        let mut first = credential("ab");
        first.identity.org = "c".into();
        let mut second = credential("a");
        second.identity.org = "bc".into();
        assert_ne!(name(&first), name(&second));
    }

    #[test]
    fn credentials_that_differ_in_subject_org_or_header_names_get_their_own_name() {
        let name = |credential: &Credential| cache_identity(credential).expect("a name");
        let by = |subject: &str, header: &str| Credential {
            identity: Identity {
                subject: subject.into(),
                org: "org".into(),
            },
            headers: vec![(header.into(), b"value".to_vec())],
        };
        let cookie_user = by("alice", "cookie");
        let other_header_user = by("bob", "x-session");
        assert_ne!(name(&cookie_user), name(&other_header_user));
        assert_ne!(name(&cookie_user), name(&by("alice", "x-session")));
        assert_ne!(name(&cookie_user), name(&by("bob", "cookie")));
        assert_ne!(name(&cookie_user), name(&Credential::default()));
        // A subject without any header is not the anonymous caller.
        let headerless = Credential {
            identity: cookie_user.identity.clone(),
            headers: Vec::new(),
        };
        assert_ne!(name(&headerless), name(&Credential::default()));
        // Header names, not values or their order or case, make the name.
        let mut renewed = by("alice", "Cookie");
        renewed.headers[0].1 = b"renewed".to_vec();
        assert_eq!(name(&cookie_user), name(&renewed));
        assert!(!name(&cookie_user).as_str().contains("value"));
        let mut two = by("alice", "cookie");
        two.headers.push(("authorization".into(), b"v".to_vec()));
        let mut swapped = two.clone();
        swapped.headers.reverse();
        assert_eq!(name(&two), name(&swapped));
        assert_ne!(name(&two), name(&cookie_user));
    }

    #[test]
    fn only_a_credential_with_no_subject_org_or_header_is_anonymous() {
        let name = |credential: &Credential| cache_identity(credential).expect("a name");
        assert_eq!(name(&Credential::default()).as_str(), "0:0:0:");
        let org_only = Credential {
            identity: Identity {
                subject: String::new(),
                org: "acme".into(),
            },
            headers: Vec::new(),
        };
        assert_ne!(name(&org_only).as_str(), "0:0:0:");
    }

    /// Shapes the generated page response has, without the generator.
    mod listing {
        use api_bones_page::{PagedResponse, TotalKind};
        use buffa::MessageField;

        #[derive(Default)]
        pub struct Kind(pub i32);

        impl Kind {
            pub fn to_i32(&self) -> i32 {
                self.0
            }
        }

        #[derive(Default)]
        pub struct Total {
            pub value: u64,
            pub kind: Kind,
        }

        #[derive(Default)]
        pub struct PageResponse {
            pub next_cursor: String,
            pub revision: String,
            pub total: MessageField<Total>,
        }

        #[derive(Default)]
        pub struct Listing {
            pub page: MessageField<PageResponse>,
            pub names: Vec<String>,
        }

        crate::paged_response!(
            Listing,
            item: String,
            size: crate::__private::item_len::string,
            items: names,
            page: page
        );

        #[test]
        fn the_macro_splits_items_and_page_info() {
            let listing = Listing {
                names: vec!["a".into(), "bc".into()],
                page: PageResponse {
                    next_cursor: "n".into(),
                    revision: "r".into(),
                    total: Total {
                        value: 9,
                        kind: Kind(2),
                    }
                    .into(),
                }
                .into(),
            };
            let (items, info) = listing.split();
            assert_eq!(items, ["a", "bc"]);
            assert_eq!(info.next_cursor, "n");
            assert_eq!(info.revision, "r");
            let total = info.total.expect("a total");
            assert_eq!((total.value, total.kind), (9, TotalKind::Exact));
        }

        #[test]
        fn an_unspecified_total_kind_is_an_estimate_and_a_missing_page_is_empty() {
            let page = PageResponse {
                total: Total {
                    value: 1,
                    kind: Kind(1),
                }
                .into(),
                ..PageResponse::default()
            };
            let (_, info) = Listing {
                page: page.into(),
                ..Listing::default()
            }
            .split();
            assert_eq!(info.total.expect("a total").kind, TotalKind::Estimate);
            let (items, info) = Listing::default().split();
            assert_eq!(items.len(), 0);
            assert_eq!(info, api_bones_page::PageInfo::default());
        }

        #[test]
        fn the_macro_measures_an_item_with_the_named_length() {
            assert_eq!(Listing::encoded_len(&"abc".to_owned()), 4);
        }
    }

    #[test]
    fn item_lengths_are_the_protobuf_encodings() {
        use item_len::*;
        assert_eq!(string("abc"), 4);
        assert_eq!(bytes(&[1, 2]), 3);
        assert_eq!(int32(&300), 2);
        assert_eq!(int32(&-1), 10);
        assert_eq!(int64(&1), 1);
        assert_eq!(uint32(&128), 2);
        assert_eq!(uint64(&1), 1);
        assert_eq!(sint32(&-64), 1);
        assert_eq!(sint64(&-65), 2);
        assert_eq!(boolean(&true), 1);
        assert_eq!(fixed32(&1.5_f32), 4);
        assert_eq!(fixed64(&1.5_f64), 8);
        let duration = buffa_types::google::protobuf::Duration {
            seconds: 1,
            ..Default::default()
        };
        assert_eq!(message(&duration), 2);
    }
}
