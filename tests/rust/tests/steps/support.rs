// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! What the lanes' loopback `Widgets` servers have in common: the answer of
//! `BatchGetWidgets` and the methods no lane serves.

use api_bones_contract_rust::{
    BatchGetWidgetsResponse, ItemError, Widget, WidgetResult, widget_result,
};

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
                Some(widget) => widget_result::Outcome::Item(Box::new(widget)),
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

/// The `Widgets` methods no lane calls, each answered `unimplemented` with
/// `$why`, except `list_widget_parts` when the block says it is
/// `serving` it. Expands inside an `impl Widgets for ...` block.
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
        unserved_widget_methods!(@method list_widget_parts_idempotent, ListWidgetPartsRequest, ListWidgetPartsResponse, $why);
        unserved_widget_methods!(@method list_widget_parts_unspecified, ListWidgetPartsRequest, ListWidgetPartsResponse, $why);
        unserved_widget_methods!(@method update_widget_parts, UpdateWidgetPartsRequest, ListWidgetPartsResponse, $why);
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
