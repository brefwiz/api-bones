// SPDX-License-Identifier: MIT
//! The uniffi exports of a `#[client]` impl, for Kotlin and Swift.
//!
//! With the consumer's feature `uniffi` on a native target, a client type `T`
//! gets an object `TFfi` and each export becomes a free function, over the same
//! protobuf messages the wasm exports use:
//!
//! - The constructor is `TFfi::new(base_url, host, options)`. It refuses a
//!   `base_url` the host boundary does not allow, and an `options` that does not
//!   fit the client, before `T::from_ffi_host(base_url, host[, options])` runs;
//!   so no credential is read for a refused address. `host` is an
//!   `api_bones_sdk_ffi::FfiHost`, and `options` is empty for a client without
//!   options.
//! - An async export is a free `async fn <symbol>(client, args, ctx)` on the
//!   tokio runtime, returning the result bytes or an
//!   `api_bones_sdk_ffi::FfiError`.
//! - A list or batch export is a plain function that returns an
//!   `api_bones_sdk_ffi::FfiHandle`; a stream export is async, because opening
//!   a stream awaits the call. Each takes `(client, args, ctx)`.
//! - A sync export and a constant are plain functions; a constant takes no
//!   arguments, and neither has a context parameter.
//!
//! `ctx` is an `api_bones_sdk_ffi::FfiCallCtx`: the call's trace fields and,
//! for a list, its prefetch and first-page-only reads, the same fields the wasm
//! export takes as parameters. It belongs to the call, never to the client or
//! the host. The call runs under `CallCtx` built from it.
//!
//! `<symbol>` is the wasm export name in lower case with single underscores.
//! The consumer calls `uniffi::setup_scaffolding!` once.

use std::collections::BTreeSet;

use api_bones_sdk_model::{Client, Export, HandleKind, Input, Return, Shape, Wire, export_name};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::common::private;
use crate::wasm::{
    Home, Member, call_of, encode, guards, message_path, messages_of, named, sync_outcome_of,
};

/// The attribute that compiles an item only on a native target with the
/// consumer's feature `uniffi` and one of the export features on.
fn gate(features: &BTreeSet<String>) -> TokenStream {
    let features = features.iter();
    quote! {
        #[cfg(all(
            feature = "uniffi",
            not(target_arch = "wasm32"),
            any(#(feature = #features),*)
        ))]
    }
}

/// The name of an export's function in the binding.
fn symbol(service: &str, ty: &str, method: &str) -> syn::Result<syn::Ident> {
    let js = export_name(service, ty, method).map_err(|error| named(&error))?;
    let mut name = js.to_lowercase();
    while name.contains("__") {
        name = name.replace("__", "_");
    }
    syn::parse_str(&name).map_err(|error| named(&error))
}

/// What the expansion of one export needs to know.
struct Shell<'a> {
    home: &'a Home<'a>,
    ffi: &'a syn::Ident,
    export: &'a Export,
    function: &'a Member,
}

/// The uniffi items of one client: its object and constructor, and the free
/// functions of its exports.
pub(crate) fn expand(
    service: &str,
    client: &Client,
    home: &Home<'_>,
    functions: &[Member],
) -> syn::Result<TokenStream> {
    let ffi = format_ident!("{}Ffi", client.ty);
    let mut all = BTreeSet::new();
    all.extend(
        api_bones_sdk_model::gate_features(&client.surface, "")[..2]
            .iter()
            .cloned(),
    );
    let mut items = Vec::new();
    for (export, function) in client.exports.iter().zip(functions) {
        all.extend(api_bones_sdk_model::gate_features(
            &export.surface,
            &export.method,
        ));
        let shell = Shell {
            home,
            ffi: &ffi,
            export,
            function,
        };
        let name = symbol(service, &client.ty, &export.method)?;
        items.push(shell.expand(&name)?);
    }
    let object = match home {
        Home::Impl(self_ty) => object(client, &ffi, self_ty, &gate(&all))?,
        Home::Module(_) => TokenStream::new(),
    };
    Ok(quote! {
        #object
        #(#items)*
    })
}

