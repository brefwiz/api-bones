// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! What the lanes' loopback `WidgetsService` servers have in common: the answer of
//! `BatchGetWidgets` and the methods no lane serves, and the loopback server
//! every lane spawns.

use std::net::SocketAddr;

use api_bones_contract_rust::{
    BatchGetWidgetsResponse, GetWidgetResponse, ItemError, Widget, WidgetResult, widget_result,
};

/// Serves `router` on an ephemeral loopback port: the task running the server
/// and the address it listens on.
pub async fn serve_loopback(
    router: connectrpc::Router,
) -> (tokio::task::JoinHandle<()>, SocketAddr) {
    let bound = connectrpc::Server::bind("127.0.0.1:0")
        .await
        .expect("the loopback server binds");
    let address = bound
        .local_addr()
        .expect("the loopback server has an address");
    let task = tokio::spawn(async move {
        let _ = bound
            .serve_with_service(connectrpc::ConnectRpcService::new(router))
            .await;
    });
    (task, address)
}

/// The answer of `BatchGetWidgets` over `ids`: one result per id, in request
/// order, holding the widget `found` gives for it or a `not_found` item error.
pub fn batch_response(
    ids: &[String],
    found: impl Fn(&str) -> Option<Widget>,
) -> BatchGetWidgetsResponse {
    let results = ids
        .iter()
        .map(|id| WidgetResult {
            id: id.clone(),
            outcome: Some(match found(id) {
                Some(widget) => widget_result::Outcome::Item(Box::new(GetWidgetResponse {
                    widget: widget.into(),
                    ..Default::default()
                })),
                None => widget_result::Outcome::Error(Box::new(ItemError {
                    code: "not_found".to_owned(),
                    message: "no such widget".to_owned(),
                    ..Default::default()
                })),
            }),
            ..Default::default()
        })
        .collect();
    BatchGetWidgetsResponse {
        results,
        ..Default::default()
    }
}

/// The `WidgetsService` methods no lane calls, each answered `unimplemented` with
/// `$why`, except `list_widget_parts` when the block says it is
/// `serving` it. Expands inside an `impl WidgetsService for ...` block.
macro_rules! unserved_widget_methods {
    (@method $name:ident, $request:ident, $response:ident, $why:literal) => {
        async fn $name<'a>(
            &'a self,
            _ctx: ::connectrpc::RequestContext,
            _request: ::connectrpc::ServiceRequest<'_, ::api_bones_contract_rust::$request>,
        ) -> ::connectrpc::ServiceResult<
            impl ::connectrpc::Encodable<::api_bones_contract_rust::$response> + Send + use<'a>,
        > {
            ::connectrpc::ServiceResult::<::api_bones_contract_rust::$response>::Err(
                ::connectrpc::ConnectError::unimplemented($why),
            )
        }
    };
    (@others $why:literal) => {
        unserved_widget_methods!(@method describe_widget, DescribeWidgetRequest, DescribeWidgetResponse, $why);
        unserved_widget_methods!(@method batch_describe_widgets, BatchDescribeWidgetsRequest, BatchDescribeWidgetsResponse, $why);
        unserved_widget_methods!(@method list_widget_parts_idempotent, ListWidgetPartsIdempotentRequest, ListWidgetPartsIdempotentResponse, $why);
        unserved_widget_methods!(@method list_widget_parts_unspecified, ListWidgetPartsUnspecifiedRequest, ListWidgetPartsUnspecifiedResponse, $why);
        unserved_widget_methods!(@method update_widget_parts, UpdateWidgetPartsRequest, UpdateWidgetPartsResponse, $why);
        unserved_widget_methods!(@method batch_list_widget_parts, BatchListWidgetPartsRequest, BatchListWidgetPartsResponse, $why);
    };
    ($why:literal, serving list_widget_parts) => {
        unserved_widget_methods!(@others $why);
    };
    ($why:literal) => {
        unserved_widget_methods!(@method list_widget_parts, ListWidgetPartsRequest, ListWidgetPartsResponse, $why);
        unserved_widget_methods!(@others $why);
    };
}

pub(crate) use unserved_widget_methods;
