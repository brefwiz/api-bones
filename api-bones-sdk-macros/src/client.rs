// SPDX-License-Identifier: MIT
//! `#[client]`: the impl whose `#[export]` methods are the Layer 2 surface.

use api_bones_sdk_model::{Client, Model, Return};
use proc_macro2::{TokenStream, TokenTree};
use quote::{ToTokens, quote};
use syn::{ImplItem, ImplItemFn, Item, ItemImpl, ReturnType, Type, parse_quote};

use crate::common::{finish, head, without};
use crate::{service, wasm};

/// The impl, kept as written but for the `#[export]` attributes, the
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
    let mut parsed: ItemImpl = match syn::parse2(item) {
        Ok(parsed) => parsed,
        Err(error) => return error.to_compile_error(),
    };
    let generated = service.and_then(|service| generate(&service, args, &parsed));
    for member in &mut parsed.items {
        if let ImplItem::Fn(function) = member {
            function.attrs = without(&function.attrs, "export");
        }
    }
    finish(&quote!(#parsed), generated)
}

fn generate(service: &str, args: TokenStream, item: &ItemImpl) -> syn::Result<TokenStream> {
    let client = read(args, item)?;
    let functions: Vec<&ImplItemFn> = item
        .items
        .iter()
        .filter_map(|member| match member {
            ImplItem::Fn(function)
                if api_bones_sdk_model::attrs::find(&function.attrs, "export").is_some() =>
            {
                Some(function)
            }
            _ => None,
        })
        .collect();
    let checks = checks(&client, &functions);
    let exports = wasm::expand(service, &client, &item.self_ty, &functions)?;
    Ok(quote!(#checks #exports))
}

/// The model of this one impl.
///
/// A macro sees one item, so the other types of the crate are unknown to it.
/// Each name the impl mentions is declared as an error type of a stand-in
/// file, which makes the model's own check of the error type pass for names
/// and leaves every other rule to the model. Messages are named by their full
/// path, which the model reads without declarations. That the names really are
/// errors is asserted by [`checks`].
fn read(args: TokenStream, item: &ItemImpl) -> syn::Result<Client> {
    let mut marked = item.clone();
    marked.attrs.push(syn::Attribute {
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
    items.push(Item::Impl(marked));
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
        .ok_or_else(|| syn::Error::new_spanned(&item.self_ty, "`#[client]` goes on an impl"))
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
fn checks(client: &Client, functions: &[&ImplItemFn]) -> TokenStream {
    let mut error: Vec<Type> = Vec::new();
    for (export, function) in client.exports.iter().zip(functions) {
        let ReturnType::Type(_, ty) = &function.sig.output else {
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
            pub async fn list(&self, limit: u32) -> ListCall<generated::contract::sdk::v1::Number> { body() }
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
            text.contains("pubasyncfnlist(&self,limit:u32)->ListCall<generated::contract::sdk::v1::Number>{body()}"),
            "{text}"
        );
        assert!(text.contains("pubfnplain(&self){}"), "{text}");
        assert!(!text.contains("#[export"), "{text}");
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
            "impl C { #[export(sync)] fn b(&self) {} #[export] async fn l(&self) -> ListCall<generated::contract::v1::Number> { body() } }",
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
        assert!(text.contains("implC{fna(&self){}}"), "{text}");
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
}