/// The object of a client and its constructor.
fn object(
    client: &Client,
    ffi: &syn::Ident,
    self_ty: &syn::Type,
    gate: &TokenStream,
) -> syn::Result<TokenStream> {
    let private = private();
    let (options, options_arg, options_guard) = match &client.options {
        None => (
            quote!(#private::no_options(&options)?;),
            TokenStream::new(),
            TokenStream::new(),
        ),
        Some(message) => (
            TokenStream::new(),
            quote!(, #private::decode_options(&options)?),
            guards(gate, [&Wire::Message(message.clone())])?,
        ),
    };
    Ok(quote! {
        #options_guard

        #gate
        #[derive(::uniffi::Object)]
        pub struct #ffi {
            inner: ::std::sync::Arc<#self_ty>,
        }

        #gate
        #[::uniffi::export]
        impl #ffi {
            #[::uniffi::constructor]
            pub fn new(
                base_url: ::std::string::String,
                host: ::std::sync::Arc<dyn ::api_bones_sdk_ffi::FfiHost>,
                options: ::std::vec::Vec<u8>,
            ) -> ::core::result::Result<::std::sync::Arc<Self>, ::api_bones_sdk_ffi::FfiError> {
                (|| -> ::core::result::Result<#self_ty, ::api_bones_sdk::Error> {
                    ::api_bones_sdk_ffi::check_base_url(&base_url)?;
                    #options
                    <#self_ty>::from_ffi_host(base_url, host #options_arg)
                        .map_err(::core::convert::Into::<::api_bones_sdk::Error>::into)
                })()
                .map(|__client| ::std::sync::Arc::new(Self {
                    inner: ::std::sync::Arc::new(__client),
                }))
                .map_err(::api_bones_sdk_ffi::FfiError::from)
            }
        }
    })
}

