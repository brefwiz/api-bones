// SPDX-License-Identifier: MIT
//! Pieces every expansion shares.

use api_bones_sdk_model::attrs::find;
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Attribute, GenericArgument, PathArguments, Type};

/// The items the generated code reaches the SDK's dependencies through, so a
/// Layer 2 crate depends on `api-bones-sdk` alone.
pub(crate) fn private() -> TokenStream {
    quote!(::api_bones_sdk::__private)
}

/// `attrs` without the attribute named `name`, spelled bare or behind a path.
pub(crate) fn without(attrs: &[Attribute], name: &str) -> Vec<Attribute> {
    attrs
        .iter()
        .filter(|attr| find(std::slice::from_ref(attr), name).is_none())
        .cloned()
        .collect()
}

/// The expansion of `item` once `generate` has run: the item, then what was
/// generated; or, when `generate` refuses, the refusal and the item, so the
/// rest of the crate still resolves and only the refusal is reported.
pub(crate) fn finish(item: &TokenStream, generated: syn::Result<TokenStream>) -> TokenStream {
    match generated {
        Ok(extra) => quote!(#item #extra),
        Err(error) => {
            let error = error.to_compile_error();
            quote!(#error #item)
        }
    }
}

/// The refusal of a method that carries `#[export]` outside a `#[client]`
/// impl.
pub(crate) fn misplaced_export(_args: TokenStream, item: &TokenStream) -> TokenStream {
    let error = syn::Error::new(
        Span::call_site(),
        "`#[export]` belongs on a method of an impl that carries `#[client]`",
    )
    .to_compile_error();
    quote!(#error #item)
}

/// The head name of a path type and its type arguments; an empty name and no
/// arguments for any other form of type.
pub(crate) fn head(ty: &Type) -> (String, Vec<Type>) {
    let Some(segment) = (match ty {
        Type::Path(path) if path.qself.is_none() => path.path.segments.last(),
        _ => None,
    }) else {
        return (String::new(), Vec::new());
    };
    let args = match &segment.arguments {
        PathArguments::AngleBracketed(args) => args
            .args
            .iter()
            .filter_map(|arg| match arg {
                GenericArgument::Type(ty) => Some(ty.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    (segment.ident.to_string(), args)
}

/// The upper camel case spelling of a snake case name.
pub(crate) fn upper_camel(name: &str) -> String {
    name.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect::<String>())
                .unwrap_or_default()
        })
        .collect()
}

/// `tokens` as text without spaces, the form the expansion tests match on.
#[cfg(test)]
pub(crate) fn flat(tokens: &TokenStream) -> String {
    tokens.to_string().replace(' ', "")
}

/// The sync exports every expansion's fixture carries, as source text.
#[cfg(test)]
macro_rules! sync_exports {
    () => {
        r#"
            #[export(sync)]
            pub fn label(&self, kind: &str) -> String { body() }
            #[export(sync)]
            pub fn reset(&self) { body() }
            #[export(sync)]
            pub fn check(&self) -> Result<(), Error> { body() }"#
    };
}
#[cfg(test)]
pub(crate) use sync_exports;

/// The `NumbersClient` fixture the expansion tests share, as source text. The
/// caller sets what differs: the parameters of `list` (with their parentheses),
/// the attribute of `get`, the exports between `get` and the sync ones, and the
/// last export with the closing of the impl.
#[cfg(test)]
macro_rules! numbers_client {
    ($list:literal, $get:literal, $middle:literal, $last:literal) => {
        concat!(
            r#"
        #[client(surface = "connect-pagination", proto = generated::contract::sdk::v1)]
        impl NumbersClient {
            #[export]
            pub fn list"#,
            $list,
            r#" -> ListCall<generated::contract::sdk::v1::Number> { body() }
            #[export]
            pub async fn watch(&self) -> Stream<generated::contract::sdk::v1::Number> { body() }
            #[export]
            pub fn get_all(&self, ids: Vec<String>) -> Batch<generated::contract::sdk::v1::Number> { body() }
            "#,
            $get,
            r#"
            pub async fn get(&self, id: String) -> Result<generated::contract::sdk::v1::Number, Error> { body() }
"#,
            $middle,
            $crate::common::sync_exports!(),
            $last
        )
    };
}
#[cfg(test)]
pub(crate) use numbers_client;

/// Expands the first impl of `source` with `expand`, after `change` edits the
/// model's first client, for shapes the model itself never produces. `expand`
/// receives the client, the impl's home and the exported members.
#[cfg(test)]
pub(crate) fn expand_first_impl(
    source: &str,
    change: impl FnOnce(&mut api_bones_sdk_model::Client),
    expand: impl FnOnce(
        &api_bones_sdk_model::Client,
        &crate::wasm::Home<'_>,
        &[crate::wasm::Member],
    ) -> syn::Result<TokenStream>,
) -> syn::Result<TokenStream> {
    use crate::wasm::{Home, Member};

    let file: syn::File = syn::parse_str(source).expect("a file");
    let mut model = api_bones_sdk_model::Model::from_files([&file]).expect("a model");
    change(&mut model.clients[0]);
    let item = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Impl(item) => Some(item),
            _ => None,
        })
        .expect("an impl");
    let exported = |attrs: &[Attribute]| find(attrs, "export").is_some();
    let functions: Vec<Member> = item
        .items
        .iter()
        .filter_map(|member| match member {
            syn::ImplItem::Fn(function) if exported(&function.attrs) => {
                Some(Member::of_fn(&function.sig))
            }
            syn::ImplItem::Const(constant) if exported(&constant.attrs) => {
                Some(Member::of_const(&constant.ident, &constant.ty))
            }
            _ => None,
        })
        .collect();
    expand(&model.clients[0], &Home::Impl(&item.self_ty), &functions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_private_path_is_the_sdks_hidden_module() {
        assert_eq!(flat(&private()), "::api_bones_sdk::__private");
    }

    #[test]
    fn without_drops_the_named_attribute_bare_or_pathed() {
        let item: syn::ItemFn =
            syn::parse_str("#[export] #[api_bones_sdk::export(sync)] #[inline] fn f() {}")
                .expect("an item");
        let kept = without(&item.attrs, "export");
        assert_eq!(kept.len(), 1);
        assert!(kept[0].path().is_ident("inline"));
    }

    #[test]
    fn finish_puts_the_item_before_what_was_generated() {
        let item = quote!(
            struct A;
        );
        let out = finish(&item, Ok(quote!(impl A {})));
        assert_eq!(flat(&out), "structA;implA{}");
    }

    #[test]
    fn finish_reports_a_refusal_and_keeps_the_item() {
        let item = quote!(
            struct A;
        );
        let out = finish(&item, Err(syn::Error::new(Span::call_site(), "no")));
        let text = flat(&out);
        assert!(text.starts_with("::core::compile_error!"), "{text}");
        assert!(text.ends_with("structA;"), "{text}");
    }

    #[test]
    fn a_misplaced_export_is_refused_and_the_method_kept() {
        let out = misplaced_export(
            quote!(),
            &quote!(
                fn f() {}
            ),
        );
        let text = flat(&out);
        assert!(
            text.contains("belongsonamethodofanimplthatcarries"),
            "{text}"
        );
        assert!(text.ends_with("fnf(){}"), "{text}");
    }

    fn ty(text: &str) -> Type {
        syn::parse_str(text).expect("a type")
    }

    #[test]
    fn the_head_of_a_path_type_is_its_last_name_and_type_arguments() {
        let (name, args) = head(&ty("std::result::Result<Vec<u8>, Error>"));
        assert_eq!(name, "Result");
        assert_eq!(args.len(), 2);
        assert_eq!(head(&ty("Number")), ("Number".to_owned(), Vec::new()));
        let (name, args) = head(&ty("Handle<'a, Item>"));
        assert_eq!((name.as_str(), args.len()), ("Handle", 1));
    }

    #[test]
    fn any_other_form_of_type_has_no_head() {
        assert_eq!(head(&ty("(u32, u32)")), (String::new(), Vec::new()));
        assert_eq!(head(&ty("<T as Trait>::Out")), (String::new(), Vec::new()));
        assert_eq!(head(&ty("fn(u32)")), (String::new(), Vec::new()));
    }

    #[test]
    fn snake_case_becomes_upper_camel_case() {
        assert_eq!(upper_camel("batch_get_widgets"), "BatchGetWidgets");
        assert_eq!(upper_camel("list"), "List");
        assert_eq!(upper_camel("_x__y_"), "XY");
        assert_eq!(upper_camel(""), "");
    }
}

/// Client source shared by the unit tests of the per-language emitters: an
/// async export taking its message whole and a sync export taking it by
/// reference, followed by `extra` members.
#[cfg(test)]
pub(crate) fn whole_message_client(extra: &str) -> String {
    format!(
        r#"
        #[client(surface = "s", proto = generated::contract::sdk::v1)]
        impl C {{
            #[export]
            pub async fn derive(&self, request: generated::contract::sdk::v1::DeriveArgs) -> Result<generated::contract::sdk::v1::Number, Error> {{ body() }}
            #[export(sync, args = generated::contract::v1::Inspect)]
            pub fn inspect(&self, request: &Inspect) -> String {{ body() }}
            {extra}
        }}
    "#
    )
}
