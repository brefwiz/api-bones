// SPDX-License-Identifier: MIT
//! The api-bones test service's Rust Layer 2.
//!
//! Every method goes through the SDK call helper, so the call budget, the
//! trace context and the identity checks are decided once in the helper and
//! never here. This crate only builds the generated Layer 1 request, makes the
//! one RPC with the [`Attempt`] it is handed, and hands the answer back as the
//! generated message the SDK macros export it as. The messages the exports take
//! and return are declared in the protos under `proto/`, and the macros read
//! them by their full path.

use std::sync::Arc;

use api_bones::connect::Idempotency;
use api_bones_page::{CanonicalPagedRequest, PageInfo, PageOptions, PagedRequest, PagedResponse};
use api_bones_rt::{BoxFuture, MaybeSend};
use api_bones_sdk::call::{Attempt, CallEnv, ListResponse, PageSend, not_modified};
use api_bones_sdk::{
    Attributes, Batch, CallCtx, Error, Handle, ListCall, SpanKind, Stream, Tracer,
};
use api_bones_sdk_macros::client;
use api_bones_transport::HttpTransport;
use connectrpc::client::ClientConfig;
use connectrpc::{ConnectError, ErrorCode};
use futures_util::stream;

/// The transport of the target: cleartext HTTP/2 to the loopback test server
/// natively, the host's `fetch` in the browser.
#[cfg(not(target_arch = "wasm32"))]
fn transport() -> HttpTransport {
    HttpTransport::h2c()
}

/// The transport of the target: cleartext HTTP/2 to the loopback test server
/// natively, the host's `fetch` in the browser.
#[cfg(target_arch = "wasm32")]
fn transport() -> HttpTransport {
    HttpTransport::fetch()
}

/// Layer 1: generated at build time from the contract protos.
#[expect(
    clippy::elidable_lifetime_names,
    clippy::missing_fields_in_debug,
    clippy::pub_underscore_fields,
    reason = "connectrpc and buffa generate this module; a fix here dies at the next codegen run"
)]
mod generated {
    connectrpc::include_generated!();
}

use generated::contract::v1::widget_result::Outcome;
use generated::contract::v1::{
    BatchGetWidgetsRequest, EventsServiceClient as EventsRpc, GetWidgetRequest,
    ListNoSideEffectsRequest, ListNoSideEffectsResponse, ListWidgetPartsRequest,
    ListWidgetPartsResponse, NumbersServiceClient as NumbersRpc, WatchRequest, WatchResponse,
    WidgetsServiceClient as WidgetsRpc,
};

/// One tick of the `Events` stream.
pub type Tick = generated::contract::v1::Tick;

/// One number of the `Numbers` list.
pub type Number = generated::contract::sdk::v1::Number;

/// The eight-field request of `NumbersClient::echo_fields`.
pub type EightFields = generated::contract::sdk::v1::EightFields;

/// One widget of the `Widgets` service.
pub type Widget = generated::contract::v1::Widget;

/// One part of a widget.
pub type WidgetPart = generated::contract::sdk::v1::WidgetPart;

pub const NUMBERS: &str = "contract.v1.NumbersService";
pub const WIDGETS: &str = "contract.v1.WidgetsService";
pub const EVENTS: &str = "contract.v1.EventsService";

/// The server's client configuration and the env a client's calls run in.
#[derive(Clone)]
struct Conn {
    config: ClientConfig,
    env: CallEnv,
}

impl Conn {
    fn at(base_url: &str, env: CallEnv) -> Result<Self, Error> {
        let (config, env) = env.at(base_url)?;
        Ok(Self { config, env })
    }
}

/// Every client of the test service, opened from one caller's env.
#[derive(Clone)]
pub struct ApiBonesSdk {
    /// The `Numbers` service.
    pub numbers_client: NumbersClient,
    /// The `Widgets` service.
    pub widgets_client: WidgetsClient,
    /// The `Events` service.
    pub events_client: EventsClient,
}

