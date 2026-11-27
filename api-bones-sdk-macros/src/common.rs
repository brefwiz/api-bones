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
