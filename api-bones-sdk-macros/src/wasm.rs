// SPDX-License-Identifier: MIT
//! The wasm32 exports of a `#[client]` impl.
//!
//! Each client becomes a wasm class named `<svc>__<Type>`. Its constructor
//! export is `<svc>__<Type>__new(base_url, host)`, built from the type's
//! `from_host(base_url: String, host: JsValue) -> Self`. Each export is a
//! method named `<svc>__<Type>__<method>`, gated by the features `sdk-all`,
//! `sdk-<surface>` and `sdk-m-<method>`.
//!
//! The wire is JSON text both ways, and every export is a thin shell over the
//! Rust method:
//!
//! - An async export takes `(args_json, traceparent, tracestate, signal)` and
//!   returns a `Promise`. The call runs inside `CallCtx::scope`, so the call
//!   helper reads the caller's trace fields. The promise resolves to a JSON
//!   string and rejects with the error JSON.
//! - An export that returns a handle resolves to a class with `next()`,
//!   `cancel()` and `span()`; an abort of `signal` cancels the handle.
//! - A sync export takes `args_json` and returns the JSON string or throws the
//!   error JSON.
//!
//! Everything the code touches outside the crate is reached through
//! `api_bones_sdk::__private`.

use std::collections::BTreeSet;

use api_bones_sdk_model::{
    Client, Export, HandleKind, Return, export_name, gate_features, json_name,
};
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{FnArg, ImplItemFn, Pat, Type};

use crate::common::{head, private, upper_camel};

fn named(error: &(impl ToString + ?Sized)) -> syn::Error {
    syn::Error::new(Span::call_site(), error.to_string())
}

/// The attribute that compiles an item only for wasm32 with one of the
/// features on.
fn gate(features: &BTreeSet<String>) -> TokenStream {
    let features = features.iter();
    quote!(#[cfg(all(target_arch = "wasm32", any(#(feature = #features),*)))])
}

/// The attribute that makes a struct or an impl a wasm class.
fn class_attr(key: &str, name: &str) -> TokenStream {
    let key = format_ident!("{}", key);
    let private = private();
    quote! {
        #[#private::wasm_bindgen::prelude::wasm_bindgen(
            wasm_bindgen = #private::wasm_bindgen,
            #key = #name
        )]
    }
}

/// The wasm items of one client: its class, its constructor and build info,
/// and the methods and handle classes of its exports.
pub(crate) fn expand(
    service: &str,
    client: &Client,
    self_ty: &Type,
    functions: &[&ImplItemFn],
) -> syn::Result<TokenStream> {
    let new_js = export_name(service, &client.ty, "new").map_err(|error| named(&error))?;
    let class = new_js.trim_end_matches("__new").to_owned();
    let class_ident = format_ident!("ApiBonesJs{}", client.ty);
    let mut all = BTreeSet::new();
    all.extend(gate_features(&client.surface, "")[..2].iter().cloned());
    let mut methods = Vec::new();
    for (export, function) in client.exports.iter().zip(functions) {
        all.extend(gate_features(&export.surface, &export.method));
        let shell = Shell {
            service,
            client,
            class_ident: &class_ident,
            class: &class,
            export,
            function,
        };
        methods.push(shell.expand()?);
    }
    let class_gate = gate(&all);
    let struct_attr = class_attr("js_name", &class);
    let impl_attr = class_attr("js_class", &class);
    let private = private();
    Ok(quote! {
        #class_gate
        #struct_attr
        pub struct #class_ident {
            inner: ::std::sync::Arc<#self_ty>,
        }

        #class_gate
        #impl_attr
        impl #class_ident {
            #[#private::wasm_bindgen::prelude::wasm_bindgen(js_name = #new_js)]
            pub fn new_js(base_url: ::std::string::String, host: #private::wasm_bindgen::JsValue) -> Self {
                Self { inner: ::std::sync::Arc::new(<#self_ty>::from_host(base_url, host)) }
            }

            #[#private::wasm_bindgen::prelude::wasm_bindgen(js_name = "__api_bones_sdk_build_info")]
            pub fn build_info_js() -> ::std::string::String {
                #private::serde_json::json!({
                    "name": ::core::env!("CARGO_PKG_NAME"),
                    "version": ::core::env!("CARGO_PKG_VERSION"),
                })
                .to_string()
            }
        }

        #(#methods)*
    })
}