impl ApiBonesSdk {
    /// Opens every client of the service as `env` says, all against the server
    /// at `base_url`. A native process has no page whose origin could stand in,
    /// so a missing `base_url` is refused.
    ///
    /// # Errors
    /// `invalid_argument` when `base_url` is missing or is not a URI.
    pub fn open(env: &CallEnv, base_url: Option<&str>) -> Result<Self, Error> {
        let base_url =
            base_url.ok_or_else(|| Error::invalid_argument("no page origin; pass baseUrl"))?;
        Ok(Self {
            numbers_client: NumbersClient::new(base_url, env.clone())?,
            widgets_client: WidgetsClient::new(base_url, env.clone())?,
            events_client: EventsClient::new(base_url, env.clone())?,
        })
    }
}

/// The wasm constructor of a client: the host says how to authenticate and
/// where to report spans.
macro_rules! from_host {
    ($name:ident) => {
        #[cfg(all(feature = "wasm", target_arch = "wasm32"))]
        impl $name {
            /// # Errors
            /// `invalid_argument` when `base_url` is not a URI.
            pub fn from_host(
                base_url: String,
                host: api_bones_sdk::__private::wasm_bindgen::JsValue,
            ) -> Result<Self, Error> {
                Self::new(&base_url, CallEnv::from_host(host))
            }
        }
    };
}

/// A page request that walks a list: a request message whose cursor and page
/// size the walk sets.
macro_rules! page_request {
    ($name:ident, $request:ty, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Default)]
        pub struct $name(pub $request);

        impl PagedRequest for $name {
            fn set_page(&mut self, cursor: &str, page_size: Option<u32>) {
                let mut page = self.0.page.take().unwrap_or_default();
                cursor.clone_into(&mut page.cursor);
                page.page_size = page_size.unwrap_or_default();
                self.0.page = page.into();
            }
        }

        impl CanonicalPagedRequest for $name {
            fn encode(&self) -> Vec<u8> {
                buffa::Message::encode_to_vec(&self.0)
            }
        }
    };
}

/// A stream that fails with `error` on its first `next()`: what a stream that
/// could not open hands its caller, so the failure arrives through the handle.
fn refused<T: MaybeSend + 'static>(error: Error) -> Stream<T> {
    let span = Tracer::default().start(
        &format!("{EVENTS}/Watch"),
        SpanKind::Internal,
        &CallCtx::default(),
        Attributes::new(),
    );
    Handle::new(Box::pin(stream::once(std::future::ready(Err(error)))), span)
}

page_request!(
    NumbersPageRequest,
    ListNoSideEffectsRequest,
    "The page request the list call walks."
);

/// The page the list call delivers: the numbers of a `ListNoSideEffectsResponse`.
pub struct NumbersPage(ListNoSideEffectsResponse);

impl PagedResponse for NumbersPage {
    type Item = Number;

    fn encoded_len(item: &Number) -> usize {
        buffa::types::uint32_encoded_len(item.value)
    }

    fn split(self) -> (Vec<Number>, PageInfo) {
        let page = self.0.page.into_option().unwrap_or_default();
        let items = self
            .0
            .items
            .into_iter()
            .map(|value| Number {
                value,
                ..Default::default()
            })
            .collect();
        // The test service never opts in to a collection total.
        let info = PageInfo {
            next_cursor: page.next_cursor,
            revision: page.revision,
            total: None,
        };
        (items, info)
    }
}

page_request!(
    WidgetPartsPageRequest,
    ListWidgetPartsRequest,
    "The page request of the parts of one widget."
);

/// The page the parts list delivers.
pub struct WidgetPartsPage(ListWidgetPartsResponse);

impl PagedResponse for WidgetPartsPage {
    type Item = WidgetPart;

    fn encoded_len(item: &WidgetPart) -> usize {
        buffa::types::string_encoded_len(&item.name)
    }

    fn split(self) -> (Vec<WidgetPart>, PageInfo) {
        let page = self.0.page.into_option().unwrap_or_default();
        let items = self
            .0
            .items
            .into_iter()
            .map(|name| WidgetPart {
                name,
                ..Default::default()
            })
            .collect();
        let info = PageInfo {
            next_cursor: page.next_cursor,
            revision: page.revision,
            total: None,
        };
        (items, info)
    }
}

