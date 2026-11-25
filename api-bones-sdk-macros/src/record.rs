// SPDX-License-Identifier: MIT
//! `#[record]`: a struct that crosses as a JSON object.

use api_bones_sdk_model::attrs::no_args;
use api_bones_sdk_model::{Env, classify, json_name};
use proc_macro2::TokenStream;
use quote::quote;
use syn::ext::IdentExt as _;
use syn::{Fields, ItemStruct};

use crate::common::{finish, private, without};

/// The struct, kept as written, and its `Wire` impl.
pub(crate) fn expand(args: TokenStream, item: TokenStream) -> TokenStream {
    let mut parsed: ItemStruct = match syn::parse2(item) {
        Ok(parsed) => parsed,
        Err(error) => return error.to_compile_error(),
    };
    parsed.attrs = without(&parsed.attrs, "record");
    let generated = wire_impl(args, &parsed);
    finish(&quote!(#parsed), generated)
}

fn wire_impl(args: TokenStream, item: &ItemStruct) -> syn::Result<TokenStream> {
    no_args("record", args)?;
    if !item.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &item.generics,
            "a `#[record]` is not generic",
        ));
    }
    let Fields::Named(fields) = &item.fields else {
        return Err(syn::Error::new_spanned(
            &item.ident,
            "a `#[record]` has named fields",
        ));
    };
    let env = Env::deferred();
    let mut names = Vec::new();
    let mut keys = Vec::new();
    for (ident, ty) in fields
        .named
        .iter()
        .filter_map(|field| field.ident.as_ref().map(|ident| (ident, &field.ty)))
    {
        let name = ident.unraw().to_string();
        classify(ty, &env, &format!("field `{name}`"))?;
        keys.push(json_name(&name));
        names.push(ident);
    }
    let private = private();
    let name = &item.ident;
    Ok(quote! {
        impl ::api_bones_sdk::Wire for #name {
            fn encode(
                &self,
            ) -> ::core::result::Result<#private::serde_json::Value, ::api_bones_sdk::BadValue> {
                let members: #private::serde_json::Map<
                    ::std::string::String,
                    #private::serde_json::Value,
                > = [
                    #(
                        (
                            ::std::string::ToString::to_string(#keys),
                            ::api_bones_sdk::Wire::encode(&self.#names)
                                .map_err(|error| error.within(#keys))?,
                        ),
                    )*
                ]
                .into_iter()
                .collect();
                ::core::result::Result::Ok(#private::serde_json::Value::Object(members))
            }

            fn decode(
                value: &#private::serde_json::Value,
            ) -> ::core::result::Result<Self, ::api_bones_sdk::BadValue> {
                let #private::serde_json::Value::Object(members) = value else {
                    return ::core::result::Result::Err(::api_bones_sdk::BadValue::new(
                        "expected an object",
                    ));
                };
                ::core::result::Result::Ok(Self {
                    #( #names: ::api_bones_sdk::field(members, #keys)?, )*
                })
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
    fn a_record_gets_a_wire_impl_keyed_by_camel_case_field_names() {
        let text = out(
            "#[derive(Debug)] pub struct Number { pub value: i64, pub page_size: Option<u32> }",
        );
        assert!(text.contains("pubstructNumber{"), "{text}");
        assert!(
            text.contains("impl::api_bones_sdk::WireforNumber"),
            "{text}"
        );
        assert!(
            text.contains("::api_bones_sdk::Wire::encode(&self.page_size)"),
            "{text}"
        );
        assert!(text.contains(".within(\"pageSize\")"), "{text}");
        assert!(
            text.contains("page_size:::api_bones_sdk::field(members,\"pageSize\")?"),
            "{text}"
        );
        assert!(
            text.contains("value:::api_bones_sdk::field(members,\"value\")?"),
            "{text}"
        );
    }

    #[test]
    fn the_expansion_is_valid_rust() {
        let tokens = expand(
            TokenStream::new(),
            quote!(
                pub struct R {
                    a: Vec<u8>,
                    b: std::collections::BTreeMap<String, Inner>,
                }
            ),
        );
        syn::parse2::<syn::File>(tokens).expect("a file");
    }

    #[test]
    fn the_record_attribute_is_removed_and_other_attributes_stay() {
        let tokens = expand(
            TokenStream::new(),
            quote!(
                #[api_bones_sdk::record]
                #[derive(Clone)]
                struct R {
                    a: bool,
                }
            ),
        );
        let text = flat(&tokens);
        assert!(text.starts_with("#[derive(Clone)]structR"), "{text}");
        assert!(!text.contains("record]"), "{text}");
    }

    #[test]
    fn a_raw_identifier_keys_by_its_plain_name() {
        let text = out("struct R { r#type: String }");
        assert!(text.contains("members,\"type\")"), "{text}");
        assert!(text.contains("r#type:::api_bones_sdk::field"), "{text}");
    }

    #[test]
    fn an_argument_is_refused() {
        let text = flat(&expand(
            quote!(camel),
            quote!(
                struct R {
                    a: bool,
                }
            ),
        ));
        assert!(text.contains("takesnoarguments"), "{text}");
        assert!(text.ends_with("structR{a:bool,}"), "{text}");
    }

    #[test]
    fn a_generic_or_tuple_struct_is_refused() {
        assert!(out("struct R<T> { a: T }").contains("isnotgeneric"));
        assert!(out("struct R(u32);").contains("hasnamedfields"));
        assert!(out("struct R;").contains("hasnamedfields"));
    }

    #[test]
    fn a_field_outside_the_allowed_set_is_refused_and_named() {
        let text = out("struct R { id: u8 }");
        assert!(text.contains("field`id`"), "{text}");
        assert!(text.contains("allowed:"), "{text}");
    }

    #[test]
    fn an_item_that_is_not_a_struct_is_only_refused() {
        let text = flat(&expand(
            TokenStream::new(),
            quote!(
                fn f() {}
            ),
        ));
        assert!(text.starts_with("::core::compile_error!"), "{text}");
        assert!(!text.contains("fnf"), "{text}");
    }
}
