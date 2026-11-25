// SPDX-License-Identifier: MIT
//! The per-method glue a generated SDK declares for the batch, page and
//! stream runtimes, written once against the probe's `Catalog` service. Each
//! block is compiled only into the variants that carry that capability.

#[cfg(any(feature = "page", feature = "batch"))]
mod batched {
    use api_bones_transport::HttpTransport;

    use crate::generated::bones::probe::v1::{
        BatchGetItemsRequest, CatalogClient, GetItemRequest, Item, ListItemsRequest,
        ListItemsResponse,
    };

    /// The descriptor set the batch runtime reads its relations from.
    const DESCRIPTOR: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/probe.fds"));

    api_bones_batch::batch_client!(
        client: CatalogClient<HttpTransport>,
        service: "bones.probe.v1.Catalog",
        descriptor: DESCRIPTOR,
    );

    api_bones_batch::paged_request!(ListItemsRequest, page);
    api_bones_batch::paged_response!(ListItemsResponse, item: Item, items: items, page: page);

    impl From<&GetItemRequest> for BatchGetItemsRequest {
        fn from(request: &GetItemRequest) -> Self {
            Self {
                org: request.org.clone(),
                ..Self::default()
            }
        }
    }

    #[cfg(feature = "page")]
    api_bones_batch::paged_method!(
        ext: CatalogPaged,
        client: CatalogClient<HttpTransport>,
        lists: [{
            method: "bones.probe.v1.Catalog/ListItems",
            name: list_items,
            with_options: list_items_with_options,
            idempotency: api_bones_batch::__private::Idempotency::NoSideEffects,
            request: ListItemsRequest,
            response: ListItemsResponse,
        }],
    );

    #[cfg(feature = "batch")]
    api_bones_batch::batch_method! {
        ext: CatalogBatch,
        client: CatalogClient<HttpTransport>,
        get: {
            method: "bones.probe.v1.Catalog/GetItem",
            name: get_item,
            with_options: get_item_with_options,
            request: GetItemRequest,
            id: id,
            item: Item,
        },
        batch: {
            method: "bones.probe.v1.Catalog/BatchGetItems",
            name: batch_get_items,
            with_options: batch_get_items_with_options,
            template: BatchGetItemsRequest,
            ids: ids,
            ids_field: 1,
            results: results,
            outcome: crate::generated::bones::probe::v1::item_result::Outcome,
        },
    }
}

#[cfg(any(feature = "page", feature = "batch"))]
pub(crate) use batched::*;

#[cfg(feature = "stream")]
api_bones_stream::stream_controlled!(
    crate::generated::bones::probe::v1::WatchEvent,
    bones_control
);