impl Shell<'_> {
    fn expand(&self, name: &syn::Ident) -> syn::Result<TokenStream> {
        let features: BTreeSet<String> =
            api_bones_sdk_model::gate_features(&self.export.surface, &self.export.method)
                .into_iter()
                .collect();
        let gate = gate(&features);
        let reads = self.decode()?;
        let call = self.call();
        let shell = if self.export.shape == Shape::Constant {
            self.constant_shell(name, &call)?
        } else if self.export.sync {
            self.sync_shell(name, &reads, &call)?
        } else if let Return::Handle { kind, .. } = &self.export.ret {
            self.handle_shell(name, &reads, &call, *kind)?
        } else {
            self.async_shell(name, &reads, &call)?
        };
        let guards = guards(&gate, self.messages())?;
        Ok(quote! {
            #gate
            #shell

            #guards
        })
    }

    /// The call of the Rust item: a method of the shared instance, an
    /// associated function or constant of the client type, or an item of the
    /// client module. A message taken whole is passed as `__req` or `&__req`.
    fn call(&self) -> TokenStream {
        if let Input::Message(param) = &self.export.input {
            let arg = if param.by_ref {
                quote!(&__req)
            } else {
                quote!(__req)
            };
            return call_of(self.home, self.function, self.export, &[arg]);
        }
        let args: Vec<TokenStream> = self
            .function
            .params
            .iter()
            .zip(self.export.input.params())
            .map(|(name, param)| {
                if param.by_ref {
                    quote!(&#name)
                } else {
                    quote!(#name)
                }
            })
            .collect();
        call_of(self.home, self.function, self.export, &args)
    }

    /// The messages the export's arguments and its result cross as.
    fn messages(&self) -> Vec<&Wire> {
        messages_of(self.export)
    }

    /// The statement that reads the arguments message, binding each parameter
    /// by the name of the field it is; a message taken whole is decoded once
    /// into `__req`.
    fn decode(&self) -> syn::Result<TokenStream> {
        let private = private();
        match &self.export.args {
            Wire::Empty => Ok(quote!(#private::no_args(&args)?;)),
            Wire::Message(message) => {
                let path = message_path(message)?;
                if matches!(self.export.input, Input::Message(_)) {
                    return Ok(quote! {
                        let __req = #private::decode_strict::<#path>(&args)?;
                    });
                }
                let names = &self.function.params;
                Ok(quote! {
                    let #path { #(#names,)* .. } =
                        #private::decode_strict::<#path>(&args)?;
                })
            }
            Wire::Wrapper(_) => Err(named(&format!(
                "export `{}`: its arguments arrive as a message of its own",
                self.export.method
            ))),
        }
    }

    /// The parameters of a shell: the client for a method, then the arguments,
    /// then the call's context for a shell that has one.
    fn params(&self, with_ctx: bool) -> TokenStream {
        let ffi = self.ffi;
        let ctx = with_ctx.then(|| quote!(, ctx: ::api_bones_sdk_ffi::FfiCallCtx));
        match self.export.shape {
            Shape::Method => {
                quote!(client: ::std::sync::Arc<#ffi>, args: ::std::vec::Vec<u8> #ctx)
            }
            Shape::Function | Shape::Constant => quote!(args: ::std::vec::Vec<u8> #ctx),
        }
    }

    /// The binding of the shared instance a method reads.
    fn instance(&self) -> TokenStream {
        match self.export.shape {
            Shape::Method => quote!(let __client = ::std::sync::Arc::clone(&client.inner);),
            Shape::Function | Shape::Constant => TokenStream::new(),
        }
    }

    fn failed() -> TokenStream {
        quote!(::api_bones_sdk_ffi::FfiError)
    }

    /// A constant: no arguments, the encoded value.
    fn constant_shell(&self, name: &syn::Ident, call: &TokenStream) -> syn::Result<TokenStream> {
        let Return::Value(wire) = &self.export.ret else {
            return Err(named("a constant crosses as a value"));
        };
        let encoded = encode(wire, &quote!(&__out));
        Ok(quote! {
            #[::uniffi::export]
            pub fn #name() -> ::std::vec::Vec<u8> {
                let __out = #call;
                #encoded
            }
        })
    }

    /// A synchronous export: the arguments in, the result out.
    fn sync_shell(
        &self,
        name: &syn::Ident,
        reads: &TokenStream,
        call: &TokenStream,
    ) -> syn::Result<TokenStream> {
        let params = self.params(false);
        let instance = self.instance();
        let failed = Self::failed();
        let outcome = self.sync_outcome(call)?;
        Ok(quote! {
            #[::uniffi::export]
            pub fn #name(#params) -> ::core::result::Result<::std::vec::Vec<u8>, #failed> {
                #instance
                (|| -> ::core::result::Result<::std::vec::Vec<u8>, ::api_bones_sdk::Error> {
                    #reads
                    #outcome
                })()
                .map_err(#failed::from)
            }
        })
    }

    fn sync_outcome(&self, call: &TokenStream) -> syn::Result<TokenStream> {
        sync_outcome_of(self.export, call)
    }

    /// An async export that resolves to one value.
    fn async_shell(
        &self,
        name: &syn::Ident,
        reads: &TokenStream,
        call: &TokenStream,
    ) -> syn::Result<TokenStream> {
        let Return::Result { ok, .. } = &self.export.ret else {
            return Err(named("an async export returns Result<T, E> or a handle"));
        };
        let params = self.params(true);
        let instance = self.instance();
        let failed = Self::failed();
        let encoded = encode(ok, &quote!(&__out));
        Ok(quote! {
            #[::uniffi::export(async_runtime = "tokio")]
            pub async fn #name(#params) -> ::core::result::Result<::std::vec::Vec<u8>, #failed> {
                #instance
                let __ctx = ctx.call_ctx(false);
                ::api_bones_sdk::CallCtx::scope(__ctx, async move {
                    #reads
                    let __out = #call
                        .await
                        .map_err(::core::convert::Into::<::api_bones_sdk::Error>::into)?;
                    ::core::result::Result::Ok::<::std::vec::Vec<u8>, ::api_bones_sdk::Error>(#encoded)
                })
                .await
                .map_err(#failed::from)
            }
        })
    }

    /// An export that returns a handle: a list or a batch at once, a stream
    /// once the call that opens it was awaited.
    fn handle_shell(
        &self,
        name: &syn::Ident,
        reads: &TokenStream,
        call: &TokenStream,
        kind: HandleKind,
    ) -> syn::Result<TokenStream> {
        let private = private();
        let Return::Handle { item, .. } = &self.export.ret else {
            return Err(named("a handle export returns a handle"));
        };
        if matches!(item, Wire::Empty) {
            return Err(named("a handle yields a message or a scalar"));
        }
        let params = self.params(true);
        let instance = self.instance();
        let failed = Self::failed();
        let list = kind == HandleKind::List;
        let each = encode(item, &quote!(__item));
        let next = match kind {
            HandleKind::List => quote!(#private::encode_page(__page, |__item| #each)),
            HandleKind::Stream => encode(item, &quote!(__page)),
            HandleKind::Batch => quote!(#private::encode_batch_chunk(__page, |__item| #each)),
        };
        // A list or a batch starts its call without awaiting, so its context
        // is installed around the start; a stream awaits the call that opens
        // it, so the context is installed around that future.
        let (asyncness, opened, runtime) = if kind.is_async() {
            (
                quote!(async),
                quote! {
                    ::api_bones_sdk::CallCtx::scope(__ctx, async { #call.await }).await
                },
                quote!(#[::uniffi::export(async_runtime = "tokio")]),
            )
        } else {
            (
                TokenStream::new(),
                quote!(::api_bones_sdk::CallCtx::with(__ctx, || #call)),
                quote!(#[::uniffi::export]),
            )
        };
        Ok(quote! {
            #runtime
            pub #asyncness fn #name(#params)
                -> ::core::result::Result<::std::sync::Arc<::api_bones_sdk_ffi::FfiHandle>, #failed>
            {
                #instance
                let __ctx = ctx.call_ctx(#list);
                #reads
                let __handle = #opened;
                ::core::result::Result::Ok(::api_bones_sdk_ffi::FfiHandle::new(
                    __handle,
                    |__page| #next,
                ))
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use api_bones_sdk_model::Model;

    use super::*;
    use crate::common::{expand_first_impl, flat};

    const SOURCE: &str = crate::common::numbers_client!(
        "(&self, limit: u32)",
        "#[export]",
        "",
        r#"
            #[export(sync)]
            pub const BASE: u32 = 10;
        }
    "#
    );

    fn built(source: &str, change: impl FnOnce(&mut Client)) -> syn::Result<TokenStream> {
        expand_first_impl(source, change, |client, home, functions| {
            expand("api-bones", client, home, functions)
        })
    }

    fn text() -> String {
        flat(&built(SOURCE, |_| {}).expect("expanded"))
    }

    /// Every identifier in `tokens`, groups included.
    fn idents(tokens: TokenStream, out: &mut Vec<String>) {
        for tree in tokens {
            match tree {
                proc_macro2::TokenTree::Ident(ident) => out.push(ident.to_string()),
                proc_macro2::TokenTree::Group(group) => idents(group.stream(), out),
                _ => {}
            }
        }
    }

    #[test]
    fn the_expansion_holds_no_unsafe_identifier_for_any_member_kind() {
        let mut found = Vec::new();
        idents(built(SOURCE, |_| {}).expect("expanded"), &mut found);
        assert!(found.iter().any(|ident| ident == "uniffi"), "{found:?}");
        for forbidden in ["unsafe", "no_mangle", "extern"] {
            assert!(
                !found.iter().any(|ident| ident == forbidden),
                "the expansion names `{forbidden}`"
            );
        }
    }

    #[test]
    fn the_expansion_is_valid_rust() {
        syn::parse2::<syn::File>(built(SOURCE, |_| {}).expect("expanded")).expect("a file");
    }

    #[test]
    fn the_client_is_an_object_named_after_its_type() {
        let text = text();
        assert!(
            text.contains("#[derive(::uniffi::Object)]pubstructNumbersClientFfi{"),
            "{text}"
        );
        assert!(
            text.contains("inner:::std::sync::Arc<NumbersClient>,"),
            "{text}"
        );
    }

    #[test]
    fn every_item_is_native_and_feature_gated() {
        let text = text();
        assert!(
            text.contains(
                "#[cfg(all(feature=\"uniffi\",not(target_arch=\"wasm32\"),any(feature=\"sdk-all\""
            ),
            "{text}"
        );
        assert!(!text.contains("target_arch=\"wasm32\",any"), "{text}");
    }

    #[test]
    fn the_constructor_checks_the_address_before_it_builds_the_client() {
        let text = text();
        let check = text.find("check_base_url(&base_url)?;").expect("a check");
        let build = text.find("from_ffi_host(base_url,host").expect("a build");
        assert!(check < build, "{text}");
        assert!(
            text.contains("host:::std::sync::Arc<dyn::api_bones_sdk_ffi::FfiHost>"),
            "{text}"
        );
        assert!(text.contains("options:::std::vec::Vec<u8>"), "{text}");
    }

    #[test]
    fn a_client_without_options_refuses_non_empty_options() {
        let text = text();
        assert!(
            text.contains("no_options(&options)?;from_ffi_host(base_url,host)")
                || (text.contains("no_options(&options)?;")
                    && text.contains("from_ffi_host(base_url,host)")),
            "{text}"
        );
        assert!(!text.contains("decode_options"), "{text}");
    }

    #[test]
    fn a_client_with_options_decodes_them_into_the_constructor() {
        let text = flat(
            &built(SOURCE, |client| {
                client.options = Some(api_bones_sdk_model::ProtoRef {
                    path: "generated::contract::sdk::v1::NumbersOptions".to_owned(),
                    full_name: "contract.sdk.v1.NumbersOptions".to_owned(),
                });
            })
            .expect("expanded"),
        );
        assert!(
            text.contains(
                "from_ffi_host(base_url,host,::api_bones_sdk::__private::decode_options(&options)?)"
            ),
            "{text}"
        );
        assert!(!text.contains("no_options"), "{text}");
    }

    #[test]
    fn an_async_export_runs_on_the_tokio_runtime() {
        let text = text();
        assert!(
            text.contains(
                "#[::uniffi::export(async_runtime=\"tokio\")]pubasyncfnapi_bones_numbersclient_get("
            ),
            "{text}"
        );
        assert!(
            text.contains("client:::std::sync::Arc<NumbersClientFfi>,args:::std::vec::Vec<u8>"),
            "{text}"
        );
        assert!(text.contains("__client.get(id).await"), "{text}");
    }

    #[test]
    fn a_failure_crosses_as_the_ffi_error() {
        let text = text();
        assert!(
            text.contains(".map_err(::api_bones_sdk_ffi::FfiError::from)"),
            "{text}"
        );
    }

    #[test]
    fn a_list_and_a_batch_are_plain_functions_and_a_stream_is_async() {
        let text = text();
        assert!(
            text.contains("#[::uniffi::export]pubfnapi_bones_numbersclient_list("),
            "{text}"
        );
        assert!(
            text.contains("#[::uniffi::export]pubfnapi_bones_numbersclient_get_all("),
            "{text}"
        );
        assert!(
            text.contains("pubasyncfnapi_bones_numbersclient_watch("),
            "{text}"
        );
        assert!(text.contains("__client.watch().await"), "{text}");
        assert!(
            text.contains("::api_bones_sdk_ffi::FfiHandle::new(__handle,|__page|"),
            "{text}"
        );
        assert!(text.contains("encode_page(__page,"), "{text}");
        assert!(text.contains("encode_batch_chunk(__page,"), "{text}");
    }

    #[test]
    fn a_sync_export_is_a_plain_function_and_a_constant_takes_nothing() {
        let text = text();
        assert!(
            text.contains("#[::uniffi::export]pubfnapi_bones_numbersclient_label("),
            "{text}"
        );
        assert!(
            text.contains(
                "#[::uniffi::export]pubfnapi_bones_numbersclient_base()->::std::vec::Vec<u8>"
            ),
            "{text}"
        );
    }

    /// The text of the export named `name`, up to the next export.
    fn export_of(text: &str, name: &str) -> String {
        let start = text
            .find(name)
            .unwrap_or_else(|| panic!("{name} in {text}"));
        let rest = &text[start..];
        let end = rest[name.len()..]
            .find("#[::uniffi::export")
            .map_or(rest.len(), |at| at + name.len());
        rest[..end].to_owned()
    }

    #[test]
    fn a_call_context_is_the_last_parameter_of_every_async_list_batch_and_stream_export() {
        let text = text();
        for name in [
            "api_bones_numbersclient_get(",
            "api_bones_numbersclient_list(",
            "api_bones_numbersclient_get_all(",
            "api_bones_numbersclient_watch(",
        ] {
            let export = export_of(&text, name);
            assert!(
                export.contains(
                    "client:::std::sync::Arc<NumbersClientFfi>,args:::std::vec::Vec<u8>,ctx:::api_bones_sdk_ffi::FfiCallCtx"
                ),
                "{name}: {export}"
            );
        }
    }

    #[test]
    fn a_sync_export_and_a_constant_take_no_call_context() {
        let text = text();
        for name in [
            "api_bones_numbersclient_label(",
            "api_bones_numbersclient_base(",
        ] {
            let export = export_of(&text, name);
            assert!(!export.contains("FfiCallCtx"), "{name}: {export}");
            assert!(!export.contains("CallCtx"), "{name}: {export}");
        }
    }

    #[test]
    fn an_async_export_runs_under_the_context_of_its_call() {
        let export = export_of(&text(), "api_bones_numbersclient_get(");
        assert!(
            export.contains(
                "let__ctx=ctx.call_ctx(false);::api_bones_sdk::CallCtx::scope(__ctx,asyncmove{"
            ),
            "{export}"
        );
        assert!(export.contains("__client.get(id).await"), "{export}");
    }

    #[test]
    fn a_list_starts_under_its_context_with_the_reads_it_chose() {
        let export = export_of(&text(), "api_bones_numbersclient_list(");
        assert!(export.contains("let__ctx=ctx.call_ctx(true);"), "{export}");
        assert!(
            export.contains("let__handle=::api_bones_sdk::CallCtx::with(__ctx,||"),
            "{export}"
        );
        assert!(!export.contains("CallCtx::scope"), "{export}");
    }

    #[test]
    fn a_batch_starts_under_its_context_without_the_list_reads() {
        let export = export_of(&text(), "api_bones_numbersclient_get_all(");
        assert!(export.contains("let__ctx=ctx.call_ctx(false);"), "{export}");
        assert!(
            export.contains("let__handle=::api_bones_sdk::CallCtx::with(__ctx,||"),
            "{export}"
        );
    }

    #[test]
    fn a_stream_opens_under_its_context_without_the_list_reads() {
        let export = export_of(&text(), "api_bones_numbersclient_watch(");
        assert!(export.contains("let__ctx=ctx.call_ctx(false);"), "{export}");
        assert!(
            export.contains(
                "let__handle=::api_bones_sdk::CallCtx::scope(__ctx,async{__client.watch().await}).await;"
            ),
            "{export}"
        );
    }

    #[test]
    fn the_context_is_read_before_the_arguments_can_shadow_it() {
        let export = export_of(&text(), "api_bones_numbersclient_list(");
        let ctx = export.find("ctx.call_ctx(").expect("a context");
        let reads = export.find("decode_strict").expect("a read");
        assert!(ctx < reads, "{export}");
    }

    #[test]
    fn the_context_is_never_a_field_of_the_client_or_a_parameter_of_its_constructor() {
        let text = text();
        let object = export_of(&text, "pubstructNumbersClientFfi");
        assert!(!object.contains("ctx"), "{object}");
        let constructor = export_of(&text, "pubfnnew(");
        assert!(!constructor.contains("ctx"), "{constructor}");
        assert!(!constructor.contains("CallCtx"), "{constructor}");
    }

    #[test]
    fn a_module_has_functions_and_no_object() {
        let source = r#"
            #[client(surface = "s", proto = generated::contract::sdk::v1)]
            mod digits {
                #[export(sync)] pub fn doubled(value: u32) -> u32 { body() }
            }
        "#;
        let file: syn::File = syn::parse_str(source).expect("a file");
        let model = Model::from_files([&file]).expect("a model");
        let syn::Item::Mod(module) = &file.items[0] else {
            panic!("a module");
        };
        let functions = vec![Member::of_fn(
            &syn::parse_quote!(fn doubled(value: u32) -> u32),
        )];
        let text = flat(
            &expand(
                "api-bones",
                &model.clients[0],
                &Home::Module(&module.ident),
                &functions,
            )
            .expect("expanded"),
        );
        assert!(!text.contains("uniffi::Object"), "{text}");
        assert!(
            text.contains("pubfnapi_bones_digits_doubled(args:::std::vec::Vec<u8>)"),
            "{text}"
        );
        assert!(text.contains("self::digits::doubled(value)"), "{text}");
    }

    const ECHO: &str = "#[export(sync, args = generated::contract::v1::Echo)]\n            pub fn echo(request: generated::contract::v1::Echo) -> generated::contract::v1::Echo { body() }";

    fn whole() -> String {
        flat(&built(&crate::common::whole_message_client(ECHO), |_| {}).expect("expanded"))
    }

    #[test]
    fn an_export_taking_its_message_whole_decodes_once_and_passes_it() {
        let export = export_of(&whole(), "api_bones_c_derive(");
        assert!(
            export.contains("let__req=::api_bones_sdk::__private::decode_strict::<generated::contract::sdk::v1::DeriveArgs>(&args)?;"),
            "{export}"
        );
        assert!(export.contains("__client.derive(__req).await"), "{export}");
        assert!(!export.contains("DeriveArgs{request"), "{export}");
    }

    #[test]
    fn a_whole_message_taken_by_reference_is_passed_by_reference() {
        let export = export_of(&whole(), "api_bones_c_inspect(");
        assert!(
            export.contains("let__req=::api_bones_sdk::__private::decode_strict::<generated::contract::v1::Inspect>(&args)?;"),
            "{export}"
        );
        assert!(export.contains("__client.inspect(&__req)"), "{export}");
    }

    #[test]
    fn an_associated_function_taking_its_message_whole_is_called_on_the_type() {
        let export = export_of(&whole(), "api_bones_c_echo(");
        assert!(
            export.starts_with("api_bones_c_echo(args:::std::vec::Vec<u8>)"),
            "{export}"
        );
        assert!(export.contains("<C>::echo(__req)"), "{export}");
        assert!(!export.contains("Echo{request"), "{export}");
    }

    #[test]
    fn a_service_name_the_wasm_export_refuses_is_refused() {
        assert!(symbol("a__b", "T", "m").is_err());
    }
}