/// The send of the `ListWidgetParts` list: one page per attempt, over
/// `config`.
pub fn widget_parts_send(
    config: ClientConfig,
) -> PageSend<WidgetPartsPageRequest, WidgetPartsPage> {
    Arc::new(
        move |(request, attempt): (WidgetPartsPageRequest, Attempt)|
              -> BoxFuture<'static, Result<ListResponse<WidgetPartsPage>, ConnectError>> {
            let config = config.clone();
            Box::pin(async move {
                let rpc = WidgetsRpc::new(attempt.transport(transport()), config);
                let response = rpc
                    .list_widget_parts_with_options(request.0, attempt.options()?)
                    .await?;
                let confirmed = not_modified(response.headers());
                Ok(ListResponse {
                    body: WidgetPartsPage(response.into_owned()),
                    not_modified: confirmed,
                })
            })
        },
    )
}

/// The send of the `Numbers` list: one page per attempt, over `config`. The
/// answer carries whether the server confirmed the revision the attempt asked
/// about.
pub fn list_send(config: ClientConfig) -> PageSend<NumbersPageRequest, NumbersPage> {
    Arc::new(
        move |(request, attempt): (NumbersPageRequest, Attempt)|
              -> BoxFuture<'static, Result<ListResponse<NumbersPage>, ConnectError>> {
            let config = config.clone();
            Box::pin(async move {
                let rpc = NumbersRpc::new(attempt.transport(transport()), config);
                let response = rpc
                    .list_no_side_effects_with_options(request.0, attempt.options()?)
                    .await?;
                let confirmed = not_modified(response.headers());
                Ok(ListResponse {
                    body: NumbersPage(response.into_owned()),
                    not_modified: confirmed,
                })
            })
        },
    )
}

/// The `Numbers` service.
#[derive(Clone)]
pub struct NumbersClient {
    conn: Conn,
    default_page_size: Option<u32>,
}

impl NumbersClient {
    /// A client of the server at `base_url`, calling as `env` says.
    ///
    /// # Errors
    /// `invalid_argument` when `base_url` is not a URI.
    pub fn new(base_url: &str, env: CallEnv) -> Result<Self, Error> {
        Ok(Self {
            conn: Conn::at(base_url, env)?,
            default_page_size: None,
        })
    }

    /// The same client asking for pages of `size` when a call names none;
    /// `None` leaves the page size to the server.
    #[must_use]
    pub fn with_default_page_size(mut self, size: Option<u32>) -> Self {
        self.default_page_size = size;
        self
    }
}

#[client(surface = "sdk-call", proto = generated::contract::sdk::v1)]
impl NumbersClient {
    /// The wasm constructor: the host says how to authenticate and where to
    /// report spans, and the options message may set the page size a list asks
    /// for by default.
    ///
    /// # Errors
    /// `invalid_argument` when `base_url` is not a URI.
    #[cfg(all(feature = "wasm", target_arch = "wasm32"))]
    pub fn from_host(
        base_url: String,
        host: api_bones_sdk::__private::wasm_bindgen::JsValue,
        options: generated::contract::sdk::v1::NumbersOptions,
    ) -> Result<Self, Error> {
        let client = Self::new(&base_url, CallEnv::from_host(host))?;
        Ok(Self {
            default_page_size: options.default_page_size,
            ..client
        })
    }

    /// The numbers, one page per `next()`.
    #[export(args = generated::contract::sdk::v1::ListNumbersArgs)]
    pub fn list(&self, page_size: Option<u32>) -> ListCall<generated::contract::sdk::v1::Number> {
        let send = list_send(self.conn.config.clone());
        let options = PageOptions {
            page_size: page_size.or(self.default_page_size),
            ..PageOptions::default()
        };
        self.conn.env.call(NUMBERS, "ListNoSideEffects").list(
            Idempotency::NoSideEffects,
            NumbersPageRequest(ListNoSideEffectsRequest::default()),
            send,
            options,
        )
    }

