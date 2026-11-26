// SPDX-License-Identifier: MIT
//! The wasm32 exports of a `#[client]` impl.
//!
//! Each client becomes a wasm class named `<svc>__<Type>`. Its constructor
//! export is `<svc>__<Type>__new(base_url, host, options_json)`, built from the
//! type's `from_host(base_url: String, host: JsValue, ..) -> Result<Self, Error>`.
//! A `from_host` inside the `#[client]` impl may take further parameters, each an
//! `Option` of a wire type; they are the client's options, read from the
//! `options_json` object by camelCase name, and a member that is not one of
//! them is refused. The constructor throws the error JSON when that fails.
//! Each export is a
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
//! - An export that returns a handle returns, at once, a class with `next()`,
//!   `cancel()` and `span()`; the call that opens the handle runs on the first
//!   `next()`, an abort of `signal` cancels the handle, and `span()` is
//!   undefined until the handle opened.
//! - A sync export takes `args_json` and returns the JSON string or throws the
//!   error JSON.
//!
//! Everything the code touches outside the crate is reached through
//! `api_bones_sdk::__private`.

use std::collections::BTreeSet;

use api_bones_sdk_model::{
    Client, Export, HandleKind, Param, Return, export_name, gate_features, json_name,
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

/// The attribute of a method inside a class impl.
///
/// wasm-bindgen's impl expansion takes over a method's own attributes only
/// when their path starts with `wasm_bindgen` (it matches the first segment),
/// and it strips them, so this name is never resolved and needs no import in
/// the consumer. A full path such as `::api_bones_sdk::__private::...` would
/// be left on the method and expand on its own as a free function: `self`
/// outside an impl, an unnamed `const` inside one, and the default
/// `wasm_bindgen` crate path the consumer does not depend on.
fn method_attr() -> TokenStream {
    quote!(wasm_bindgen)
}

/// The attribute that makes a struct a wasm class. The struct expansion is a
/// derive whose path is the option's value, so the option is needed here.
fn struct_attr(name: &str) -> TokenStream {
    let private = private();
    quote! {
        #[#private::wasm_bindgen::prelude::wasm_bindgen(
            wasm_bindgen = #private::wasm_bindgen,
            js_name = #name
        )]
    }
}

/// The attribute that makes an impl a wasm class impl. The impl expansion does
/// not read a `wasm_bindgen = <path>` option, so none is given: the plain
/// `wasm_bindgen::` paths it emits resolve through the `use` of the block the
/// client sits in.
fn impl_attr(name: &str) -> TokenStream {
    let private = private();
    quote!(#[#private::wasm_bindgen::prelude::wasm_bindgen(js_class = #name)])
}

/// What a handle's `next()` resolves to for an item: a list page is its JSON
/// and a hold on its items, which keeps them alive for as long as the host holds
/// the page; a stream item and a batch chunk are the JSON alone.
fn delivery(kind: HandleKind) -> TokenStream {
    let private = private();
    let js_value = quote!(#private::wasm_bindgen::JsValue);
    match kind {
        HandleKind::List => quote! {
            #js_value::from(#private::js_sys::Array::of2(
                &#js_value::from_str(&__text),
                &#js_value::from(#private::PageHold::new(&__page)),
            ))
        },
        HandleKind::Stream | HandleKind::Batch => quote!(#js_value::from_str(&__text)),
    }
}

/// The wasm items of one client: its class, its constructor and build info,
/// and the methods and handle classes of its exports.
///
/// They sit in one anonymous block that imports the `wasm_bindgen` the SDK
/// re-exports, so the plain `wasm_bindgen::` paths the wasm-bindgen expansions
/// emit resolve in a consumer without a direct dependency. A block, unlike a
/// module, leaves `self::` and `super::` meaning what they mean at the call
/// site, and one block per client keeps two clients of a module apart.
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
    let struct_attr = struct_attr(&class);
    let impl_attr = impl_attr(&class);
    let private = private();
    let method = method_attr();
    let option_keys: Vec<String> = client.options.iter().map(Param::json_name).collect();
    Ok(quote! {
        #class_gate
        const _: () = {
        use #private::wasm_bindgen;

        #struct_attr
        pub struct #class_ident {
            inner: ::std::sync::Arc<#self_ty>,
        }

        #impl_attr
        impl #class_ident {
            #[#method(js_name = #new_js)]
            pub fn new_js(
                base_url: ::std::string::String,
                host: #private::wasm_bindgen::JsValue,
                options_json: ::std::string::String,
            ) -> ::core::result::Result<Self, #private::wasm_bindgen::JsValue> {
                (|| -> ::core::result::Result<#self_ty, ::api_bones_sdk::Error> {
                    let __options = ::api_bones_sdk::parse_options(
                        &options_json,
                        &[#(#option_keys),*],
                    )?;
                    <#self_ty>::from_host(
                        base_url,
                        host
                        #(, ::api_bones_sdk::field(&__options, #option_keys)?)*
                    )
                    .map_err(::core::convert::Into::<::api_bones_sdk::Error>::into)
                })()
                .map(|__client| Self { inner: ::std::sync::Arc::new(__client) })
                .map_err(|__error| #private::wasm_bindgen::JsValue::from_str(&__error.to_json()))
            }

            #[#method(js_name = "__api_bones_sdk_build_info")]
            pub fn build_info_js() -> ::std::string::String {
                #private::serde_json::json!({
                    "name": ::core::env!("CARGO_PKG_NAME"),
                    "version": ::core::env!("CARGO_PKG_VERSION"),
                })
                .to_string()
            }
        }

        #(#methods)*
        };
    })
}

