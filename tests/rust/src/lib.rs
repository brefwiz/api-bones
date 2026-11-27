// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! Step-lane crate: the steps live in `tests/`; this target exists so cargo
//! has something to hang them on. It owns the generated stream response types
//! the rotation steps use: `WatchEvent`, declared with `stream_controlled!`,
//! and `PlainEvent`, which carries no stream control. It also owns the generated
//! `NumbersService` list service the pagination steps serve and call over the wire,
//! and the `WidgetsService` service whose batch glue the batch steps drive.

pub mod batch_descriptor;
#[cfg(test)]
mod error_arg_interop;
#[cfg(test)]
mod nested_descriptor;
pub mod widget_fixture;

use std::marker::PhantomData;

use api_bones_stream::{StreamControl, StreamControlled};
use buffa::Message;

/// Rust types generated at build time from `proto/contract/v1/watch.proto`.
#[expect(
    clippy::elidable_lifetime_names,
    clippy::missing_fields_in_debug,
    clippy::pub_underscore_fields,
    reason = "connectrpc and buffa generate this module; a fix here dies at the next codegen run"
)]
mod generated {
    connectrpc::include_generated!();
}

/// Canonical wire messages generated for the contract service.
pub use generated::bones::v1 as field_wire;
pub use generated::bones::v1::PageResponse;
pub use generated::bones::v1::{ErrorInfo as WireErrorInfo, ItemError, ValidationFailure};
pub use generated::contract::v1::{
    BatchGetWidgetsRequest, BatchGetWidgetsResponse, BatchListWidgetPartsRequest,
    BatchListWidgetPartsResponse, GetWidgetRequest, GetWidgetResponse, ListIdempotentRequest,
    ListIdempotentResponse, ListNoSideEffectsRequest, ListNoSideEffectsResponse, ListRequest,
    ListResponse, ListWidgetPartsIdempotentRequest, ListWidgetPartsIdempotentResponse,
    ListWidgetPartsRequest, ListWidgetPartsResponse, ListWidgetPartsUnspecifiedRequest,
    ListWidgetPartsUnspecifiedResponse, NumbersService, NumbersServiceClient, NumbersServiceExt,
    PlainEvent, UpdateWidgetPartsRequest, UpdateWidgetPartsResponse, WatchEvent, Widget,
    WidgetPartsGroup, WidgetResult, WidgetsService, WidgetsServiceClient, WidgetsServiceExt,
    watch_event, widget_result,
};
pub use generated::contract::v1::{
    EventsService, EventsServiceExt, Tick, WatchRequest, WatchResponse,
};
pub use generated::contract::v1::{
    FieldsService, FieldsServiceClient, FieldsServiceExt, ValidateRequest, ValidateResponse,
};

/// Generated list glue. It lives in its own module so the root scope binds no
/// name the opaque generated file could shadow; only the list traits are
/// re-exported, by name.
mod paged {
    include!(concat!(env!("OUT_DIR"), "/paged.rs"));
}
pub use paged::{NumbersServiceLists, WidgetsServiceLists};

impl From<&GetWidgetRequest> for BatchGetWidgetsRequest {
    fn from(request: &GetWidgetRequest) -> Self {
        Self {
            kind: request.kind.clone(),
            ..Default::default()
        }
    }
}

impl api_bones_batch::ListRequestFor<BatchListWidgetPartsRequest> for ListWidgetPartsRequest {
    fn for_parent(template: &BatchListWidgetPartsRequest, parent_id: &str) -> Self {
        Self {
            widget_id: parent_id.to_owned(),
            page: template.page.clone(),
            ..Default::default()
        }
    }
}

/// The generated descriptor set used by the contract clients.
pub const CONTRACT_DESCRIPTOR: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/watch.fds"));

api_bones_batch::batch_client! {
    client: NumbersServiceClient<api_bones_transport::HttpTransport>,
    service: "contract.v1.NumbersService",
    descriptor: CONTRACT_DESCRIPTOR,
}

api_bones_batch::batch_client! {
    client: WidgetsServiceClient<api_bones_transport::HttpTransport>,
    service: "contract.v1.WidgetsService",
    descriptor: CONTRACT_DESCRIPTOR,
}

api_bones_batch::batch_method! {
    ext: WidgetGets,
    client: WidgetsServiceClient<api_bones_transport::HttpTransport>,
    get: {
        method: "contract.v1.WidgetsService/GetWidget",
        name: get_widget,
        with_options: get_widget_with_options,
        request: GetWidgetRequest,
        id: id,
        item: GetWidgetResponse,
    },
    batch: {
        method: "contract.v1.WidgetsService/BatchGetWidgets",
        name: batch_get_widgets,
        with_options: batch_get_widgets_with_options,
        template: BatchGetWidgetsRequest,
        ids: ids,
        ids_field: 1,
        results: results,
        outcome: widget_result::Outcome,
    },
}

api_bones_batch::batch_method! {
    ext: WidgetPartLists,
    client: WidgetsServiceClient<api_bones_transport::HttpTransport>,
    list: {
        method: "contract.v1.WidgetsService/ListWidgetParts",
        with_options: list_widget_parts_with_options,
        request: ListWidgetPartsRequest,
        response: ListWidgetPartsResponse,
    },
    batch: {
        method: "contract.v1.WidgetsService/BatchListWidgetParts",
        name: batch_list_widget_parts,
        with_options: batch_list_widget_parts_with_options,
        template: BatchListWidgetPartsRequest,
        parents: widget_ids,
        parents_field: 1,
        groups: groups,
        group_parent: widget_id,
    },
}

api_bones_batch::batch_client! {
    client: FieldsServiceClient<api_bones_transport::HttpTransport>,
    service: "contract.v1.FieldsService",
    descriptor: CONTRACT_DESCRIPTOR,
}

api_bones_stream::stream_controlled!(WatchEvent, bones_control);

/// The compiler's answer to whether a response type opens through
/// [`api_bones_stream::rotating_stream`], which bounds its response type by
/// [`StreamControlled`].
///
/// The inherent `ACCEPTS` exists only where `T` implements that trait, and an
/// inherent constant is found before a trait's. So
/// `RotatingStreamProbe::<T>::ACCEPTS` reads `true` there and falls back to
/// [`Refused::ACCEPTS`], `false`, everywhere else.
pub struct RotatingStreamProbe<T>(PhantomData<T>);

impl<T: StreamControlled> RotatingStreamProbe<T> {
    /// `rotating_stream` accepts `T`.
    pub const ACCEPTS: bool = true;
}

/// The answer for a response type `rotating_stream` does not accept.
pub trait Refused {
    /// `rotating_stream` does not accept the type.
    const ACCEPTS: bool = false;
}

impl<T> Refused for RotatingStreamProbe<T> {}

/// The generated message a server sends for `control` alone: its control-only
/// proto frame, decoded.
///
/// # Panics
/// When the codec's frame is not a valid `WatchEvent`.
#[must_use]
pub fn control_only(control: &StreamControl) -> WatchEvent {
    WatchEvent::decode_from_slice(&control.to_proto_frame()).expect("a control-only frame decodes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_control_removes_the_control_from_a_generated_message() {
        let drain = StreamControl::Drain { cut_in_ms: 5000 };
        let mut message = control_only(&drain);
        assert!(!message.is_empty());
        assert_eq!(message.take_control(), Ok(Some(drain)));
        assert!(message.is_empty());
        assert_eq!(message.take_control(), Ok(None));
    }

    #[test]
    fn a_generated_message_without_a_control_yields_none() {
        let mut message = WatchEvent::default();
        assert_eq!(message.take_control(), Ok(None));
    }
}