    /// The largest page size the test service's clients ask for. A constant
    /// export: the TypeScript package reads it through a function that takes
    /// nothing.
    #[export(sync)]
    pub const MAX_PAGE_SIZE: u32 = 1000;

    /// `value` held within `low..=high`; `low` wins when the bounds cross. A
    /// static export: it needs no client, so it runs without a server.
    #[export(sync)]
    #[must_use]
    pub fn clamp(value: u32, low: u32, high: u32) -> u32 {
        value.min(high).max(low)
    }

    /// The request, answered as it came. The export takes its arguments
    /// message whole, so the eight fields arrive as one value.
    #[export(sync, args = generated::contract::sdk::v1::EightFields)]
    #[must_use]
    pub fn echo_fields(
        request: generated::contract::sdk::v1::EightFields,
    ) -> generated::contract::sdk::v1::EightFields {
        request
    }
}

/// The attempt of the `GetWidget` call: one widget by `id`.
pub async fn get_widget_send(
    config: ClientConfig,
    id: String,
    attempt: Attempt,
) -> Result<Widget, ConnectError> {
    let request = GetWidgetRequest {
        id,
        ..Default::default()
    };
    let rpc = WidgetsRpc::new(attempt.transport(transport()), config);
    let response = rpc
        .get_widget_with_options(request, attempt.options()?)
        .await?;
    Ok(response
        .into_owned()
        .widget
        .into_option()
        .unwrap_or_default())
}

/// The attempt of the `BatchGetWidgets` call: the per-id results of `ids`, in
/// request order.
pub async fn batch_get_widgets_send(
    config: ClientConfig,
    ids: Vec<String>,
    attempt: Attempt,
) -> Result<Vec<Result<Widget, ConnectError>>, ConnectError> {
    let request = BatchGetWidgetsRequest {
        ids,
        ..Default::default()
    };
    let rpc = WidgetsRpc::new(attempt.transport(transport()), config);
    let response = rpc
        .batch_get_widgets_with_options(request, attempt.options()?)
        .await?
        .into_owned();
    Ok(response
        .results
        .into_iter()
        .map(|result| match result.outcome {
            Some(Outcome::Item(item)) => Ok(item.widget.into_option().unwrap_or_default()),
            Some(Outcome::Error(error)) => Err(ConnectError::new(
                error.code.parse().unwrap_or(ErrorCode::Unknown),
                error.message,
            )),
            None => Err(ConnectError::internal("a result carried no outcome")),
        })
        .collect())
}

/// The `Widgets` service.
#[derive(Clone)]
pub struct WidgetsClient(Conn);

impl WidgetsClient {
    /// A client of the server at `base_url`, calling as `env` says.
    ///
    /// # Errors
    /// `invalid_argument` when `base_url` is not a URI.
    pub fn new(base_url: &str, env: CallEnv) -> Result<Self, Error> {
        Conn::at(base_url, env).map(Self)
    }
}

from_host!(WidgetsClient);

#[client(surface = "sdk-call", proto = generated::contract::sdk::v1)]
impl WidgetsClient {
    /// One widget by id.
    #[export]
    pub async fn get_widget(&self, id: String) -> Result<generated::contract::v1::Widget, Error> {
        let config = self.0.config.clone();
        self.0
            .env
            .call(WIDGETS, "GetWidget")
            .unary(Idempotency::NoSideEffects, move |attempt| {
                get_widget_send(config.clone(), id.clone(), attempt)
            })
            .await
    }

    /// The parts of one widget, one page per `next()`.
    #[export]
    pub fn list_widget_parts(
        &self,
        widget_id: String,
        page_size: Option<u32>,
    ) -> ListCall<generated::contract::sdk::v1::WidgetPart> {
        let send = widget_parts_send(self.0.config.clone());
        let options = PageOptions {
            page_size,
            ..PageOptions::default()
        };
        let request = ListWidgetPartsRequest {
            widget_id,
            ..Default::default()
        };
        self.0.env.call(WIDGETS, "ListWidgetParts").list(
            Idempotency::NoSideEffects,
            WidgetPartsPageRequest(request),
            send,
            options,
        )
    }

