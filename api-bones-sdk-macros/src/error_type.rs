// SPDX-License-Identifier: MIT
//! `#[error]`: the errors a client declares, which cross as the error JSON.

use api_bones_sdk_model::attrs::{ErrorArgs, ErrorVariantArgs, find, tokens};
use api_bones_sdk_model::{Env, WireType, classify};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Fields, ItemEnum};

use crate::common::{finish, without};

/// The enum, kept as written, with `Display`, `std::error::Error` and the
/// conversion into `api_bones_sdk::Error`.
pub(crate) fn expand(args: TokenStream, item: TokenStream) -> TokenStream {
    let mut parsed: ItemEnum = match syn::parse2(item) {
        Ok(parsed) => parsed,
        Err(error) => return error.to_compile_error(),
    };
    parsed.attrs = without(&parsed.attrs, "error");
    let generated = impls(args, &parsed);
    for variant in &mut parsed.variants {
        variant.attrs = without(&variant.attrs, "error");
    }
    finish(&quote!(#parsed), generated)
}

fn impls(args: TokenStream, item: &ItemEnum) -> syn::Result<TokenStream> {
    let args = ErrorArgs::parse(args)?;
    if !item.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &item.generics,
            "an `#[error]` type is not generic",
        ));
    }
    let name = &item.ident;
    let emitter = &args.emitter;
    let mut displays = Vec::new();
    let mut conversions = Vec::new();
    for variant in &item.variants {
        let attr = find(&variant.attrs, "error").ok_or_else(|| {
            syn::Error::new_spanned(
                variant,
                "an `#[error]` variant needs `#[error(code = \"...\", platform = \"...\")]`",
            )
        })?;
        let ErrorVariantArgs { code, platform } = ErrorVariantArgs::parse(tokens(attr)?)?;
        let ident = &variant.ident;
        let (pattern, entries) = match &variant.fields {
            Fields::Unit => (quote!(#name::#ident), Vec::new()),
            Fields::Named(named) => {
                let mut bound = Vec::new();
                let mut entries = Vec::new();
                for (field, ty) in named
                    .named
                    .iter()
                    .filter_map(|field| field.ident.as_ref().map(|ident| (ident, &field.ty)))
                {
                    argument(field, ty)?;
                    let key = field.to_string();
                    entries.push(quote! {
                        (
                            ::std::string::ToString::to_string(#key),
                            ::api_bones_sdk::Arg::from(#field),
                        )
                    });
                    bound.push(field);
                }
                (quote!(#name::#ident { #(#bound),* }), entries)
            }
            Fields::Unnamed(_) => {
                return Err(syn::Error::new_spanned(
                    variant,
                    "an `#[error]` variant names its fields: they become `args` by name",
                ));
            }
        };
        displays.push(quote!(#name::#ident { .. } => #code));
        conversions.push(quote! {
            #pattern => ::api_bones_sdk::Error::emitted(
                #emitter,
                #code,
                #platform,
                #code,
                ::std::collections::BTreeMap::<
                    ::std::string::String,
                    ::api_bones_sdk::Arg,
                >::from([#(#entries),*]),
            )
        });
    }
    Ok(quote! {
        impl ::core::fmt::Display for #name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.write_str(match *self { #(#displays,)* })
            }
        }

        impl ::std::error::Error for #name {}

        impl ::core::convert::From<#name> for ::api_bones_sdk::Error {
            fn from(error: #name) -> Self {
                match error { #(#conversions,)* }
            }
        }
    })
}

/// A field of an error variant becomes one entry of `args`, so it is a String,
/// an i64, an f64 or a `SystemTime`.
fn argument(field: &syn::Ident, ty: &syn::Type) -> syn::Result<()> {
    let what = format!("field `{field}`");
    match classify(ty, &Env::deferred(), &what)? {
        WireType::String | WireType::I64 | WireType::F64 | WireType::SystemTime => Ok(()),
        _ => Err(syn::Error::new_spanned(
            ty,
            format!("{what}: an error argument is a String, i64, f64 or SystemTime"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::flat;

    fn out(item: &str) -> String {
        flat(&expand(
            quote!(emitter = "widgets"),
            item.parse().expect("tokens"),
        ))
    }

    const SAMPLE: &str = r#"
        #[derive(Debug)]
        pub enum WidgetError {
            #[error(code = "WIDGET_GONE", platform = "NOT_FOUND")]
            Gone { id: String, at: SystemTime, size: i64, ratio: f64 },
            #[error(code = "WIDGET_BUSY", platform = "CONFLICT")]
            Busy,
        }
    "#;

    #[test]
    fn an_error_displays_its_code() {
        let text = out(SAMPLE);
        assert!(
            text.contains("impl::core::fmt::DisplayforWidgetError"),
            "{text}"
        );
        assert!(
            text.contains("WidgetError::Gone{..}=>\"WIDGET_GONE\""),
            "{text}"
        );
        assert!(
            text.contains("WidgetError::Busy{..}=>\"WIDGET_BUSY\""),
            "{text}"
        );
        assert!(
            text.contains("impl::std::error::ErrorforWidgetError{}"),
            "{text}"
        );
    }

    #[test]
    fn an_error_converts_into_the_sdk_error_with_its_fields_as_args() {
        let text = out(SAMPLE);
        assert!(
            text.contains("impl::core::convert::From<WidgetError>for::api_bones_sdk::Error"),
            "{text}"
        );
        assert!(text.contains("WidgetError::Gone{id,at,size,ratio}=>::api_bones_sdk::Error::emitted(\"widgets\",\"WIDGET_GONE\",\"NOT_FOUND\",\"WIDGET_GONE\""), "{text}");
        assert!(
            text.contains(
                "(::std::string::ToString::to_string(\"size\"),::api_bones_sdk::Arg::from(size),)"
            ),
            "{text}"
        );
        assert!(text.contains("WidgetError::Busy=>::api_bones_sdk::Error::emitted(\"widgets\",\"WIDGET_BUSY\",\"CONFLICT\""), "{text}");
    }

    #[test]
    fn the_expansion_is_valid_rust_without_the_error_attributes() {
        let tokens = expand(quote!(emitter = "w"), SAMPLE.parse().expect("tokens"));
        let text = flat(&tokens);
        assert!(
            text.starts_with("#[derive(Debug)]pubenumWidgetError{Gone{"),
            "{text}"
        );
        assert!(!text.contains("#[error"), "{text}");
        syn::parse2::<syn::File>(tokens).expect("a file");
    }

    #[test]
    fn an_empty_enum_still_expands() {
        syn::parse2::<syn::File>(expand(
            quote!(emitter = "w"),
            quote!(
                enum Never {}
            ),
        ))
        .expect("a file");
    }

    #[test]
    fn the_type_needs_its_emitter() {
        let text = flat(&expand(
            TokenStream::new(),
            quote!(
                enum E {
                    A,
                }
            ),
        ));
        assert!(text.contains("needs`emitter"), "{text}");
    }

    #[test]
    fn a_generic_enum_is_refused() {
        assert!(out("enum E<T> { A }").contains("isnotgeneric"));
    }

    #[test]
    fn a_variant_needs_its_code_and_platform() {
        assert!(out("enum E { A }").contains("needs`#[error(code="));
        let text = out("enum E { #[error(code = \"X\")] A }");
        assert!(text.contains("needs`platform"), "{text}");
        let named = out("enum E { #[error = \"x\"] A }");
        assert!(named.contains("inparentheses"), "{named}");
    }

    #[test]
    fn a_variant_names_its_fields() {
        let text = out("enum E { #[error(code = \"X\", platform = \"Y\")] A(String) }");
        assert!(text.contains("namesitsfields"), "{text}");
    }

    #[test]
    fn a_field_is_an_argument_kind() {
        let text = out("enum E { #[error(code = \"X\", platform = \"Y\")] A { n: u32 } }");
        assert!(
            text.contains("anerrorargumentisaString,i64,f64orSystemTime"),
            "{text}"
        );
        let outside = out("enum E { #[error(code = \"X\", platform = \"Y\")] A { n: u8 } }");
        assert!(outside.contains("field`n`"), "{outside}");
        assert!(outside.contains("allowed:"), "{outside}");
    }

    #[test]
    fn an_item_that_is_not_an_enum_is_only_refused() {
        let text = flat(&expand(
            quote!(emitter = "w"),
            quote!(
                struct S;
            ),
        ));
        assert!(text.starts_with("::core::compile_error!"), "{text}");
        assert!(!text.contains("structS"), "{text}");
    }
}
