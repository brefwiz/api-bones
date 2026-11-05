// SPDX-License-Identifier: MIT
//! Credential-bound unary paging on the platform client.

use std::sync::Arc;

use api_bones::connect::Idempotency;
use api_bones_page::{ListCall, ListResponse, MetadataFetch, PagedRequest, PagedResponse};
use connectrpc::ConnectError;
use connectrpc::client::CallOptions;
use http::HeaderMap;
use http::header::{HeaderName, IF_NONE_MATCH};

use crate::key::{CallHeaders, Credential, CredentialGuard};
use crate::{Batched, Boxed, header_hash};

/// Server answer header: the cached revision chain is still current.
pub const BONES_NOT_MODIFIED: HeaderName = HeaderName::from_static("bones-not-modified");
/// Client request header: the caller holds a list revision cache.
pub const BONES_LIST_CACHE: HeaderName = HeaderName::from_static("bones-list-cache");

/// The generated unary List transport, preserving response metadata.
pub type ListFn<Req, Resp> =
    Arc<dyn Fn(Req, CallOptions) -> Boxed<Result<ListResponse<Resp>, ConnectError>> + Send + Sync>;

/// Whether the server confirmed the complete cached revision chain.
#[must_use]
pub fn not_modified(headers: &HeaderMap) -> bool {
    headers
        .get(&BONES_NOT_MODIFIED)
        .is_some_and(|value| value == "1")
}

fn identity(credential: &Credential) -> String {
    if !credential.headers.iter().any(|(name, _)| {
        name.eq_ignore_ascii_case("authorization")
            || name.eq_ignore_ascii_case("brefwiz-relayed-credential")
    }) {
        return String::new();
    }
    format!(
        "{}:{}{}",
        credential.identity.subject.len(),
        credential.identity.subject,
        credential.identity.org
    )
}

/// Request encoding used by generated paging glue, with its page field cleared.
#[doc(hidden)]
pub trait CanonicalPagedRequest {
    /// Encode every request field except the page envelope.
    fn canonical_page_request(&self) -> Vec<u8>;
}

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
        let guard = wire
            .credentials
            .current()
            .map_err(|_| crate::item_error::identity_changed())
            .and_then(|credential| CredentialGuard::capture(&credential));
        let headers = headers.continuation();
        let mut canonical = canonical.to_vec();
        canonical.extend_from_slice(&header_hash(&headers.effective()));
        let identity_wire = Arc::clone(&wire);
        let identity_guard = guard.clone();
        let identity_cache = self.revisions.clone();
        let identity_source = Arc::new(move || {
            if let Ok(credential) = identity_guard
                .as_ref()
                .map_err(Clone::clone)
                .and_then(|guard| identity_wire.verify(guard))
            {
                identity(&credential)
            } else {
                identity_cache.clear();
                String::new()
            }
        });
        let cache = self.revisions.clone();
        let fetch: MetadataFetch<Req, Resp> = Arc::new(move |request, metadata| {
            let wire = Arc::clone(&wire);
            let guard = guard.clone();
            let single = Arc::clone(&single);
            let mut headers = headers.clone();
            let cache = cache.clone();
            Box::pin(async move {
                let result = async {
                    let guard = guard?;
                    let credential = wire.verify(&guard)?;
                    headers.validate(&credential.headers)?;
                    if metadata.cache_opt_in {
                        headers = headers.header(BONES_LIST_CACHE.as_str(), b"1".to_vec());
                    }
                    if let Some(revision) = metadata.if_none_match {
                        headers = headers.header(IF_NONE_MATCH.as_str(), revision.into_bytes());
                    }
                    let response =
                        single(request, headers.call_options(&credential.headers)?).await;
                    wire.verify(&guard)?;
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
            fn canonical_page_request(&self) -> Vec<u8> {
                let mut request = self.clone();
                request.$page = Default::default();
                $crate::__private::buffa::Message::encode_to_vec(&request)
            }
        }
    };
}

/// Implements the page response adapter for a generated response.
#[macro_export]
macro_rules! paged_response {
    ($response:ty, item: $item:ty, items: $items:ident, page: $page:ident $(,)?) => {
        impl $crate::__private::PagedResponse for $response {
            type Item = $item;
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
                    let canonical = $crate::__private::CanonicalPagedRequest::canonical_page_request(&request);
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
    use crate::{CredentialSource, Identity, Wire};
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
        let client = client(move || Ok(source.lock().expect("credential").clone()));
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
        let client = client(|| Ok(credential("alice")));
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
        let client = client(|| Ok(credential("alice")));
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
    async fn send_and_response_verify_identity_and_clear_existing_cache() {
        for change_before_send in [true, false] {
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
            let pending = call(&client, &CallHeaders::new(), send);
            if change_before_send {
                *current.lock().expect("credential") = credential("bob");
            }
            let error = pending.first_page().await.expect_err("changed identity");
            assert_eq!(
                rpc_error(&error).code,
                connectrpc::ErrorCode::Unauthenticated
            );
            assert_eq!(
                calls.load(Ordering::SeqCst),
                usize::from(!change_before_send)
            );
            assert!(client.revisions.is_empty());
            assert_eq!(&*held.items, &[7]);
        }
    }

    #[tokio::test]
    async fn identity_change_between_iteration_guard_and_send_refuses_dispatch() {
        let reads = Arc::new(AtomicUsize::new(0));
        let client = {
            let reads = Arc::clone(&reads);
            client(move || {
                let read = reads.fetch_add(1, Ordering::SeqCst);
                Ok(credential(if read < 3 { "alice" } else { "bob" }))
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
            let source = Arc::new(Mutex::new(Ok(credential("alice"))));
            let client = {
                let source = Arc::clone(&source);
                client(move || source.lock().expect("source").clone())
            };
            let held = call(&client, &CallHeaders::new(), success())
                .first_page()
                .await
                .expect("seed");
            let pending = call(&client, &CallHeaders::new(), success());
            *source.lock().expect("source") = if failure {
                Err(ConnectError::unavailable("source unavailable"))
            } else {
                Ok(Credential::default())
            };
            let error = pending.first_page().await.expect_err("lost credential");
            assert_eq!(
                rpc_error(&error).code,
                connectrpc::ErrorCode::Unauthenticated
            );
            assert!(client.revisions.is_empty());
            assert_eq!(&*held.items, &[7]);
        }
    }

    #[test]
    fn marker_and_identity_are_exact_and_unambiguous() {
        let mut headers = HeaderMap::new();
        assert!(!not_modified(&headers));
        headers.insert(BONES_NOT_MODIFIED, "true".parse().expect("header"));
        assert!(!not_modified(&headers));
        headers.insert(BONES_NOT_MODIFIED, "1".parse().expect("header"));
        assert!(not_modified(&headers));
        assert_eq!(identity(&Credential::default()), "");
        assert_eq!(identity(&credential("alice")), "5:aliceorg");
        let mut first = credential("ab");
        first.identity.org = "c".into();
        let mut second = credential("a");
        second.identity.org = "bc".into();
        assert_ne!(identity(&first), identity(&second));
    }
}