    /// Several widgets by id, delivered as one chunk of per-id results in
    /// request order.
    #[export]
    pub fn batch_get_widgets(&self, ids: Vec<String>) -> Batch<generated::contract::v1::Widget> {
        let config = self.0.config.clone();
        self.0
            .env
            .call(WIDGETS, "BatchGetWidgets")
            .batch(Idempotency::NoSideEffects, move |attempt| {
                batch_get_widgets_send(config.clone(), ids.clone(), attempt)
            })
    }
}

/// The open of the `Watch` call: `ticks` ticks, one per item.
pub async fn watch_open(
    config: ClientConfig,
    ticks: u32,
    attempt: Attempt,
) -> Result<impl stream::Stream<Item = Result<Tick, ConnectError>>, ConnectError> {
    let rpc = EventsRpc::new(attempt.transport(transport()), config);
    let upstream = rpc
        .watch_with_options(
            WatchRequest {
                count: Some(ticks),
                ..Default::default()
            },
            attempt.options()?,
        )
        .await?;
    Ok(stream::unfold(upstream, |mut upstream| async move {
        match upstream.message::<WatchResponse>().await {
            Ok(Some(response)) => {
                let tick = response.to_owned_message().tick.into_option();
                Some((Ok(tick.unwrap_or_default()), upstream))
            }
            Ok(None) => None,
            Err(error) => Some((Err(error), upstream)),
        }
    }))
}

/// The `Events` service.
#[derive(Clone)]
pub struct EventsClient(Conn);

impl EventsClient {
    /// A client of the server at `base_url`, calling as `env` says.
    ///
    /// # Errors
    /// `invalid_argument` when `base_url` is not a URI.
    pub fn new(base_url: &str, env: CallEnv) -> Result<Self, Error> {
        Conn::at(base_url, env).map(Self)
    }
}

from_host!(EventsClient);

#[client(surface = "sdk-call", proto = generated::contract::sdk::v1)]
impl EventsClient {
    /// `ticks` ticks, one per `next()`. A stream that cannot open fails its
    /// first `next()`.
    #[export(args = generated::contract::sdk::v1::WatchArgs)]
    pub async fn watch(&self, ticks: u32) -> Stream<generated::contract::v1::Tick> {
        let config = self.0.config.clone();
        let opened = self
            .0
            .env
            .call(EVENTS, "Watch")
            .stream(Idempotency::NoSideEffects, move |attempt| {
                watch_open(config.clone(), ticks, attempt)
            })
            .await;
        opened.unwrap_or_else(refused)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_holds_a_value_within_its_bounds_and_the_lower_bound_wins_a_crossed_pair() {
        assert_eq!(NumbersClient::clamp(5, 1, 10), 5);
        assert_eq!(NumbersClient::clamp(0, 1, 10), 1);
        assert_eq!(NumbersClient::clamp(50, 1, 10), 10);
        assert_eq!(NumbersClient::clamp(5, 8, 3), 8);
    }

    #[test]
    fn the_largest_page_size_is_a_page_the_clamp_leaves_alone() {
        let size = NumbersClient::MAX_PAGE_SIZE;
        assert_eq!(NumbersClient::clamp(size, 1, size), size);
        assert_eq!(NumbersClient::clamp(size + 1, 1, size), size);
    }

    #[test]
    fn every_client_opens_against_the_base_address() {
        ApiBonesSdk::open(&CallEnv::anonymous(), Some("http://localhost:8080"))
            .map(|_| ())
            .expect("a URI opens the SDK");
    }

    #[test]
    fn a_missing_base_address_is_refused_as_invalid() {
        let error = ApiBonesSdk::open(&CallEnv::anonymous(), None)
            .err()
            .expect("no base address");
        assert_eq!(error.platform, "VALIDATION_FAILED");
        assert!(error.message.contains("no page origin; pass baseUrl"));
    }

    #[test]
    fn a_base_address_that_is_not_a_uri_is_refused_as_invalid() {
        let error = ApiBonesSdk::open(&CallEnv::anonymous(), Some("not a uri"))
            .err()
            .expect("not a URI");
        assert_eq!(error.platform, "VALIDATION_FAILED");
    }
}
