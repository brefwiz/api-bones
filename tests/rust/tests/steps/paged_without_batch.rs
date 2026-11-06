// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! Paging on a service that declares no batch method, served by a real
//! in-process connectrpc server and called through the generated client.
use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::world::UnbatchedPagingWorld;
use api_bones_batch::Batched;
use api_bones_contract_rust::{
    FieldsClient, ListNumbersRequest, ListNumbersResponse, Numbers, NumbersClient, NumbersExt,
    NumbersLists, PageResponse,
};
use connectrpc::client::{ClientConfig, HttpClient};
use connectrpc::{
    ConnectError, ConnectRpcService, ErrorCode, RequestContext, Response, Router, Server,
    ServiceRequest, ServiceResult,
};
use cucumber::{given, then, when};
use futures_util::StreamExt;

/// Serves the configured pages; the cursor `cN` names page `N`.
#[derive(Clone)]
struct PagedNumbers {
    pages: Arc<Vec<Vec<u32>>>,
    requests: Arc<std::sync::atomic::AtomicUsize>,
}

impl PagedNumbers {
    fn page(&self, request: ListNumbersRequest) -> ListNumbersResponse {
        self.requests.fetch_add(1, Ordering::SeqCst);
        let cursor = request.page.into_option().unwrap_or_default().cursor;
        let index = cursor
            .strip_prefix('c')
            .and_then(|index| index.parse::<usize>().ok())
            .unwrap_or(0);
        let next_cursor = if index + 1 < self.pages.len() {
            format!("c{}", index + 1)
        } else {
            String::new()
        };
        ListNumbersResponse {
            items: self.pages.get(index).cloned().unwrap_or_default(),
            page: PageResponse {
                next_cursor,
                revision: "r1".into(),
                ..Default::default()
            }
            .into(),
            ..Default::default()
        }
    }
}

impl Numbers for PagedNumbers {
    async fn list_no_side_effects<'a>(
        &'a self,
        _ctx: RequestContext,
        request: ServiceRequest<'_, ListNumbersRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<ListNumbersResponse> + Send + use<'a>> {
        Response::ok(self.page(request.to_owned_message()))
    }

    async fn list_idempotent<'a>(
        &'a self,
        _ctx: RequestContext,
        request: ServiceRequest<'_, ListNumbersRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<ListNumbersResponse> + Send + use<'a>> {
        Response::ok(self.page(request.to_owned_message()))
    }

    async fn list<'a>(
        &'a self,
        _ctx: RequestContext,
        request: ServiceRequest<'_, ListNumbersRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<ListNumbersResponse> + Send + use<'a>> {
        Response::ok(self.page(request.to_owned_message()))
    }
}

fn numbers(list: &str) -> Vec<u32> {
    list.split(',')
        .filter(|number| !number.is_empty())
        .map(|number| number.parse().expect("a number"))
        .collect()
}

#[given(expr = "a list-only service whose pages are {string}")]
fn given_pages(world: &mut UnbatchedPagingWorld, pages: String) {
    world.pages = pages.split('|').map(numbers).collect();
}

#[when("the caller reads every item of the list-only service through its platform client")]
async fn when_read_items(world: &mut UnbatchedPagingWorld) {
    let bound = Server::bind("127.0.0.1:0")
        .await
        .expect("the numbers server binds");
    let address = bound
        .local_addr()
        .expect("the numbers server has an address");
    let service = PagedNumbers {
        pages: Arc::new(world.pages.clone()),
        requests: world.requests.clone(),
    };
    let router = NumbersExt::register(Arc::new(service), Router::new());
    world.server = Some(tokio::spawn(async move {
        let _ = bound
            .serve_with_service(ConnectRpcService::new(router))
            .await;
    }));
    let config = ClientConfig::new(format!("http://{address}").parse().expect("a loopback uri"));
    let client = Batched::anonymous(NumbersClient::new(
        HttpClient::plaintext_http2_only(),
        config,
    ))
    .expect("a list-only client binds without a batch method");
    let results: Vec<_> =
        api_bones_page::ListCall::items(client.list_no_side_effects(ListNumbersRequest::default()))
            .collect()
            .await;
    world.items = results
        .into_iter()
        .map(|item| item.expect("every page is delivered"))
        .collect();
}

#[then(expr = "the caller receives the numbers {string}")]
fn then_numbers(world: &mut UnbatchedPagingWorld, expected: String) {
    assert_eq!(world.items, numbers(&expected));
}

#[then(expr = "the list-only service answered {int} list requests")]
fn then_requests(world: &mut UnbatchedPagingWorld, count: usize) {
    assert_eq!(world.requests.load(Ordering::SeqCst), count);
}

#[given("a service with neither batch nor paging methods")]
fn given_plain_service(_world: &mut UnbatchedPagingWorld) {}

#[when("the SDK constructs a platform client for that service")]
fn when_construct_plain(world: &mut UnbatchedPagingWorld) {
    let config = ClientConfig::new("http://127.0.0.1:1".parse().expect("a loopback uri"));
    world.refusal = Batched::anonymous(FieldsClient::new(
        HttpClient::plaintext_http2_only(),
        config,
    ))
    .err();
}

#[then(expr = "platform client construction fails with {string}")]
fn then_refused(world: &mut UnbatchedPagingWorld, message: String) {
    let error: &ConnectError = world.refusal.as_ref().expect("construction was refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert!(
        error
            .message
            .as_deref()
            .is_some_and(|actual| actual.contains(&message)),
        "expected {message:?}, got {:?}",
        error.message
    );
}
