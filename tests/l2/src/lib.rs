// SPDX-License-Identifier: MIT
//! The api-bones test service's Rust Layer 2.
//!
//! Every method goes through the SDK call helper, so the call budget, the
//! trace context and the identity checks are decided once in the helper and
//! never here. This crate only builds the generated Layer 1 request, makes the
//! one RPC with the [`Attempt`] it is handed, and maps the answer to the
//! records the SDK macros export.

use std::sync::Arc;

use api_bones::connect::Idempotency;
use api_bones_page::{PageInfo, PageOptions, PagedRequest, PagedResponse};
use api_bones_rt::{BoxFuture, MaybeSend};
use api_bones_sdk::call::{Attempt, CallEnv, PageSend};
use api_bones_sdk::{
    Attributes, Batch, CallCtx, Error, Handle, ListCall, SpanKind, Stream, Tracer,
};
use api_bones_sdk_macros::{client, record};
use api_bones_transport::HttpTransport;
use connectrpc::client::ClientConfig;
use connectrpc::{ConnectError, ErrorCode};
use futures_util::stream;

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
    BatchGetWidgetsRequest, EventsClient as EventsRpc, GetWidgetRequest, ListNumbersRequest,
    ListNumbersResponse, NumbersClient as NumbersRpc, Tick as TickWire, WatchTicksRequest,
    WidgetsClient as WidgetsRpc,
};

pub const NUMBERS: &str = "contract.v1.Numbers";
pub const WIDGETS: &str = "contract.v1.Widgets";
pub const EVENTS: &str = "contract.v1.Events";

/// One tick of the `Events` stream.
#[record]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tick {
    pub sequence: u32,
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

/// One number of the `Numbers` list.
#[record]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Number {
    pub value: u32,
}

/// One widget of the `Widgets` service.
#[record]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Widget {
    pub id: String,
    pub name: String,
    pub kind: String,
}

impl From<generated::contract::v1::Widget> for Widget {
    fn from(widget: generated::contract::v1::Widget) -> Self {
        Self {
            id: widget.id,
            name: widget.name,
            kind: widget.kind,
        }
    }
}

/// The page request the list call walks: a `ListNumbersRequest` whose cursor
/// and page size the walk sets.
#[derive(Clone, Default)]
pub struct NumbersPageRequest(ListNumbersRequest);

impl PagedRequest for NumbersPageRequest {
    fn set_page(&mut self, cursor: &str, page_size: Option<u32>) {
        let mut page = self.0.page.take().unwrap_or_default();
        cursor.clone_into(&mut page.cursor);
        page.page_size = page_size.unwrap_or_default();
        self.0.page = page.into();
    }
}

/// The page the list call delivers: the numbers of a `ListNumbersResponse`.
pub struct NumbersPage(ListNumbersResponse);

impl PagedResponse for NumbersPage {
    type Item = Number;

