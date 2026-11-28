// SPDX-License-Identifier: MIT
//! `#[client]`: the impl whose `#[export]` methods are the Layer 2 surface.

use api_bones_sdk_model::{Client, Model, Return};
use proc_macro2::{TokenStream, TokenTree};
use quote::{ToTokens, quote};
use syn::{ImplItem, Item, parse_quote};

use crate::common::{finish, head, without};
use crate::wasm::{Home, Member};
use crate::{service, uniffi, wasm};

/// The impl or module, kept as written but for the `#[export]` attributes, the
/// compile-time check that every error is an `Into<api_bones_sdk::Error>`, and
/// the wasm exports.
pub(crate) fn expand(args: TokenStream, item: TokenStream) -> TokenStream {
    expand_for(service::resolve(), args, item)
}

pub(crate) fn expand_for(
    service: syn::Result<String>,
    args: TokenStream,
    item: TokenStream,
) -> TokenStream {
    let mut parsed: Item = match syn::parse2(item) {
        Ok(parsed) => parsed,
        Err(error) => return error.to_compile_error(),
    };
    let generated = service.and_then(|service| generate(&service, args, &parsed));
    match &mut parsed {
        Item::Impl(item) => {
            for member in &mut item.items {
                match member {
                    ImplItem::Fn(function) => function.attrs = kept(&function.attrs),
                    ImplItem::Const(constant) => {
                        constant.attrs = without(&constant.attrs, "export");
                    }
                    _ => {}
                }
            }
        }
        Item::Mod(item) => {
            for member in item.content.iter_mut().flat_map(|(_, items)| items) {
                match member {
                    Item::Fn(function) => function.attrs = kept(&function.attrs),
                    Item::Const(constant) => constant.attrs = without(&constant.attrs, "export"),
                    _ => {}
                }
            }
        }
        _ => return not_a_client(&parsed),
    }
    finish(&quote!(#parsed), generated)
}

/// The attributes of an exported function as it is kept: no `#[export]`, and
/// `clippy::too_many_arguments` allowed. The parameters of an export are the
/// fields of its arguments message, so their number follows the contract.
fn kept(attrs: &[syn::Attribute]) -> Vec<syn::Attribute> {
    let marked = api_bones_sdk_model::attrs::find(attrs, "export").is_some();
    let mut out = without(attrs, "export");
    if marked {
        // An export's parameters mirror the fields of its proto args message; the contract sets the count.
        // golden-lint: justified: parameter count is fixed by the proto args message (api-bones #289)
        out.push(parse_quote!(#[allow(clippy::too_many_arguments)]));
    }
    out
}

fn not_a_client(item: &Item) -> TokenStream {
    syn::Error::new_spanned(item, "`#[client]` goes on an impl or an inline module")
        .to_compile_error()
}

fn generate(service: &str, args: TokenStream, item: &Item) -> syn::Result<TokenStream> {
    let client = read(args, item)?;
    let members = members(item);
    let checks = checks(&client, &members);
    let exports = match item {
        Item::Impl(item) => {
            let home = Home::Impl(&item.self_ty);
            let mut out = wasm::expand(service, &client, &home, &members)?;
            out.extend(uniffi::expand(service, &client, &home, &members)?);
            out
        }
        Item::Mod(item) => {
            let home = Home::Module(&item.ident);
            let mut out = wasm::expand(service, &client, &home, &members)?;
            out.extend(uniffi::expand(service, &client, &home, &members)?);
            out
        }
        _ => return Err(syn::Error::new_spanned(item, "`#[client]` goes on an impl")),
    };
    Ok(quote!(#checks #exports))
}

/// The exported items of the impl or module, in source order.
fn members(item: &Item) -> Vec<Member> {
    let marked =
        |attrs: &[syn::Attribute]| api_bones_sdk_model::attrs::find(attrs, "export").is_some();
    match item {
        Item::Impl(item) => item
            .items
            .iter()
            .filter_map(|member| match member {
                ImplItem::Fn(function) if marked(&function.attrs) => {
                    Some(Member::of_fn(&function.sig))
                }
                ImplItem::Const(constant) if marked(&constant.attrs) => {
                    Some(Member::of_const(&constant.ident, &constant.ty))
                }
                _ => None,
            })
            .collect(),
        Item::Mod(item) => item
            .content
            .iter()
            .flat_map(|(_, items)| items)
            .filter_map(|member| match member {
                Item::Fn(function) if marked(&function.attrs) => Some(Member::of_fn(&function.sig)),
                Item::Const(constant) if marked(&constant.attrs) => {
                    Some(Member::of_const(&constant.ident, &constant.ty))
                }
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The model of this one impl.
///
/// A macro sees one item, so the other types of the crate are unknown to it.
/// Each name the impl mentions is declared as an error type of a stand-in
/// file, which makes the model's own check of the error type pass for names
/// and leaves every other rule to the model. Messages are named by their full
/// path, which the model reads without declarations. That the names really are
/// errors is asserted by [`checks`].
fn read(args: TokenStream, item: &Item) -> syn::Result<Client> {
    let mut marked = item.clone();
    let attrs = match &mut marked {
        Item::Impl(item) => &mut item.attrs,
        Item::Mod(item) => &mut item.attrs,
        _ => return Err(syn::Error::new_spanned(item, "`#[client]` goes on an impl")),
    };
    attrs.push(syn::Attribute {
        pound_token: syn::token::Pound::default(),
        style: syn::AttrStyle::Outer,
        bracket_token: syn::token::Bracket::default(),
        meta: syn::Meta::List(syn::MetaList {
            path: parse_quote!(client),
            delimiter: syn::MacroDelimiter::Paren(syn::token::Paren::default()),
            tokens: args,
        }),
    });
    let mut names = Vec::new();
    names_in(item.to_token_stream(), &mut names);
    names.sort();
    names.dedup();
    let mut items: Vec<Item> = Vec::new();
    for name in &names {
        if let Ok(stand_in) =
            syn::parse2::<Item>(quote!(#[error(emitter = "stand-in")] enum #name {}))
        {
            items.push(stand_in);
        }
    }
    items.push(marked);
    let file = syn::File {
        shebang: None,
        attrs: Vec::new(),
        items,
    };
    let model = Model::from_files([&file])?;
    model
        .clients
        .into_iter()
        .next()
        .ok_or_else(|| syn::Error::new_spanned(item, "`#[client]` goes on an impl"))
}

/// Every identifier in `tokens`.
fn names_in(tokens: TokenStream, out: &mut Vec<syn::Ident>) {
    for token in tokens {
        match token {
            TokenTree::Ident(ident) => out.push(ident),
            TokenTree::Group(group) => names_in(group.stream(), out),
            TokenTree::Punct(_) | TokenTree::Literal(_) => {}
        }
    }
}

/// A constant that does not compile unless every error converts into the SDK's
/// error. What crosses is a generated message or a scalar in its wrapper; the
/// exports' own `decode_strict`, `encode_*` and name guards assert that on
/// wasm32.
fn checks(client: &Client, functions: &[Member]) -> TokenStream {
    let mut error: Vec<syn::Type> = Vec::new();
    for (export, function) in client.exports.iter().zip(functions) {
        let Some(ty) = &function.output else {
            continue;
        };
        if let Return::Result { .. } = &export.ret {
            error.extend(head(ty).1.into_iter().nth(1));
        }
    }
    if error.is_empty() {
        return TokenStream::new();
    }
    quote! {
        const _: fn() = || {
            fn error<E: ::core::convert::Into<::api_bones_sdk::Error>>() {}
            #( error::<#error>(); )*
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::flat;

    const IMPL: &str = r"
        impl NumbersClient {
            #[export(sync)]
            pub fn label(&self, n: u32) -> String { body() }
            #[export]
            pub fn list(&self, limit: u32) -> ListCall<generated::contract::sdk::v1::Number> { body() }
            #[export(sync)]
            pub fn reset(&self) { body() }
            #[export]
            pub async fn get(&self, id: String) -> Result<generated::contract::sdk::v1::Number, NumbersError> { body() }
            pub fn plain(&self) {}
            #[export]
            pub async fn clear(&self) -> Result<(), Error> { body() }
        }
    ";

    fn run(args: TokenStream, item: &str) -> TokenStream {
        expand_for(Ok("svc".to_owned()), args, item.parse().expect("tokens"))
    }

    fn sample() -> TokenStream {
        run(
            quote!(surface = "numbers", proto = generated::contract::sdk::v1),
            IMPL,
        )
    }

    #[test]
    fn the_impl_is_kept_without_its_export_attributes() {
        let text = flat(&sample());
        assert!(text.starts_with("implNumbersClient{"), "{text}");
        assert!(
            text.contains(
                "pubfnlist(&self,limit:u32)->ListCall<generated::contract::sdk::v1::Number>{body()}"
            ),
            "{text}"
        );
        assert!(text.contains("pubfnplain(&self){}"), "{text}");
        assert!(!text.contains("#[export"), "{text}");
    }

    #[test]
    fn a_kept_export_allows_the_argument_count_its_contract_sets() {
        let text = flat(&sample());
        assert!(
            text.contains("#[allow(clippy::too_many_arguments)]pubfnlist("),
            "{text}"
        );
        assert!(
            !text.contains("#[allow(clippy::too_many_arguments)]pubfnplain"),
            "{text}"
        );
    }

    #[test]
    fn the_expansion_is_valid_rust() {
        syn::parse2::<syn::File>(sample()).expect("a file");
    }

    #[test]
    fn every_error_must_convert_into_the_sdk_error() {
        let text = flat(&sample());
        assert!(text.contains("error::<NumbersError>();"), "{text}");
        assert!(text.contains("error::<Error>();"), "{text}");
        assert!(
            text.contains("fnerror<E:::core::convert::Into<::api_bones_sdk::Error>>(){}"),
            "{text}"
        );
        assert!(!text.contains("Wire"), "{text}");
    }

    #[test]
    fn an_impl_without_a_result_export_has_no_checks() {
        let text = flat(&run(
            quote!(surface = "s"),
            "impl C { #[export(sync)] fn b(&self) {} #[export] fn l(&self) -> ListCall<generated::contract::v1::Number> { body() } }",
        ));
        assert!(!text.contains("fnerror"), "{text}");
        let text = flat(&run(quote!(surface = "s"), "impl C {}"));
        assert!(!text.contains("fnerror"), "{text}");
    }

    #[test]
    fn the_wasm_exports_are_generated_for_the_resolved_service() {
        let text = flat(&sample());
        assert!(
            text.contains("js_name=\"svc__NumbersClient__list\""),
            "{text}"
        );
        assert!(
            text.contains("js_name=\"svc__NumbersClient__label\""),
            "{text}"
        );
    }

    #[test]
    fn the_surface_is_required() {
        let text = flat(&run(TokenStream::new(), IMPL));
        assert!(text.starts_with("::core::compile_error!"), "{text}");
        assert!(text.contains("surface"), "{text}");
        assert!(text.contains("implNumbersClient{"), "{text}");
    }

    #[test]
    fn the_model_refuses_what_it_refuses_with_the_item_kept() {
        let text = flat(&run(
            quote!(surface = "s"),
            "impl C { #[export] fn a(&self) {} }",
        ));
        assert!(text.contains("anasyncexportis`async`"), "{text}");
        assert!(
            text.contains("implC{#[allow(clippy::too_many_arguments)]fna(&self){}}"),
            "{text}"
        );
        let text = flat(&run(
            quote!(surface = "s"),
            "impl C { #[export] async fn a(&self, n: u8) -> Result<(), Error> { body() } }",
        ));
        assert!(text.contains("nomessagetoarriveas"), "{text}");
        let text = flat(&run(
            quote!(surface = "s"),
            "impl C { #[export] async fn a(&mut self) -> Result<(), Error> { body() } }",
        ));
        assert!(text.contains("takes`&self`"), "{text}");
    }

    #[test]
    fn an_async_list_or_batch_export_is_refused_with_the_rule_and_a_stream_stays_async() {
        for handle in ["ListCall", "Batch"] {
            let text = flat(&run(
                quote!(surface = "s"),
                &format!(
                    "impl C {{ #[export] async fn a(&self) -> {handle}<generated::contract::v1::Number> {{ body() }} }}"
                ),
            ));
            assert!(
                text.contains("aListCallorBatchexportisaplain`fn`returningthehandle,not`async`"),
                "{handle}: {text}"
            );
        }
        let text = flat(&run(
            quote!(surface = "s"),
            "impl C { #[export] async fn a(&self) -> Stream<generated::contract::v1::Number> { body() } }",
        ));
        assert!(!text.contains("compile_error"), "{text}");
        assert!(text.contains("ApiBonesJsCAHandle"), "{text}");
    }

    #[test]
    fn a_from_host_in_the_impl_is_kept_and_its_options_message_reaches_the_constructor() {
        let text = flat(&run(
            quote!(surface = "s"),
            "impl C { pub fn from_host(base_url: String, host: JsValue, options: generated::contract::v1::COptions) -> Result<Self, Error> { body() } }",
        ));
        assert!(
            text.contains("pubfnfrom_host(base_url:String,host:JsValue,options:generated::contract::v1::COptions)"),
            "{text}"
        );
        assert!(
            text.contains(
                "<C>::from_host(base_url,host,::api_bones_sdk::__private::decode_options(&__options)?)"
            ),
            "{text}"
        );
    }

    #[test]
    fn an_option_of_from_host_that_is_not_a_message_is_refused() {
        let text = flat(&run(
            quote!(surface = "s"),
            "impl C { pub fn from_host(base_url: String, host: JsValue, size: u32) -> Result<Self, Error> { body() } }",
        ));
        assert!(text.starts_with("::core::compile_error!"), "{text}");
        assert!(text.contains("option`size`"), "{text}");
        assert!(text.contains("optionsmessage"), "{text}");
    }

    #[test]
    fn more_than_one_option_is_refused() {
        let text = flat(&run(
            quote!(surface = "s"),
            "impl C { pub fn from_host(base_url: String, host: JsValue, a: A, b: B) -> Result<Self, Error> { body() } }",
        ));
        assert!(text.starts_with("::core::compile_error!"), "{text}");
        assert!(text.contains("oneoptionsmessage"), "{text}");
    }

    #[test]
    fn a_trait_impl_or_generic_impl_is_refused() {
        let text = flat(&run(quote!(surface = "s"), "impl<T> C<T> {}"));
        assert!(text.contains("inherent,non-genericimpl"), "{text}");
        let text = flat(&run(
            quote!(surface = "s"),
            "impl Drop for C { fn drop(&mut self) {} }",
        ));
        assert!(text.contains("inherent,non-genericimpl"), "{text}");
    }

    #[test]
    fn a_service_that_cannot_be_named_is_reported_with_the_item_kept() {
        let text = flat(&expand_for(
            Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                "no service",
            )),
            quote!(surface = "s"),
            quote!(impl C {}),
        ));
        assert!(text.starts_with("::core::compile_error!"), "{text}");
        assert!(text.ends_with("implC{}"), "{text}");
    }

    #[test]
    fn an_item_that_is_not_an_impl_is_only_refused() {
        let text = flat(&run(quote!(surface = "s"), "struct S;"));
        assert!(text.starts_with("::core::compile_error!"), "{text}");
        assert!(!text.contains("structS"), "{text}");
    }

    #[test]
    fn a_double_underscore_in_a_name_is_refused() {
        let text = flat(&run(
            quote!(surface = "s"),
            "impl C { #[export(sync)] fn a__b(&self) {} }",
        ));
        assert!(text.contains("`__`"), "{text}");
    }

    #[test]
    fn the_real_entry_point_resolves_the_service_from_the_build() {
        let text = flat(&expand(quote!(surface = "s"), quote!(impl C {})));
        assert!(
            text.contains("js_name=\"") && text.contains("__C__new\""),
            "{text}"
        );
    }

    const STATICS: &str = r"
        impl Tools {
            #[export(sync)]
            pub fn label(value: u32) -> String { body() }
            #[export(sync)]
            pub const LIMIT: u32 = 5;
            #[export(sync)]
            pub fn check(&self) -> Result<(), Error> { body() }
        }
    ";

    #[test]
    fn a_static_function_and_a_constant_are_static_exports_of_the_class() {
        let text = flat(&run(
            quote!(surface = "s", proto = generated::contract::v1),
            STATICS,
        ));
        assert!(!text.contains("#[export"), "{text}");
        assert!(
            text.contains("pubfnlabel(value:u32)->String{body()}"),
            "{text}"
        );
        assert!(text.contains("pubconstLIMIT:u32=5;"), "{text}");
        assert!(text.contains("js_name=\"svc__Tools__label\""), "{text}");
        assert!(text.contains("js_name=\"svc__Tools__LIMIT\""), "{text}");
        assert!(text.contains("pubfnlabel_js(__args:"), "{text}");
        assert!(text.contains("pubfnlimit_const_js(__args:"), "{text}");
        assert!(text.contains("<Tools>::label(value)"), "{text}");
        assert!(text.contains("<Tools>::LIMIT"), "{text}");
        assert!(text.contains("__client.check()"), "{text}");
    }

    #[test]
    fn a_client_module_is_kept_and_expands_to_a_class_without_an_instance() {
        let text = flat(&run(
            quote!(surface = "s", proto = generated::contract::v1),
            "mod digits { #[export(sync)] pub fn doubled(value: u32) -> u32 { body() } #[export(sync)] pub const BASE: u32 = 10; }",
        ));
        assert!(text.contains("moddigits{"), "{text}");
        assert!(
            text.contains("pubfndoubled(value:u32)->u32{body()}"),
            "{text}"
        );
        assert!(!text.contains("#[export"), "{text}");
        assert!(text.contains("js_name=\"svc__digits__doubled\""), "{text}");
        assert!(text.contains("self::digits::doubled(value)"), "{text}");
        assert!(text.contains("self::digits::BASE"), "{text}");
        assert!(text.contains("pubstructApiBonesJsdigits{}"), "{text}");
        assert!(!text.contains("__new"), "{text}");
        assert!(!text.contains("from_host"), "{text}");
        assert!(!text.contains("inner"), "{text}");
        syn::parse2::<syn::File>(run(
            quote!(surface = "s", proto = generated::contract::v1),
            "mod digits { #[export(sync)] pub fn doubled(value: u32) -> u32 { body() } }",
        ))
        .expect("a file");
    }

    #[test]
    fn a_module_that_breaks_a_rule_is_reported_with_the_module_kept() {
        let text = flat(&run(
            quote!(surface = "s"),
            "mod m { #[export] pub async fn f() -> Result<(), Error> { body() } }",
        ));
        assert!(text.contains("anasyncexporttakes`&self`"), "{text}");
        assert!(text.contains("modm{"), "{text}");
    }
}
