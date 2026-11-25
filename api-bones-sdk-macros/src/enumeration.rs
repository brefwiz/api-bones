// SPDX-License-Identifier: MIT
//! `#[enumeration]`: a fieldless enum that crosses as a snake case string.

use api_bones_sdk_model::attrs::no_args;
use api_bones_sdk_model::enumeration_values;
use proc_macro2::TokenStream;
use quote::quote;
use syn::ItemEnum;

use crate::common::{finish, private, without};

/// The enum, kept as written, and its `Wire` impl.
pub(crate) fn expand(args: TokenStream, item: TokenStream) -> TokenStream {
    let mut parsed: ItemEnum = match syn::parse2(item) {
        Ok(parsed) => parsed,
        Err(error) => return error.to_compile_error(),
    };
    parsed.attrs = without(&parsed.attrs, "enumeration");
    let generated = wire_impl(args, &parsed);
    finish(&quote!(#parsed), generated)
}

fn wire_impl(args: TokenStream, item: &ItemEnum) -> syn::Result<TokenStream> {
    no_args("enumeration", args)?;
    if !item.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &item.generics,
            "an `#[enumeration]` is not generic",
        ));
    }
    let values = enumeration_values(item)?;
    let variants: Vec<_> = item.variants.iter().map(|variant| &variant.ident).collect();
    let expected = values.join(", ");
    let private = private();
    let name = &item.ident;
    Ok(quote! {
        impl ::api_bones_sdk::Wire for #name {
            fn encode(
                &self,
            ) -> ::core::result::Result<#private::serde_json::Value, ::api_bones_sdk::BadValue> {
                ::core::result::Result::Ok(#private::serde_json::Value::String(
                    ::std::string::ToString::to_string(match *self {
                        #( Self::#variants => #values, )*
                    }),
                ))
            }

            fn decode(
                value: &#private::serde_json::Value,
            ) -> ::core::result::Result<Self, ::api_bones_sdk::BadValue> {
                let #private::serde_json::Value::String(text) = value else {
                    return ::core::result::Result::Err(::api_bones_sdk::BadValue::new(
                        "expected a string",
                    ));
                };
                match text.as_str() {
                    #( #values => ::core::result::Result::Ok(Self::#variants), )*
                    other => ::core::result::Result::Err(::api_bones_sdk::BadValue::new(
                        ::std::format!("`{other}` is not one of {}", #expected),
                    )),
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{expanded, flat};

    fn out(item: &str) -> String {
        expanded(expand, item)
    }

    #[test]
    fn variants_cross_as_snake_case_strings_both_ways() {
        let text = out("#[derive(Debug)] pub enum Kind { Exact, LowerBound }");
        assert!(text.contains("impl::api_bones_sdk::WireforKind"), "{text}");
        assert!(text.contains("Self::Exact=>\"exact\""), "{text}");
        assert!(text.contains("Self::LowerBound=>\"lower_bound\""), "{text}");
        assert!(
            text.contains("\"lower_bound\"=>::core::result::Result::Ok(Self::LowerBound)"),
            "{text}"
        );
        assert!(text.contains("\"exact,lower_bound\""), "{text}");
    }

    #[test]
    fn the_expansion_is_valid_rust_and_drops_the_attribute() {
        let tokens = expand(
            TokenStream::new(),
            quote!(
                #[enumeration]
                #[derive(Clone)]
                enum K {
                    A,
                }
            ),
        );
        let text = flat(&tokens);
        assert!(text.starts_with("#[derive(Clone)]enumK{A,}"), "{text}");
        syn::parse2::<syn::File>(tokens).expect("a file");
    }

    #[test]
    fn an_empty_enum_still_expands() {
        syn::parse2::<syn::File>(expand(
            TokenStream::new(),
            quote!(
                enum Never {}
            ),
        ))
        .expect("a file");
    }

    #[test]
    fn an_argument_a_generic_or_a_field_is_refused() {
        let text = flat(&expand(
            quote!(x),
            quote!(
                enum K {
                    A,
                }
            ),
        ));
        assert!(text.contains("takesnoarguments"), "{text}");
        assert!(out("enum K<T> { A }").contains("isnotgeneric"));
        assert!(out("enum K { A(u32) }").contains("onlyunitvariants"));
        assert!(out("enum K { A { x: u32 } }").contains("onlyunitvariants"));
    }

    #[test]
    fn an_item_that_is_not_an_enum_is_only_refused() {
        let text = flat(&expand(
            TokenStream::new(),
            quote!(
                struct S;
            ),
        ));
        assert!(text.starts_with("::core::compile_error!"), "{text}");
        assert!(!text.contains("structS"), "{text}");
    }
}