    fn split(self) -> (Vec<Number>, PageInfo) {
        let page = self.0.page.into_option().unwrap_or_default();
        let items = self
            .0
            .items
            .into_iter()
            .map(|value| Number { value })
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

/// The client configuration of `base_url`.
///
/// # Errors
/// `invalid_argument` when `base_url` is not a URI.
pub fn config_of(base_url: &str) -> Result<ClientConfig, Error> {
    let uri: http::Uri = base_url
        .parse()
        .map_err(|_| Error::invalid_argument("base_url is not a URI"))?;
    Ok(ClientConfig::new(uri))
}

/// The send of the `Numbers` list: one page per attempt, over `config`.
pub fn list_send(config: ClientConfig) -> PageSend<NumbersPageRequest, NumbersPage> {
    Arc::new(
        move |(request, attempt): (NumbersPageRequest, Attempt)|
              -> BoxFuture<'static, Result<NumbersPage, ConnectError>> {
            let config = config.clone();
            Box::pin(async move {
                let rpc = NumbersRpc::new(attempt.transport(HttpTransport::h2c()), config);
                let response = rpc
                    .list_no_side_effects_with_options(request.0, attempt.options()?)
                    .await?;
                Ok(NumbersPage(response.into_owned()))
            })
        },
    )
}

/// The `Numbers` service.
#[derive(Clone)]
pub struct NumbersClient {
    config: ClientConfig,
    env: CallEnv,
}

impl NumbersClient {
    /// A client of the server at `base_url`, calling as `env` says.
    ///
    /// # Errors
    /// `invalid_argument` when `base_url` is not a URI.
    pub fn new(base_url: &str, env: CallEnv) -> Result<Self, Error> {
        Ok(Self {
            config: config_of(base_url)?,
            env,
        })
    }
}

#[client(surface = "connect-pagination")]
impl NumbersClient {
    /// The numbers, one page per `next()`.
    #[export]
    pub async fn list(&self, page_size: Option<u32>) -> ListCall<Number> {
        let send = list_send(self.config.clone());
        let options = PageOptions {
            page_size,
            ..PageOptions::default()
        };
        self.env.call(NUMBERS, "ListNoSideEffects").list(
            Idempotency::NoSideEffects,
            NumbersPageRequest(ListNumbersRequest::default()),
            send,
            options,
        )
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
    let rpc = WidgetsRpc::new(attempt.transport(HttpTransport::h2c()), config);
    let response = rpc
        .get_widget_with_options(request, attempt.options()?)
        .await?;
    Ok(Widget::from(response.into_owned()))
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
    let rpc = WidgetsRpc::new(attempt.transport(HttpTransport::h2c()), config);
    let response = rpc
        .batch_get_widgets_with_options(request, attempt.options()?)
        .await?
        .into_owned();
    Ok(response
        .results
        .into_iter()
        .map(|result| match result.outcome {
            Some(Outcome::Item(widget)) => Ok(Widget::from(*widget)),
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
pub struct WidgetsClient {
    config: ClientConfig,
    env: CallEnv,
}

impl WidgetsClient {
    /// A client of the server at `base_url`, calling as `env` says.
    ///
    /// # Errors
    /// `invalid_argument` when `base_url` is not a URI.
    pub fn new(base_url: &str, env: CallEnv) -> Result<Self, Error> {
        Ok(Self {
            config: config_of(base_url)?,
            env,
        })
    }
}

#[client(surface = "connect-batch")]
impl WidgetsClient {
    /// One widget by id.
    #[export]
    pub async fn get_widget(&self, id: String) -> Result<Widget, Error> {
        let config = self.config.clone();
        self.env
            .call(WIDGETS, "GetWidget")
            .unary(Idempotency::NoSideEffects, move |attempt| {
                get_widget_send(config.clone(), id.clone(), attempt)
            })
            .await
    }

    /// Several widgets by id, delivered as one chunk of per-id results in
    /// request order.
    #[export]
    pub async fn batch_get_widgets(&self, ids: Vec<String>) -> Batch<Widget> {
        let config = self.config.clone();
        self.env
            .call(WIDGETS, "BatchGetWidgets")
            .batch(Idempotency::NoSideEffects, move |attempt| {
                batch_get_widgets_send(config.clone(), ids.clone(), attempt)
            })
    }
}

/// The open of the `Watch` call: `count` ticks, one per item.
pub async fn watch_open(
    config: ClientConfig,
    count: u32,
    attempt: Attempt,
) -> Result<impl stream::Stream<Item = Result<Tick, ConnectError>>, ConnectError> {
    let rpc = EventsRpc::new(attempt.transport(HttpTransport::h2c()), config);
    let upstream = rpc
        .watch_with_options(
            WatchTicksRequest {
                count,
                ..Default::default()
            },
            attempt.options()?,
        )
        .await?;
    Ok(stream::unfold(upstream, |mut upstream| async move {
        match upstream.message::<TickWire>().await {
            Ok(Some(tick)) => Some((
                Ok(Tick {
                    sequence: tick.to_owned_message().sequence,
                }),
                upstream,
            )),
            Ok(None) => None,
            Err(error) => Some((Err(error), upstream)),
        }
    }))
}

/// The `Events` service.
#[derive(Clone)]
pub struct EventsClient {
    config: ClientConfig,
    env: CallEnv,
}

impl EventsClient {
    /// A client of the server at `base_url`, calling as `env` says.
    ///
    /// # Errors
    /// `invalid_argument` when `base_url` is not a URI.
    pub fn new(base_url: &str, env: CallEnv) -> Result<Self, Error> {
        Ok(Self {
            config: config_of(base_url)?,
            env,
        })
    }
}

#[client(surface = "sdk-call")]
impl EventsClient {
    /// `count` ticks, one per `next()`. A stream that cannot open fails its
    /// first `next()`.
    #[export]
    pub async fn watch(&self, count: u32) -> Stream<Tick> {
        let config = self.config.clone();
        let opened = self
            .env
            .call(EVENTS, "Watch")
            .stream(Idempotency::NoSideEffects, move |attempt| {
                watch_open(config.clone(), count, attempt)
            })
            .await;
        opened.unwrap_or_else(refused)
    }
}
