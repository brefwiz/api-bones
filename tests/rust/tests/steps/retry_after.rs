// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! The Rust half of `../../../features/connect_retry_after.feature`.
//!
//! The TypeScript half answers the same file; neither keeps its own copy of
//! the rows.
//!
//! A real connectrpc server on loopback refuses the call with the shipped
//! `resource_exhausted_retry_after`, and the generated client reads the refusal
//! off the wire, so the header, the detail and the message the steps check are
//! the ones that crossed the stack.

use std::sync::Arc;
use std::time::Duration;

use api_bones::connect::{RETRY_INFO_TYPE, RetryInfo, resource_exhausted_retry_after, retry_after};
use api_bones_contract_rust::{
    BatchGetWidgetsRequest, BatchGetWidgetsResponse, GetWidgetRequest, Widget, Widgets,
    WidgetsClient, WidgetsExt,
};
use api_bones_transport::HttpTransport;
use connectrpc::client::ClientConfig;
use connectrpc::{
    ConnectError, ErrorCode, ErrorDetail, RequestContext, Router, ServiceRequest, ServiceResult,
};
use cucumber::{given, then, when};
use http::HeaderValue;
use http::header::{CACHE_CONTROL, RETRY_AFTER};

use super::support::{serve_loopback, unserved_widget_methods};
use crate::world::RetryAfterWorld;

/// The refusal text of the scenario, free text that names no component.
const MESSAGE: &str = "slow down";

/// A service that refuses every call, asking the caller to wait `wait`.
#[derive(Clone)]
struct Refuser {
    wait: Duration,
}

impl Widgets for Refuser {
    async fn get_widget<'a>(
        &'a self,
        _ctx: RequestContext,
        _request: ServiceRequest<'_, GetWidgetRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<Widget> + Send + use<'a>> {
        ServiceResult::<Widget>::Err(resource_exhausted_retry_after(MESSAGE, self.wait))
    }

    async fn batch_get_widgets<'a>(
        &'a self,
        _ctx: RequestContext,
        _request: ServiceRequest<'_, BatchGetWidgetsRequest>,
    ) -> ServiceResult<impl connectrpc::Encodable<BatchGetWidgetsResponse> + Send + use<'a>> {
        ServiceResult::<BatchGetWidgetsResponse>::Err(ConnectError::unimplemented(
            "the refuser serves only the method under test",
        ))
    }

    unserved_widget_methods!("the refuser serves only the method under test");
}

fn failure(world: &RetryAfterWorld) -> &ConnectError {
    world.failure.as_ref().expect("no refusal given or sent")
}

#[given(expr = "a service that refuses a call and asks the caller to wait {int} ms")]
fn given_service(world: &mut RetryAfterWorld, millis: u64) {
    world.wait = Duration::from_millis(millis);
}

#[when("the refused call is sent")]
async fn when_sent(world: &mut RetryAfterWorld) {
    let router = WidgetsExt::register(Arc::new(Refuser { wait: world.wait }), Router::new());
    let (server, address) = serve_loopback(router).await;
    world.server = Some(server);
    let config = ClientConfig::new(format!("http://{address}").parse().expect("a loopback uri"));
    let client = WidgetsClient::new(HttpTransport::h2c(), config);
    let outcome = client
        .get_widget(GetWidgetRequest {
            id: "w1".into(),
            ..Default::default()
        })
        .await;
    world.failure = Some(outcome.expect_err("the refuser refuses every call"));
}

#[given(expr = "a refusal with Retry-After {string} and RetryInfo {string}")]
fn given_refusal(world: &mut RetryAfterWorld, header: String, info: String) {
    let mut refusal = ConnectError::new(ErrorCode::ResourceExhausted, MESSAGE);
    if header != "none" {
        refusal.response_headers_mut().insert(
            RETRY_AFTER,
            HeaderValue::from_str(&header).expect("the contract states a header value"),
        );
    }
    if info != "none" {
        let seconds = info
            .parse::<u64>()
            .expect("the contract states whole seconds");
        refusal = refusal.with_detail(RetryInfo::new(Duration::from_secs(seconds)).to_detail());
    }
    world.failure = Some(refusal);
}

#[given(expr = "a refusal with the RetryInfo wire value {string}")]
fn given_wire(world: &mut RetryAfterWorld, wire: String) {
    let detail = ErrorDetail {
        type_url: RETRY_INFO_TYPE.into(),
        value: Some(wire),
        debug: None,
    };
    world.failure =
        Some(ConnectError::new(ErrorCode::ResourceExhausted, MESSAGE).with_detail(detail));
}

#[when("the client reads the wait")]
fn when_read(world: &mut RetryAfterWorld) {
    world.read = Some(retry_after(failure(world)));
}

#[then(expr = "the call is refused as {string}")]
fn then_code(world: &mut RetryAfterWorld, code: String) {
    assert_eq!(super::name_of(failure(world).code), code);
}

#[then(expr = "the refusal's message is {string}")]
fn then_message(world: &mut RetryAfterWorld, message: String) {
    assert_eq!(failure(world).message.as_deref(), Some(message.as_str()));
}

#[then(expr = "the refusal's Retry-After is {string}")]
fn then_header(world: &mut RetryAfterWorld, seconds: String) {
    assert_eq!(
        failure(world)
            .response_headers()
            .get(RETRY_AFTER)
            .expect("the refusal carries Retry-After"),
        seconds.as_str()
    );
}

#[then(expr = "the refusal's Cache-Control is {string}")]
fn then_cache_control(world: &mut RetryAfterWorld, value: String) {
    assert_eq!(
        failure(world)
            .response_headers()
            .get(CACHE_CONTROL)
            .expect("the refusal carries Cache-Control"),
        value.as_str()
    );
}

#[then(expr = "the refusal's RetryInfo is {int} seconds")]
fn then_info(world: &mut RetryAfterWorld, seconds: u64) {
    let mut refusal = failure(world).clone();
    refusal.response_headers_mut().remove(RETRY_AFTER);
    assert_eq!(retry_after(&refusal), Some(Duration::from_secs(seconds)));
}

#[then(expr = "the wait the client reads is {string}")]
fn then_read(world: &mut RetryAfterWorld, millis: String) {
    let expected = (millis != "none").then(|| {
        Duration::from_millis(
            millis
                .parse::<u64>()
                .expect("the contract states milliseconds or none"),
        )
    });
    assert_eq!(world.read.expect("the wait was not read"), expected);
}

#[then(expr = "a RetryInfo of {int} seconds is written as {string}")]
fn then_written(_world: &mut RetryAfterWorld, seconds: u64, wire: String) {
    assert_eq!(
        RetryInfo::new(Duration::from_secs(seconds))
            .to_detail()
            .value
            .as_deref(),
        Some(wire.as_str())
    );
}