/// What the expansion of one export needs to know.
struct Shell<'a> {
    service: &'a str,
    client: &'a Client,
    class_ident: &'a syn::Ident,
    class: &'a str,
    export: &'a Export,
    function: &'a ImplItemFn,
}

impl Shell<'_> {
    fn expand(&self) -> syn::Result<TokenStream> {
        let js = export_name(self.service, &self.client.ty, &self.export.method)
            .map_err(|error| named(&error))?;
        let features: BTreeSet<String> = gate_features(&self.export.surface, &self.export.method)
            .into_iter()
            .collect();
        let gate = gate(&features);
        let private = private();
        let method = &self.function.sig.ident;
        let shell = format_ident!("{}_js", method);
        let class_ident = self.class_ident;
        let impl_attr = class_attr("js_class", self.class);
        let parameters: Vec<(&syn::Ident, &Type)> = self
            .function
            .sig
            .inputs
            .iter()
            .filter_map(|input| match input {
                FnArg::Typed(typed) => match &*typed.pat {
                    Pat::Ident(ident) => Some((&ident.ident, &*typed.ty)),
                    _ => None,
                },
                FnArg::Receiver(_) => None,
            })
            .collect();
        let names: Vec<_> = parameters.iter().map(|(name, _)| *name).collect();
        let types: Vec<_> = parameters.iter().map(|(_, ty)| *ty).collect();
        let keys: Vec<_> = names
            .iter()
            .map(|name| json_name(&name.to_string()))
            .collect();
        let reads = quote! {
            let __args = ::api_bones_sdk::parse_args(&__args_json)?;
            #( let #names: #types = ::api_bones_sdk::field(&__args, #keys)?; )*
        };
        let call = quote!(__client.#method(#(#names),*));
        let wasm_bindgen = quote!(#private::wasm_bindgen::prelude::wasm_bindgen);
        let js_value = quote!(#private::wasm_bindgen::JsValue);
        let promise = quote!(#private::js_sys::Promise);
        let signal = quote!(#private::web_sys::AbortSignal);
        if self.export.sync {
            let outcome = self.sync_outcome(&call);
            return Ok(quote! {
                #gate
                #impl_attr
                impl #class_ident {
                    #[#wasm_bindgen(js_name = #js)]
                    pub fn #shell(
                        &self,
                        __args_json: ::std::string::String,
                    ) -> ::core::result::Result<::std::string::String, #js_value> {
                        let __client = &self.inner;
                        (|| -> ::core::result::Result<::std::string::String, ::api_bones_sdk::Error> {
                            #reads
                            #outcome
                        })()
                        .map_err(|__error| #js_value::from_str(&__error.to_json()))
                    }
                }
            });
        }
        let (handle_items, outcome) = self.async_outcome(&call, &js)?;
        let keep_signal = if matches!(self.export.ret, Return::Handle { .. }) {
            quote!(let __abort = __signal.clone();)
        } else {
            TokenStream::new()
        };
        Ok(quote! {
            #gate
            #impl_attr
            impl #class_ident {
                #[#wasm_bindgen(js_name = #js)]
                pub fn #shell(
                    &self,
                    __args_json: ::std::string::String,
                    __traceparent: ::core::option::Option<::std::string::String>,
                    __tracestate: ::core::option::Option<::std::string::String>,
                    __signal: ::core::option::Option<#signal>,
                ) -> #promise {
                    let __client = self.inner.clone();
                    #keep_signal
                    #private::promise(
                        __signal,
                        ::api_bones_sdk::CallCtx::scope(
                            ::api_bones_sdk::CallCtx {
                                traceparent: __traceparent,
                                tracestate: __tracestate,
                            },
                            async move {
                                #reads
                                #outcome
                            },
                        ),
                    )
                }
            }

            #handle_items
        })
    }

    /// The JSON text of a successful result, as an expression.
    fn encoded(has_value: bool) -> TokenStream {
        if has_value {
            quote!(::api_bones_sdk::to_json_string(&__out)?)
        } else {
            quote!({
                let () = __out;
                ::std::string::ToString::to_string("null")
            })
        }
    }

    fn sync_outcome(&self, call: &TokenStream) -> TokenStream {
        match &self.export.ret {
            Return::Unit => quote! {
                #call;
                ::core::result::Result::Ok(::std::string::ToString::to_string("null"))
            },
            Return::Result { ok, .. } => {
                let encoded = Self::encoded(ok.is_some());
                quote! {
                    let __out = #call.map_err(::core::convert::Into::<::api_bones_sdk::Error>::into)?;
                    ::core::result::Result::Ok(#encoded)
                }
            }
            Return::Value(_) | Return::Handle { .. } => {
                let encoded = Self::encoded(true);
                quote! {
                    let __out = #call;
                    ::core::result::Result::Ok(#encoded)
                }
            }
        }
    }

    /// The handle class an export returns, if any, and the body of its async
    /// block.
    fn async_outcome(
        &self,
        call: &TokenStream,
        js: &str,
    ) -> syn::Result<(TokenStream, TokenStream)> {
        let private = private();
        let js_value = quote!(#private::wasm_bindgen::JsValue);
        match &self.export.ret {
            Return::Handle { kind, .. } => {
                let output = match &self.function.sig.output {
                    syn::ReturnType::Type(_, ty) => head(ty).1.into_iter().next(),
                    syn::ReturnType::Default => None,
                };
                let item = output.ok_or_else(|| named("a handle yields one type"))?;
                let handle = self.handle_class(*kind, &item, js);
                let handle_ident = self.handle_ident();
                let outcome = quote! {
                    let __handle = ::std::sync::Arc::new(#call.await);
                    let __cancel = __handle.clone();
                    #private::on_abort(&__abort, move || __cancel.cancel());
                    ::core::result::Result::Ok(#js_value::from(#handle_ident { inner: __handle }))
                };
                Ok((handle, outcome))
            }
            Return::Result { ok, .. } => {
                let encoded = Self::encoded(ok.is_some());
                Ok((
                    TokenStream::new(),
                    quote! {
                        let __out = #call
                            .await
                            .map_err(::core::convert::Into::<::api_bones_sdk::Error>::into)?;
                        ::core::result::Result::Ok(#js_value::from_str(&#encoded))
                    },
                ))
            }
            Return::Unit | Return::Value(_) => {
                Err(named("an async export returns Result<T, E> or a handle"))
            }
        }
    }

    fn handle_ident(&self) -> syn::Ident {
        format_ident!(
            "ApiBonesJs{}{}Handle",
            self.client.ty,
            upper_camel(&self.export.method)
        )
    }

    /// The class a handle export resolves to: `next()`, `cancel()`, `span()`.
    fn handle_class(&self, kind: HandleKind, item: &Type, js: &str) -> TokenStream {
        let private = private();
        let js_value = quote!(#private::wasm_bindgen::JsValue);
        let wasm_bindgen = quote!(#private::wasm_bindgen::prelude::wasm_bindgen);
        let ident = self.handle_ident();
        let class = format!("{js}__Handle");
        let struct_attr = class_attr("js_name", &class);
        let impl_attr = class_attr("js_class", &class);
        let kind_ident = format_ident!("{}", kind.type_name());
        let gate = gate(
            &gate_features(&self.export.surface, &self.export.method)
                .into_iter()
                .collect(),
        );
        let json = quote!(#private::serde_json);
        let next = match kind {
            HandleKind::List => quote! {
                let mut __items = ::std::vec::Vec::new();
                for __item in __page.items.iter() {
                    __items.push(::api_bones_sdk::Wire::encode(__item)?);
                }
                let mut __json = #json::Map::new();
                __json.insert(::std::string::ToString::to_string("items"), #json::Value::Array(__items));
                __json.insert(::std::string::ToString::to_string("revision"), #json::Value::String(__page.revision.clone()));
                __json.insert(::std::string::ToString::to_string("fromCache"), #json::Value::Bool(__page.from_cache));
                __json.insert(::std::string::ToString::to_string("hasNext"), #json::Value::Bool(__page.has_next));
                if let ::core::option::Option::Some(__total) = __page.total {
                    __json.insert(
                        ::std::string::ToString::to_string("total"),
                        #json::json!({
                            "value": __total.value.to_string(),
                            "kind": ::std::format!("{:?}", __total.kind).to_lowercase(),
                        }),
                    );
                }
                #json::Value::Object(__json).to_string()
            },
            HandleKind::Stream => quote! {
                ::api_bones_sdk::to_json_string(&__page)?
            },
            HandleKind::Batch => quote! {
                let mut __chunk = ::std::vec::Vec::new();
                for __result in __page {
                    __chunk.push(match __result {
                        ::core::result::Result::Ok(__value) => {
                            #json::json!({ "ok": ::api_bones_sdk::Wire::encode(&__value)? })
                        }
                        ::core::result::Result::Err(__error) => {
                            #json::json!({ "error": __error.to_value() })
                        }
                    });
                }
                #json::Value::Array(__chunk).to_string()
            },
        };
        quote! {
            #gate
            #struct_attr
            pub struct #ident {
                inner: ::std::sync::Arc<::api_bones_sdk::#kind_ident<#item>>,
            }

            #gate
            #impl_attr
            impl #ident {
                #[#wasm_bindgen(js_name = "next")]
                pub fn next_js(&self) -> #private::js_sys::Promise {
                    let __handle = self.inner.clone();
                    #private::promise(
                        ::core::option::Option::None,
                        async move {
                            match __handle.next().await? {
                                ::core::option::Option::None => {
                                    ::core::result::Result::Ok(#js_value::UNDEFINED)
                                }
                                ::core::option::Option::Some(__page) => {
                                    let __text = { #next };
                                    ::core::result::Result::Ok(#js_value::from_str(&__text))
                                }
                            }
                        },
                    )
                }

                #[#wasm_bindgen(js_name = "cancel")]
                pub fn cancel_js(&self) {
                    self.inner.cancel();
                }

                #[#wasm_bindgen(js_name = "span")]
                pub fn span_js(&self) -> ::core::option::Option<::std::string::String> {
                    self.inner.span()
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use api_bones_sdk_model::Model;

    use super::*;
    use crate::common::flat;

    const SOURCE: &str = r#"
        #[record]
        pub struct Number { pub value: i64 }

        #[client(surface = "connect-pagination")]
        impl NumbersClient {
            #[export]
            pub async fn list(&self, limit: u32, since: Option<SystemTime>) -> ListCall<Number> { body() }
            #[export]
            pub async fn watch(&self) -> Stream<Number> { body() }
            #[export]
            pub async fn get_all(&self, ids: Vec<String>) -> Batch<Number> { body() }
            #[export(surface = "connect-batch")]
            pub async fn get(&self, id: String) -> Result<Number, Error> { body() }
            #[export]
            pub async fn clear(&self) -> Result<(), Error> { body() }
            #[export(sync)]
            pub fn label(&self, n: Number) -> String { body() }
            #[export(sync)]
            pub fn reset(&self) { body() }
            #[export(sync)]
            pub fn check(&self) -> Result<(), Error> { body() }
            #[export(sync)]
            pub fn parse(&self, text: String) -> Result<Number, Error> { body() }
        }
    "#;

    fn built(service: &str) -> syn::Result<TokenStream> {
        let file: syn::File = syn::parse_str(SOURCE).expect("a file");
        let model = Model::from_files([&file]).expect("a model");
        let item = file
            .items
            .iter()
            .find_map(|item| match item {
                syn::Item::Impl(item) => Some(item),
                _ => None,
            })
            .expect("an impl");
        let functions: Vec<&ImplItemFn> = item
            .items
            .iter()
            .filter_map(|member| match member {
                syn::ImplItem::Fn(function) => Some(function),
                _ => None,
            })
            .collect();
        expand(service, &model.clients[0], &item.self_ty, &functions)
    }

    fn text() -> String {
        flat(&built("api-bones").expect("expanded"))
    }

    #[test]
    fn the_expansion_is_valid_rust() {
        syn::parse2::<syn::File>(built("api-bones").expect("expanded")).expect("a file");
    }

    #[test]
    fn a_client_is_a_class_named_by_service_and_type() {
        let text = text();
        assert!(
            text.contains("js_name=\"api_bones__NumbersClient\""),
            "{text}"
        );
        assert!(
            text.contains("js_class=\"api_bones__NumbersClient\""),
            "{text}"
        );
        assert!(
            text.contains(
                "pubstructApiBonesJsNumbersClient{inner:::std::sync::Arc<NumbersClient>,}"
            ),
            "{text}"
        );
        assert!(
            text.contains("js_name=\"api_bones__NumbersClient__new\""),
            "{text}"
        );
        assert!(
            text.contains("<NumbersClient>::from_host(base_url,host)"),
            "{text}"
        );
    }

    #[test]
    fn the_class_reports_its_build_info_from_the_package() {
        let text = text();
        assert!(
            text.contains("js_name=\"__api_bones_sdk_build_info\""),
            "{text}"
        );
        assert!(
            text.contains("\"name\":::core::env!(\"CARGO_PKG_NAME\")"),
            "{text}"
        );
        assert!(
            text.contains("\"version\":::core::env!(\"CARGO_PKG_VERSION\")"),
            "{text}"
        );
    }

    #[test]
    fn every_export_is_gated_by_its_three_features_on_wasm32() {
        let text = text();
        assert!(
            text.contains("#[cfg(all(target_arch=\"wasm32\",any(feature=\"sdk-all\",feature=\"sdk-connect-pagination\",feature=\"sdk-m-list\")))]"),
            "{text}"
        );
        assert!(
            text.contains("feature=\"sdk-connect-batch\",feature=\"sdk-m-get\""),
            "{text}"
        );
    }

    #[test]
    fn the_class_is_gated_by_every_feature_of_its_exports() {
        let text = text();
        assert!(text.contains("feature=\"sdk-all\",feature=\"sdk-connect-batch\",feature=\"sdk-connect-pagination\",feature=\"sdk-m-check\""), "{text}");
    }

    #[test]
    fn an_async_export_runs_under_the_callers_trace_context_and_abort_signal() {
        let text = text();
        assert!(
            text.contains("js_name=\"api_bones__NumbersClient__get\""),
            "{text}"
        );
        assert!(text.contains("pubfnget_js(&self,__args_json:::std::string::String,__traceparent:::core::option::Option<::std::string::String>,__tracestate:::core::option::Option<::std::string::String>,__signal:::core::option::Option<::api_bones_sdk::__private::web_sys::AbortSignal>,)->::api_bones_sdk::__private::js_sys::Promise"), "{text}");
        assert!(text.contains("::api_bones_sdk::__private::promise(__signal,::api_bones_sdk::CallCtx::scope(::api_bones_sdk::CallCtx{traceparent:__traceparent,tracestate:__tracestate,}"), "{text}");
    }

    #[test]
    fn arguments_are_read_by_camel_case_name_in_declaration_order() {
        let text = text();
        assert!(text.contains("let__args=::api_bones_sdk::parse_args(&__args_json)?;letlimit:u32=::api_bones_sdk::field(&__args,\"limit\")?;letsince:Option<SystemTime>=::api_bones_sdk::field(&__args,\"since\")?;"), "{text}");
        assert!(text.contains("__client.list(limit,since)"), "{text}");
    }

    #[test]
    fn a_result_export_resolves_to_the_json_of_its_value_or_rejects_with_the_error() {
        let text = text();
        assert!(text.contains("let__out=__client.get(id).await.map_err(::core::convert::Into::<::api_bones_sdk::Error>::into)?;::core::result::Result::Ok(::api_bones_sdk::__private::wasm_bindgen::JsValue::from_str(&::api_bones_sdk::to_json_string(&__out)?))"), "{text}");
    }

    #[test]
    fn a_unit_result_resolves_to_null() {
        let text = text();
        assert!(
            text.contains("let()=__out;::std::string::ToString::to_string(\"null\")"),
            "{text}"
        );
    }

    #[test]
    fn a_handle_export_resolves_to_a_class_that_an_abort_cancels() {
        let text = text();
        assert!(text.contains("let__handle=::std::sync::Arc::new(__client.list(limit,since).await);let__cancel=__handle.clone();::api_bones_sdk::__private::on_abort(&__abort,move||__cancel.cancel());"), "{text}");
        assert!(
            text.contains("JsValue::from(ApiBonesJsNumbersClientListHandle{inner:__handle})"),
            "{text}"
        );
        assert!(
            text.contains("js_name=\"api_bones__NumbersClient__list__Handle\""),
            "{text}"
        );
        assert!(
            text.contains("inner:::std::sync::Arc<::api_bones_sdk::ListCall<Number>>"),
            "{text}"
        );
        assert!(
            text.contains("inner:::std::sync::Arc<::api_bones_sdk::Stream<Number>>"),
            "{text}"
        );
        assert!(
            text.contains("inner:::std::sync::Arc<::api_bones_sdk::Batch<Number>>"),
            "{text}"
        );
    }

    #[test]
    fn a_handle_class_has_next_cancel_and_span() {
        let text = text();
        assert!(text.contains("js_name=\"next\""), "{text}");
        assert!(text.contains("js_name=\"cancel\""), "{text}");
        assert!(text.contains("js_name=\"span\""), "{text}");
        assert!(text.contains("::core::option::Option::None=>{::core::result::Result::Ok(::api_bones_sdk::__private::wasm_bindgen::JsValue::UNDEFINED)}"), "{text}");
        assert!(text.contains("self.inner.cancel();"), "{text}");
        assert!(text.contains("self.inner.span()"), "{text}");
    }

    #[test]
    fn a_list_page_carries_items_revision_total_cache_and_next_flags() {
        let text = text();
        for key in [
            "\"items\"",
            "\"revision\"",
            "\"fromCache\"",
            "\"hasNext\"",
            "\"total\"",
        ] {
            assert!(
                text.contains(&format!("ToString::to_string({key})")),
                "{key} {text}"
            );
        }
        assert!(
            text.contains("\"value\":__total.value.to_string()"),
            "{text}"
        );
        assert!(
            text.contains("\"kind\":::std::format!(\"{:?}\",__total.kind).to_lowercase()"),
            "{text}"
        );
    }

    #[test]
    fn a_batch_chunk_is_ok_or_error_in_request_order() {
        let text = text();
        assert!(
            text.contains("\"ok\":::api_bones_sdk::Wire::encode(&__value)?"),
            "{text}"
        );
        assert!(text.contains("\"error\":__error.to_value()"), "{text}");
    }

    #[test]
    fn a_stream_item_is_its_json() {
        let text = text();
        assert!(
            text.contains("let__text={::api_bones_sdk::to_json_string(&__page)?};"),
            "{text}"
        );
    }

    #[test]
    fn a_sync_export_returns_the_json_or_throws_the_error_json() {
        let text = text();
        assert!(
            text.contains("js_name=\"api_bones__NumbersClient__label\""),
            "{text}"
        );
        assert!(text.contains("pubfnlabel_js(&self,__args_json:::std::string::String,)->::core::result::Result<::std::string::String,::api_bones_sdk::__private::wasm_bindgen::JsValue>"), "{text}");
        assert!(text.contains(".map_err(|__error|::api_bones_sdk::__private::wasm_bindgen::JsValue::from_str(&__error.to_json()))"), "{text}");
        assert!(text.contains("let__out=__client.label(n);::core::result::Result::Ok(::api_bones_sdk::to_json_string(&__out)?)"), "{text}");
        assert!(text.contains("__client.reset();::core::result::Result::Ok(::std::string::ToString::to_string(\"null\"))"), "{text}");
        assert!(text.contains("let__out=__client.check().map_err(::core::convert::Into::<::api_bones_sdk::Error>::into)?;::core::result::Result::Ok({let()=__out;"), "{text}");
        assert!(
            text.contains("let__out=__client.parse(text).map_err("),
            "{text}"
        );
    }

    #[test]
    fn a_part_of_a_name_with_a_double_underscore_is_refused() {
        let error = built("a__b").expect_err("refused");
        assert!(error.to_string().contains("`__`"), "{error}");
    }

    #[test]
    fn the_wasm_names_are_refused_when_a_method_has_a_double_underscore() {
        let file: syn::File = syn::parse_str(
            "#[client(surface = \"s\")] impl C { #[export(sync)] fn a__b(&self) {} }",
        )
        .expect("a file");
        let model = Model::from_files([&file]).expect("a model");
        let syn::Item::Impl(item) = &file.items[0] else {
            return;
        };
        let functions: Vec<&ImplItemFn> = item
            .items
            .iter()
            .filter_map(|member| match member {
                syn::ImplItem::Fn(function) => Some(function),
                _ => None,
            })
            .collect();
        let error =
            expand("svc", &model.clients[0], &item.self_ty, &functions).expect_err("refused");
        assert!(error.to_string().contains("method"), "{error}");
    }

    #[test]
    fn a_client_without_exports_is_gated_by_its_surface_alone() {
        let file: syn::File =
            syn::parse_str("#[client(surface = \"s\")] impl C {}").expect("a file");
        let model = Model::from_files([&file]).expect("a model");
        let syn::Item::Impl(item) = &file.items[0] else {
            return;
        };
        let text = flat(&expand("svc", &model.clients[0], &item.self_ty, &[]).expect("expanded"));
        assert!(
            text.contains("any(feature=\"sdk-all\",feature=\"sdk-s\")"),
            "{text}"
        );
        assert!(text.contains("js_name=\"svc__C__new\""), "{text}");
    }

    #[test]
    fn the_shape_of_an_export_the_model_would_refuse_is_refused_here_too() {
        let model = Model::default();
        let file: syn::File =
            syn::parse_str("impl C { fn f(&self) -> u32 { 1 } }").expect("a file");
        let syn::Item::Impl(item) = &file.items[0] else {
            return;
        };
        let function = item
            .items
            .iter()
            .find_map(|member| match member {
                syn::ImplItem::Fn(function) => Some(function),
                _ => None,
            })
            .expect("a fn");
        let export = Export {
            method: "f".to_owned(),
            sync: false,
            surface: "s".to_owned(),
            params: Vec::new(),
            ret: Return::Value(api_bones_sdk_model::WireType::U32),
        };
        let client = Client {
            ty: "C".to_owned(),
            surface: "s".to_owned(),
            exports: vec![export],
        };
        assert_eq!(model.clients.len(), 0);
        let error = expand("svc", &client, &item.self_ty, &[function]).expect_err("refused");
        assert!(
            error.to_string().contains("async export returns"),
            "{error}"
        );
        let handle = Export {
            ret: Return::Handle {
                kind: HandleKind::List,
                item: "X".to_owned(),
            },
            ..client.exports[0].clone()
        };
        let client = Client {
            exports: vec![handle],
            ..client
        };
        let error = expand("svc", &client, &item.self_ty, &[function]).expect_err("refused");
        assert!(error.to_string().contains("yields one type"), "{error}");
    }
}