/// The call of an async export, run under the caller's trace fields: `reads`
/// parse the arguments, then `body` runs.
fn scoped(reads: &TokenStream, body: &TokenStream) -> TokenStream {
    quote! {
        ::api_bones_sdk::CallCtx::scope(
            ::api_bones_sdk::CallCtx {
                traceparent: __traceparent,
                tracestate: __tracestate,
            },
            async move {
                #reads
                #body
            },
        )
    }
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
        let impl_attr = impl_attr(self.class);
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
        let wasm_bindgen = method_attr();
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
        let params = quote! {
            &self,
            __args_json: ::std::string::String,
            __traceparent: ::core::option::Option<::std::string::String>,
            __tracestate: ::core::option::Option<::std::string::String>,
            __signal: ::core::option::Option<#signal>,
        };
        if let Return::Handle { kind, .. } = &self.export.ret {
            let item = self.handle_item()?;
            let handle_items = self.handle_class(*kind, &item, &js);
            let handle_ident = self.handle_ident();
            let scoped = scoped(&reads, &quote!(::core::result::Result::Ok(#call.await)));
            return Ok(quote! {
                #gate
                #impl_attr
                impl #class_ident {
                    #[#wasm_bindgen(js_name = #js)]
                    pub fn #shell(#params) -> #handle_ident {
                        let __client = self.inner.clone();
                        let __lazy = #private::Lazy::new(#scoped);
                        let __cancel = __lazy.clone();
                        #private::on_abort(&__signal, move || __cancel.cancel());
                        #handle_ident { inner: __lazy }
                    }
                }

                #handle_items
            });
        }
        let scoped = scoped(&reads, &self.async_outcome(&call)?);
        Ok(quote! {
            #gate
            #impl_attr
            impl #class_ident {
                #[#wasm_bindgen(js_name = #js)]
                pub fn #shell(#params) -> #promise {
                    let __client = self.inner.clone();
                    #private::promise(__signal, #scoped)
                }
            }
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

    /// The item type a handle export yields.
    fn handle_item(&self) -> syn::Result<Type> {
        let output = match &self.function.sig.output {
            syn::ReturnType::Type(_, ty) => head(ty).1.into_iter().next(),
            syn::ReturnType::Default => None,
        };
        output.ok_or_else(|| named("a handle yields one type"))
    }

    /// The body of the async block of a promise export.
    fn async_outcome(&self, call: &TokenStream) -> syn::Result<TokenStream> {
        let private = private();
        let js_value = quote!(#private::wasm_bindgen::JsValue);
        match &self.export.ret {
            Return::Result { ok, .. } => {
                let encoded = Self::encoded(ok.is_some());
                Ok(quote! {
                    let __out = #call
                        .await
                        .map_err(::core::convert::Into::<::api_bones_sdk::Error>::into)?;
                    ::core::result::Result::Ok(#js_value::from_str(&#encoded))
                })
            }
            Return::Handle { .. } | Return::Unit | Return::Value(_) => {
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

    /// The class a handle export returns: `next()`, `cancel()`, `span()`.
    fn handle_class(&self, kind: HandleKind, item: &Type, js: &str) -> TokenStream {
        let private = private();
        let js_value = quote!(#private::wasm_bindgen::JsValue);
        let wasm_bindgen = method_attr();
        let ident = self.handle_ident();
        let class = format!("{js}__Handle");
        let struct_attr = struct_attr(&class);
        let impl_attr = impl_attr(&class);
        let kind_ident = format_ident!("{}", kind.type_name());
        let gate = gate(
            &gate_features(&self.export.surface, &self.export.method)
                .into_iter()
                .collect(),
        );
        let json = quote!(#private::serde_json);
        let delivered = delivery(kind);
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
                inner: #private::Lazy<::api_bones_sdk::#kind_ident<#item>>,
            }

            #gate
            #impl_attr
            impl #ident {
                #[#wasm_bindgen(js_name = "next")]
                pub fn next_js(&self) -> #private::js_sys::Promise {
                    let __lazy = self.inner.clone();
                    #private::promise(
                        ::core::option::Option::None,
                        async move {
                            let ::core::option::Option::Some(__handle) = __lazy.open().await? else {
                                return ::core::result::Result::Ok(#js_value::UNDEFINED);
                            };
                            match __handle.next().await? {
                                ::core::option::Option::None => {
                                    ::core::result::Result::Ok(#js_value::UNDEFINED)
                                }
                                ::core::option::Option::Some(__page) => {
                                    let __text = { #next };
                                    ::core::result::Result::Ok(#delivered)
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
    use quote::ToTokens;

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
        built_from(SOURCE, service)
    }

    fn built_from(source: &str, service: &str) -> syn::Result<TokenStream> {
        let file: syn::File = syn::parse_str(source).expect("a file");
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
                syn::ImplItem::Fn(function)
                    if api_bones_sdk_model::attrs::find(&function.attrs, "export").is_some() =>
                {
                    Some(function)
                }
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

    /// Takes the attribute named `name` off `attrs`, as the compiler does
    /// before it hands an item to an attribute macro, and returns its
    /// arguments.
    fn take_wasm_bindgen(attrs: &mut Vec<syn::Attribute>, name: &str) -> TokenStream {
        let at = attrs
            .iter()
            .position(|attr| {
                attr.path()
                    .segments
                    .last()
                    .is_some_and(|segment| segment.ident == name)
            })
            .expect("the attribute");
        attrs
            .remove(at)
            .meta
            .require_list()
            .expect("a list")
            .tokens
            .clone()
    }

    /// Whether `text` names `wasm_bindgen` with a path of its own, the way the
    /// wasm-bindgen expansions do when they emit their default path.
    fn has_plain_path(text: &str) -> bool {
        text.match_indices("wasm_bindgen::").any(|(at, _)| {
            at == 0
                || !matches!(
                    text.as_bytes()[at - 1],
                    b':' | b'_' | b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9'
                )
        })
    }

    /// The struct of a client through the wasm-bindgen struct expansion.
    fn expand_struct(item: &syn::ItemStruct) -> String {
        let mut item = item.clone();
        let attr = take_wasm_bindgen(&mut item.attrs, "wasm_bindgen");
        item.attrs.clear();
        flat(
            &wasm_bindgen_macro_support::expand(attr, item.to_token_stream())
                .expect("a struct expansion"),
        )
    }

    /// The impl of a client through the wasm-bindgen impl expansion, then each
    /// of its methods through the class marker expansion the compiler runs on
    /// it. Returns the number of methods and whether their expansions emit a plain
    /// `wasm_bindgen::` path.
    fn expand_impl(item: &syn::ItemImpl) -> (usize, bool) {
        let mut item = item.clone();
        let attr = take_wasm_bindgen(&mut item.attrs, "wasm_bindgen");
        item.attrs.clear();
        let out = wasm_bindgen_macro_support::expand(attr, item.to_token_stream())
            .expect("an impl expansion");
        let text = flat(&out);
        assert!(
            !text.contains("letwasm_bindgen:()"),
            "an option the impl expansion ignores: {text}"
        );
        let file = syn::parse2::<syn::File>(out).expect("items");
        let [syn::Item::Impl(expanded), syn::Item::Const(section)] = file.items.as_slice() else {
            panic!("an impl and its custom section: {text}");
        };
        assert_eq!(section.ident, "_", "{text}");
        let mut methods = 0;
        let mut plain = false;
        for member in expanded.items.iter().cloned() {
            let syn::ImplItem::Fn(mut function) = member else {
                panic!("a method");
            };
            let marker = take_wasm_bindgen(&mut function.attrs, "__wasm_bindgen_class_marker");
            let out =
                wasm_bindgen_macro_support::expand_class_marker(marker, function.to_token_stream())
                    .expect("a method expansion");
            let text = flat(&out);
            let method = syn::parse2::<ImplItemFn>(out).expect("a method");
            assert_eq!(method.attrs.len(), 0, "{}: {text}", method.sig.ident);
            plain |= has_plain_path(&text);
            methods += 1;
        }
        (methods, plain)
    }

    /// The expansion of a client in a consumer with no wasm-bindgen
    /// dependency, run through the real wasm-bindgen expanders. It does not
    /// resolve names or type-check for wasm32; the `ts` CI lane does that.
    #[test]
    fn a_client_expands_through_wasm_bindgen_without_a_direct_dependency() {
        let file = syn::parse2::<syn::File>(built("api-bones").expect("expanded")).expect("a file");
        let [syn::Item::Const(block)] = file.items.as_slice() else {
            panic!("a client is one item: {} items", file.items.len());
        };
        assert_eq!(block.ident, "_");
        let syn::Expr::Block(block) = &*block.expr else {
            panic!("a client is a block");
        };
        let [syn::Stmt::Item(import), items @ ..] = block.block.stmts.as_slice() else {
            panic!("a client opens with its import");
        };
        assert_eq!(
            flat(&import.to_token_stream()),
            "use::api_bones_sdk::__private::wasm_bindgen;"
        );

        let mut structs = 0;
        let mut methods = 0;
        let mut plain = false;
        for stmt in items {
            let syn::Stmt::Item(item) = stmt else {
                panic!("a block of items");
            };
            match item {
                syn::Item::Struct(item) => {
                    let out = expand_struct(item);
                    assert!(
                        out.starts_with(
                            "#[derive(::api_bones_sdk::__private::wasm_bindgen::__rt::BindgenedStruct)]"
                        ),
                        "{out}"
                    );
                    structs += 1;
                }
                syn::Item::Impl(item) => {
                    let (count, emitted) = expand_impl(item);
                    methods += count;
                    plain |= emitted;
                }
                other => panic!("an item: {}", flat(&other.to_token_stream())),
            }
        }
        assert!(structs > 0, "no class struct expanded");
        assert!(methods > 0, "no class method expanded");
        assert!(plain, "the import is needed by no expansion");
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
            text.contains("pubfnnew_js(base_url:::std::string::String,host:::api_bones_sdk::__private::wasm_bindgen::JsValue,options_json:::std::string::String,)->::core::result::Result<Self,::api_bones_sdk::__private::wasm_bindgen::JsValue>"),
            "{text}"
        );
    }

    #[test]
    fn a_constructor_that_fails_throws_the_error_json_instead_of_panicking() {
        let text = text();
        assert!(text.contains("(||->::core::result::Result<NumbersClient,::api_bones_sdk::Error>{let__options=::api_bones_sdk::parse_options(&options_json,&[],)?;<NumbersClient>::from_host(base_url,host).map_err(::core::convert::Into::<::api_bones_sdk::Error>::into)})().map(|__client|Self{inner:::std::sync::Arc::new(__client)}).map_err(|__error|::api_bones_sdk::__private::wasm_bindgen::JsValue::from_str(&__error.to_json()))"), "{text}");
    }

    #[test]
    fn the_options_of_from_host_are_read_from_the_options_json_by_name() {
        let source = r#"
            #[enumeration]
            pub enum Transport { Fetch }

            #[client(surface = "s")]
            impl NumbersClient {
                pub fn from_host(
                    base_url: String,
                    host: JsValue,
                    default_page_size: Option<u32>,
                    transport: Option<Transport>,
                ) -> Result<Self, Error> { body() }
                #[export]
                pub async fn clear(&self) -> Result<(), Error> { body() }
            }
        "#;
        let text = flat(&built_from(source, "api-bones").expect("expanded"));
        assert!(
            text.contains("::api_bones_sdk::parse_options(&options_json,&[\"defaultPageSize\",\"transport\"],)?;"),
            "{text}"
        );
        assert!(
            text.contains("<NumbersClient>::from_host(base_url,host,::api_bones_sdk::field(&__options,\"defaultPageSize\")?,::api_bones_sdk::field(&__options,\"transport\")?)"),
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
    fn a_handle_export_returns_its_class_at_once_and_opens_on_the_first_read() {
        let text = text();
        assert!(text.contains("__signal:::core::option::Option<::api_bones_sdk::__private::web_sys::AbortSignal>,)->ApiBonesJsNumbersClientListHandle{"), "{text}");
        assert!(text.contains("let__lazy=::api_bones_sdk::__private::Lazy::new(::api_bones_sdk::CallCtx::scope(::api_bones_sdk::CallCtx{traceparent:__traceparent,tracestate:__tracestate,},asyncmove{let__args=::api_bones_sdk::parse_args(&__args_json)?;"), "{text}");
        assert!(
            text.contains("::core::result::Result::Ok(__client.list(limit,since).await)"),
            "{text}"
        );
        assert!(text.contains("let__cancel=__lazy.clone();::api_bones_sdk::__private::on_abort(&__signal,move||__cancel.cancel());ApiBonesJsNumbersClientListHandle{inner:__lazy}"), "{text}");
        assert!(
            text.contains("js_name=\"api_bones__NumbersClient__list__Handle\""),
            "{text}"
        );
        for kind in ["ListCall", "Stream", "Batch"] {
            assert!(
                text.contains(&format!(
                    "inner:::api_bones_sdk::__private::Lazy<::api_bones_sdk::{kind}<Number>>"
                )),
                "{kind} {text}"
            );
        }
    }

    #[test]
    fn a_handle_class_has_next_cancel_and_span() {
        let text = text();
        assert!(text.contains("js_name=\"next\""), "{text}");
        assert!(text.contains("js_name=\"cancel\""), "{text}");
        assert!(text.contains("js_name=\"span\""), "{text}");
        assert!(text.contains("let::core::option::Option::Some(__handle)=__lazy.open().await?else{return::core::result::Result::Ok(::api_bones_sdk::__private::wasm_bindgen::JsValue::UNDEFINED);};"), "{text}");
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

    /// A list page reaches the host as its JSON and a hold on its items; a
    /// stream item and a batch chunk are the JSON alone.
    #[test]
    fn a_list_page_is_delivered_with_a_hold_on_its_items_and_no_other_handle_is() {
        let text = text();
        let hold = "::api_bones_sdk::__private::wasm_bindgen::JsValue::from(::api_bones_sdk::__private::js_sys::Array::of2(&::api_bones_sdk::__private::wasm_bindgen::JsValue::from_str(&__text),&::api_bones_sdk::__private::wasm_bindgen::JsValue::from(::api_bones_sdk::__private::PageHold::new(&__page)),))";
        assert_eq!(text.matches(hold).count(), 1, "{text}");
        assert_eq!(text.matches("PageHold").count(), 1, "{text}");
        let bare = "let__text={::api_bones_sdk::to_json_string(&__page)?};::core::result::Result::Ok(::api_bones_sdk::__private::wasm_bindgen::JsValue::from_str(&__text))";
        assert_eq!(text.matches(bare).count(), 1, "{text}");
        assert_eq!(text.matches("Array::of2").count(), 1, "{text}");
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
            options: Vec::new(),
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
