// SPDX-License-Identifier: MIT
//! The wasm32 exports of a `#[client]` impl.
//!
//! Each client becomes a wasm class named `<svc>__<Type>`. Its constructor
//! export is `<svc>__<Type>__new(base_url, host, options, wrapper, runtime)`,
//! built from the type's `from_host(base_url: String, host: JsValue, ..) ->
//! Result<Self, Error>`. `options` is a `Uint8Array` holding the options
//! message; a `from_host` takes that message as its one further parameter, and
//! a client without one takes an empty `options`. `wrapper` and `runtime` are
//! the boundary versions the generated wrapper and the runtime were built for;
//! the constructor refuses with `SDK_BOUNDARY_MISMATCH` when either is not
//! `api_bones_sdk::BOUNDARY`. The class also has two static exports that need
//! no instance: `__api_bones_sdk_boundary()`, the plain `u32` boundary version,
//! readable across a mismatch, and `__api_bones_sdk_build_info()`, the
//! `bones.sdk.v1.BuildInfo` bytes. Each export is a method named
//! `<svc>__<Type>__<method>`, gated by the features `sdk-all`, `sdk-<surface>`
//! and `sdk-m-<method>`.
//!
//! No export takes or returns JSON text. Every value that crosses is a
//! `Uint8Array` holding the protobuf encoding of a message, and every export is
//! a thin shell over the Rust method:
//!
//! - The arguments are one message declared in proto, named by the model
//!   (`#[client(proto = ..)]` gives `<proto>::<Method>Args`, and
//!   `#[export(args = ..)]` names another). The shell decodes it strictly and
//!   binds each parameter by the name of the message field it is, so a missing,
//!   renamed or mistyped field fails to compile rather than crossing wrongly.
//!   An export without parameters takes the empty message. A result is the
//!   message itself; a scalar result is its `google.protobuf` wrapper message
//!   and a unit result is the empty message. Each message path is checked
//!   against the proto name the model recorded, in a constant.
//! - An async export takes `(args, traceparent, tracestate, signal)` and
//!   returns a `Promise`. The call runs inside `CallCtx::scope`, so the call
//!   helper reads the caller's trace fields. The promise resolves to the
//!   result bytes and rejects with `bones.sdk.v1.CallError` bytes.
//! - An export that returns a handle returns, at once, a class with `next()`,
//!   `cancel()` and `span()`; the call that opens the handle runs on the first
//!   `next()`, an abort of `signal` cancels the handle, and `span()` is
//!   undefined until the handle opened. A list export has two more typed
//!   parameters, `prefetch` (undefined or a count; a value above 1 reads one
//!   ahead), which the call helper reads back from `CallCtx` as the pages the
//!   list fetches ahead, and `firstOnly`, which makes the list deliver its
//!   first page alone. `next()` resolves to `bones.sdk.v1.Page` bytes for a list, the item message for a stream and
//!   `bones.sdk.v1.BatchChunk` bytes for a batch.
//! - A sync export takes `args` and returns the result bytes or throws the
//!   `CallError` bytes.
//!
//! Everything the code touches outside the crate is reached through
//! `api_bones_sdk::__private`.

use std::collections::BTreeSet;

use api_bones_sdk_model::{
    Client, Export, HandleKind, ProtoRef, Return, Shape, Wire, export_name, gate_features,
};
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{FnArg, Ident, Pat, Signature, Type};

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

/// A `Uint8Array` holding `bytes`, a `Vec<u8>` expression.
fn uint8(bytes: &TokenStream) -> TokenStream {
    let private = private();
    quote!(#private::js_sys::Uint8Array::from(#bytes.as_slice()))
}

/// The `uint8` array as a `JsValue`.
fn js_bytes(bytes: &TokenStream) -> TokenStream {
    let private = private();
    let array = uint8(bytes);
    quote!(#private::wasm_bindgen::JsValue::from(#array))
}

/// The `JsValue` an export throws or rejects with for `error`, an
/// `&api_bones_sdk::Error` expression: the `bones.sdk.v1.CallError` bytes.
fn rejection(error: &TokenStream) -> TokenStream {
    let private = private();
    js_bytes(&quote!(#private::encode_call_error(#error)))
}

/// The Rust path of a message the model recorded.
fn message_path(message: &ProtoRef) -> syn::Result<syn::Path> {
    syn::parse_str(&message.path).map_err(|error| named(&error))
}

/// The bytes of the value behind `value`, an expression of type `&T`, as the
/// message `wire` names: a message as itself, a scalar in its
/// `google.protobuf` wrapper, and no value as the empty message.
fn encode(wire: &Wire, value: &TokenStream) -> TokenStream {
    let private = private();
    match wire {
        Wire::Empty => quote!({
            let () = *#value;
            ::std::vec::Vec::new()
        }),
        Wire::Wrapper(_) => quote!(#private::encode_wrapped(#value)),
        Wire::Message(_) => quote!(#private::encode_message(#value)),
    }
}

/// A constant that does not compile unless the generated type at `message`'s
/// path is the proto message the model recorded, so the Rust side and the
/// descriptor the generator reads cannot name different messages.
fn guard(gate: &TokenStream, message: &ProtoRef) -> syn::Result<TokenStream> {
    let private = private();
    let path = message_path(message)?;
    let name = &message.full_name;
    Ok(quote! {
        #gate
        const _: () = ::core::assert!(
            #private::same_name(<#path as #private::MessageName>::FULL_NAME, #name),
            "the message a path names is not the one the model recorded",
        );
    })
}

/// The guards of every message `wires` name.
fn guards<'a>(
    gate: &TokenStream,
    wires: impl IntoIterator<Item = &'a Wire>,
) -> syn::Result<TokenStream> {
    let mut out = TokenStream::new();
    for wire in wires {
        if let Wire::Message(message) = wire {
            out.extend(guard(gate, message)?);
        }
    }
    Ok(out)
}

/// The pieces of one export's shell that every kind of shell shares.
struct Parts {
    js: String,
    shell: syn::Ident,
    reads: TokenStream,
    call: TokenStream,
}

/// The parameters of an async shell: the arguments, the trace fields, the
/// signal, and for a list the two reads it chooses.
fn async_params(list: bool) -> TokenStream {
    let private = private();
    let signal = quote!(#private::web_sys::AbortSignal);
    let bytes = quote!(#private::js_sys::Uint8Array);
    let list = list.then(|| {
        quote! {
            __prefetch: ::core::option::Option<u8>,
            __first_only: bool,
        }
    });
    quote! {
        &self,
        __args: #bytes,
        __traceparent: ::core::option::Option<::std::string::String>,
        __tracestate: ::core::option::Option<::std::string::String>,
        __signal: ::core::option::Option<#signal>,
        #list
    }
}

/// The call of an async export, run under the caller's trace fields and, for a
/// list, the reads it chose: `reads` parse the arguments, then `body` runs.
fn scoped(list: bool, reads: &TokenStream, body: &TokenStream) -> TokenStream {
    let (prefetch, first_only) = if list {
        (quote!(__prefetch), quote!(__first_only))
    } else {
        (quote!(::core::option::Option::None), quote!(false))
    };
    quote! {
        ::api_bones_sdk::CallCtx::scope(
            ::api_bones_sdk::CallCtx {
                traceparent: __traceparent,
                tracestate: __tracestate,
                prefetch: #prefetch,
                first_only: #first_only,
            },
            async move {
                #reads
                #body
            },
        )
    }
}

/// What the shell of an export reads of its Rust item: the name, the names of
/// its parameters and the type it returns. A method, a function and a constant
/// are all read through it.
pub(crate) struct Member {
    ident: Ident,
    params: Vec<Ident>,
    pub(crate) output: Option<Type>,
}

impl Member {
    /// A method or function, from its signature.
    pub(crate) fn of_fn(sig: &Signature) -> Self {
        let params = sig
            .inputs
            .iter()
            .filter_map(|input| match input {
                FnArg::Typed(typed) => match &*typed.pat {
                    Pat::Ident(ident) => Some(ident.ident.clone()),
                    _ => None,
                },
                FnArg::Receiver(_) => None,
            })
            .collect();
        let output = match &sig.output {
            syn::ReturnType::Type(_, ty) => Some((**ty).clone()),
            syn::ReturnType::Default => None,
        };
        Self {
            ident: sig.ident.clone(),
            params,
            output,
        }
    }

    /// A constant: no parameter, its type is what it returns.
    pub(crate) fn of_const(ident: &Ident, ty: &Type) -> Self {
        Self {
            ident: ident.clone(),
            params: Vec::new(),
            output: Some(ty.clone()),
        }
    }
}

/// Where a client's receiver-less exports live: an associated item of the
/// impl's type, or an item of the client module.
pub(crate) enum Home<'a> {
    /// The type of a `#[client]` impl.
    Impl(&'a Type),
    /// The name of a `#[client]` module, a sibling of the expansion.
    Module(&'a Ident),
}

/// What the expansion of one export needs to know.
struct Shell<'a> {
    service: &'a str,
    client: &'a Client,
    home: &'a Home<'a>,
    class_ident: &'a syn::Ident,
    class: &'a str,
    export: &'a Export,
    function: &'a Member,
}

/// The wasm items of one client: its class, its constructor, boundary version
/// and build info, and the methods and handle classes of its exports.
///
/// They sit in one anonymous block that imports the `wasm_bindgen` the SDK
/// re-exports, so the plain `wasm_bindgen::` paths the wasm-bindgen expansions
/// emit resolve in a consumer without a direct dependency. A block, unlike a
/// module, leaves `self::` and `super::` meaning what they mean at the call
/// site, and one block per client keeps two clients of a module apart.
pub(crate) fn expand(
    service: &str,
    client: &Client,
    home: &Home<'_>,
    functions: &[Member],
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
            home,
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
    let (options, options_arg, options_guard) = match &client.options {
        None => (
            quote!(#private::no_options(&__options)?;),
            TokenStream::new(),
            TokenStream::new(),
        ),
        Some(message) => (
            TokenStream::new(),
            quote!(, #private::decode_options(&__options)?),
            guard(&TokenStream::new(), message)?,
        ),
    };
    let class_items = match home {
        Home::Impl(self_ty) => {
            let rejected = rejection(&quote!(&__error));
            let build_info = uint8(&quote! {
                #private::encode_build_info(
                    ::core::env!("CARGO_PKG_NAME"),
                    ::core::env!("CARGO_PKG_VERSION"),
                )
            });
            quote! {
                #options_guard

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
                        options: #private::js_sys::Uint8Array,
                        wrapper: u32,
                        runtime: u32,
                    ) -> ::core::result::Result<Self, #private::wasm_bindgen::JsValue> {
                        (|| -> ::core::result::Result<#self_ty, ::api_bones_sdk::Error> {
                            #private::check_boundary(wrapper, runtime)?;
                            let __options = options.to_vec();
                            #options
                            <#self_ty>::from_host(base_url, host #options_arg)
                                .map_err(::core::convert::Into::<::api_bones_sdk::Error>::into)
                        })()
                        .map(|__client| Self { inner: ::std::sync::Arc::new(__client) })
                        .map_err(|__error| #rejected)
                    }

                    #[#method(js_name = "__api_bones_sdk_boundary")]
                    pub fn boundary_js() -> u32 {
                        ::api_bones_sdk::BOUNDARY
                    }

                    #[#method(js_name = "__api_bones_sdk_build_info")]
                    pub fn build_info_js() -> #private::js_sys::Uint8Array {
                        #build_info
                    }
                }
            }
        }
        // A module has no instance: its class only carries the static exports,
        // and the `Default` derive is the one place the empty struct is built.
        Home::Module(_) => quote! {
            #[derive(::core::default::Default)]
            #struct_attr
            pub struct #class_ident {}
        },
    };
    Ok(quote! {
        #class_gate
        const _: () = {
        use #private::wasm_bindgen;

        #class_items

        #(#methods)*
        };
    })
}

impl Shell<'_> {
    fn expand(&self) -> syn::Result<TokenStream> {
        let js = export_name(self.service, &self.client.ty, &self.export.method)
            .map_err(|error| named(&error))?;
        let features: BTreeSet<String> = gate_features(&self.export.surface, &self.export.method)
            .into_iter()
            .collect();
        let gate = gate(&features);
        let method = &self.function.ident;
        let impl_attr = impl_attr(self.class);
        let class_ident = self.class_ident;
        let decode = self.decode()?;
        let call_args = self.call_args();
        let parts = Parts {
            js,
            shell: match self.export.shape {
                Shape::Constant => format_ident!("{}_const_js", method.to_string().to_lowercase()),
                Shape::Method | Shape::Function => format_ident!("{}_js", method),
            },
            reads: quote! {
                #decode
            },
            call: self.call(&call_args),
        };
        let (shell, extra) = if self.export.sync {
            (self.sync_shell(&parts)?, TokenStream::new())
        } else if let Return::Handle { kind, item } = &self.export.ret {
            self.handle_shell(&parts, *kind, item)?
        } else {
            (self.async_shell(&parts)?, TokenStream::new())
        };
        let guards = guards(&gate, self.messages())?;
        Ok(quote! {
            #gate
            #impl_attr
            impl #class_ident {
                #shell
            }

            #extra

            #guards
        })
    }

    /// The call of the Rust item: a method of the shared instance, an
    /// associated function or constant of the client type, or an item of the
    /// client module.
    fn call(&self, args: &[TokenStream]) -> TokenStream {
        let method = &self.function.ident;
        let path = match self.home {
            Home::Impl(self_ty) => quote!(<#self_ty>::#method),
            Home::Module(module) => quote!(self::#module::#method),
        };
        match self.export.shape {
            Shape::Method => quote!(__client.#method(#(#args),*)),
            Shape::Function => quote!(#path(#(#args),*)),
            Shape::Constant => path,
        }
    }

    /// The messages the export's arguments and its result cross as.
    fn messages(&self) -> Vec<&Wire> {
        let result = match &self.export.ret {
            Return::Unit => None,
            Return::Value(wire) => Some(wire),
            Return::Result { ok, .. } => Some(ok),
            Return::Handle { item, .. } => Some(item),
        };
        std::iter::once(&self.export.args).chain(result).collect()
    }

    /// The statement that reads the arguments message: each parameter is bound
    /// by the name of the field it is, so the compiler checks the message
    /// against the method.
    fn decode(&self) -> syn::Result<TokenStream> {
        let private = private();
        match &self.export.args {
            Wire::Empty => Ok(quote!(#private::no_args(&__args.to_vec())?;)),
            Wire::Message(message) => {
                let path = message_path(message)?;
                let names = self.parameters();
                Ok(quote! {
                    let #path { #(#names,)* .. } =
                        #private::decode_strict::<#path>(&__args.to_vec())?;
                })
            }
            Wire::Wrapper(_) => Err(named(&format!(
                "export `{}`: its arguments arrive as a message of its own",
                self.export.method
            ))),
        }
    }

    /// The names of the method's typed parameters.
    fn parameters(&self) -> Vec<&syn::Ident> {
        self.function.params.iter().collect()
    }

    /// The arguments of the call: `&field` for a reference parameter, the
    /// field otherwise.
    fn call_args(&self) -> Vec<TokenStream> {
        self.parameters()
            .into_iter()
            .zip(&self.export.params)
            .map(|(name, param)| {
                if param.by_ref {
                    quote!(&#name)
                } else {
                    quote!(#name)
                }
            })
            .collect()
    }

    /// The type a method returns, as written.
    fn declared(&self) -> Option<&Type> {
        self.function.output.as_ref()
    }

    /// The shell of a synchronous export: the arguments in, the result out.
    fn sync_shell(&self, parts: &Parts) -> syn::Result<TokenStream> {
        let Parts {
            js,
            shell,
            reads,
            call,
        } = parts;
        let wasm_bindgen = method_attr();
        let private = private();
        let js_value = quote!(#private::wasm_bindgen::JsValue);
        let outcome = self.sync_outcome(call)?;
        let delivered = uint8(&quote!(__bytes));
        let rejected = rejection(&quote!(&__error));
        let (receiver, instance) = match self.export.shape {
            Shape::Method => (quote!(&self,), quote!(let __client = &self.inner;)),
            Shape::Function | Shape::Constant => (TokenStream::new(), TokenStream::new()),
        };
        Ok(quote! {
            #[#wasm_bindgen(js_name = #js)]
            pub fn #shell(
                #receiver
                __args: #private::js_sys::Uint8Array,
            ) -> ::core::result::Result<#private::js_sys::Uint8Array, #js_value> {
                #instance
                (|| -> ::core::result::Result<::std::vec::Vec<u8>, ::api_bones_sdk::Error> {
                    #reads
                    #outcome
                })()
                .map(|__bytes| #delivered)
                .map_err(|__error| #rejected)
            }
        })
    }

    /// The shell of an async export that resolves to one value.
    fn async_shell(&self, parts: &Parts) -> syn::Result<TokenStream> {
        let Parts {
            js,
            shell,
            reads,
            call,
        } = parts;
        let wasm_bindgen = method_attr();
        let private = private();
        let promise = quote!(#private::js_sys::Promise);
        let params = async_params(false);
        let scoped = scoped(false, reads, &self.async_outcome(call)?);
        Ok(quote! {
            #[#wasm_bindgen(js_name = #js)]
            pub fn #shell(#params) -> #promise {
                let __client = self.inner.clone();
                #private::promise(__signal, #scoped)
            }
        })
    }

    /// The shell of an export that returns a handle, and the handle's class.
    fn handle_shell(
        &self,
        parts: &Parts,
        kind: HandleKind,
        wire: &Wire,
    ) -> syn::Result<(TokenStream, TokenStream)> {
        let Parts {
            js,
            shell,
            reads,
            call,
        } = parts;
        let wasm_bindgen = method_attr();
        let private = private();
        let params = async_params(kind == HandleKind::List);
        let item = self.handle_item(wire)?;
        let handle_items = self.handle_class(kind, &item, wire, js);
        let handle_ident = self.handle_ident();
        let body = quote!(::core::result::Result::Ok(#call.await));
        let scoped = scoped(kind == HandleKind::List, reads, &body);
        let shell = quote! {
            #[#wasm_bindgen(js_name = #js)]
            pub fn #shell(#params) -> #handle_ident {
                let __client = self.inner.clone();
                let __lazy = #private::Lazy::new(#scoped);
                let __cancel = __lazy.clone();
                #private::on_abort(&__signal, move || __cancel.cancel());
                #handle_ident { inner: __lazy }
            }
        };
        Ok((shell, handle_items))
    }

    fn sync_outcome(&self, call: &TokenStream) -> syn::Result<TokenStream> {
        match &self.export.ret {
            Return::Unit => Ok(quote! {
                #call;
                ::core::result::Result::Ok(::std::vec::Vec::new())
            }),
            Return::Result { ok, .. } => {
                let encoded = encode(ok, &quote!(&__out));
                Ok(quote! {
                    let __out = #call.map_err(::core::convert::Into::<::api_bones_sdk::Error>::into)?;
                    ::core::result::Result::Ok(#encoded)
                })
            }
            Return::Value(wire) => {
                let encoded = encode(wire, &quote!(&__out));
                Ok(quote! {
                    let __out = #call;
                    ::core::result::Result::Ok(#encoded)
                })
            }
            Return::Handle { .. } => Err(named("a sync export never returns a handle")),
        }
    }

    /// The Rust item type a handle export yields, and what it crosses as.
    fn handle_item(&self, wire: &Wire) -> syn::Result<Type> {
        if matches!(wire, Wire::Empty) {
            return Err(named("a handle yields a message or a scalar"));
        }
        let output = self.declared().and_then(|ty| head(ty).1.into_iter().next());
        output.ok_or_else(|| named("a handle yields one type"))
    }

    /// The body of the async block of a promise export.
    fn async_outcome(&self, call: &TokenStream) -> syn::Result<TokenStream> {
        match &self.export.ret {
            Return::Result { ok, .. } => {
                let encoded = encode(ok, &quote!(&__out));
                let delivered = js_bytes(&quote!(__bytes));
                Ok(quote! {
                    let __out = #call
                        .await
                        .map_err(::core::convert::Into::<::api_bones_sdk::Error>::into)?;
                    let __bytes: ::std::vec::Vec<u8> = #encoded;
                    ::core::result::Result::Ok(#delivered)
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
    fn handle_class(&self, kind: HandleKind, item: &Type, wire: &Wire, js: &str) -> TokenStream {
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
        let each = encode(wire, &quote!(__item));
        let delivered = js_bytes(&quote!(__bytes));
        let next = match kind {
            HandleKind::List => quote! {
                #private::encode_page(&__page, |__item| #each)
            },
            HandleKind::Stream => encode(wire, &quote!(&__page)),
            HandleKind::Batch => quote! {
                #private::encode_batch_chunk(&__page, |__item| #each)
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
                                    let __bytes: ::std::vec::Vec<u8> = { #next };
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
    use syn::ImplItemFn;

    use super::*;
    use crate::common::flat;

    const SOURCE: &str = r#"
        #[client(surface = "connect-pagination", proto = generated::contract::sdk::v1)]
        impl NumbersClient {
            #[export]
            pub async fn list(&self, limit: u32, since: Option<SystemTime>) -> ListCall<generated::contract::sdk::v1::Number> { body() }
            #[export]
            pub async fn watch(&self) -> Stream<generated::contract::sdk::v1::Number> { body() }
            #[export]
            pub async fn get_all(&self, ids: Vec<String>) -> Batch<generated::contract::sdk::v1::Number> { body() }
            #[export(surface = "connect-batch")]
            pub async fn get(&self, id: String) -> Result<generated::contract::sdk::v1::Number, Error> { body() }
            #[export]
            pub async fn clear(&self) -> Result<(), Error> { body() }
            #[export(sync)]
            pub fn label(&self, kind: &str) -> String { body() }
            #[export(sync)]
            pub fn reset(&self) { body() }
            #[export(sync)]
            pub fn check(&self) -> Result<(), Error> { body() }
            #[export(sync, args = generated::contract::v1::ParseRequest)]
            pub fn parse(&self, text: String) -> Result<generated::contract::sdk::v1::Number, Error> { body() }
        }
    "#;

    fn built(service: &str) -> syn::Result<TokenStream> {
        built_from(SOURCE, service)
    }

    fn built_from(source: &str, service: &str) -> syn::Result<TokenStream> {
        expanded(source, service, |_| {})
    }

    /// Expands the first impl of `source` after `change` edits its model's first
    /// client, for shapes the model itself never produces.
    fn expanded(
        source: &str,
        service: &str,
        change: impl FnOnce(&mut Client),
    ) -> syn::Result<TokenStream> {
        let file: syn::File = syn::parse_str(source).expect("a file");
        let mut model = Model::from_files([&file]).expect("a model");
        change(&mut model.clients[0]);
        let item = file
            .items
            .iter()
            .find_map(|item| match item {
                syn::Item::Impl(item) => Some(item),
                _ => None,
            })
            .expect("an impl");
        let functions: Vec<Member> = item
            .items
            .iter()
            .filter_map(|member| match member {
                syn::ImplItem::Fn(function)
                    if api_bones_sdk_model::attrs::find(&function.attrs, "export").is_some() =>
                {
                    Some(Member::of_fn(&function.sig))
                }
                _ => None,
            })
            .collect();
        expand(
            service,
            &model.clients[0],
            &Home::Impl(&item.self_ty),
            &functions,
        )
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
                syn::Item::Const(guard) => assert_eq!(guard.ident, "_"),
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
            text.contains("pubfnnew_js(base_url:::std::string::String,host:::api_bones_sdk::__private::wasm_bindgen::JsValue,options:::api_bones_sdk::__private::js_sys::Uint8Array,wrapper:u32,runtime:u32,)->::core::result::Result<Self,::api_bones_sdk::__private::wasm_bindgen::JsValue>"),
            "{text}"
        );
    }

    #[test]
    fn a_constructor_refuses_a_stale_boundary_then_throws_the_call_error_bytes() {
        let text = text();
        assert!(text.contains("(||->::core::result::Result<NumbersClient,::api_bones_sdk::Error>{::api_bones_sdk::__private::check_boundary(wrapper,runtime)?;let__options=options.to_vec();::api_bones_sdk::__private::no_options(&__options)?;<NumbersClient>::from_host(base_url,host).map_err(::core::convert::Into::<::api_bones_sdk::Error>::into)})().map(|__client|Self{inner:::std::sync::Arc::new(__client)}).map_err(|__error|::api_bones_sdk::__private::wasm_bindgen::JsValue::from(::api_bones_sdk::__private::js_sys::Uint8Array::from(::api_bones_sdk::__private::encode_call_error(&__error).as_slice())))"), "{text}");
        assert!(!text.contains("to_json"), "{text}");
    }

    #[test]
    fn the_options_of_from_host_are_one_message_decoded_from_the_options_bytes() {
        let source = r#"
            #[client(surface = "s")]
            impl NumbersClient {
                pub fn from_host(
                    base_url: String,
                    host: JsValue,
                    options: generated::contract::v1::ClientOptions,
                ) -> Result<Self, Error> { body() }
                #[export]
                pub async fn clear(&self) -> Result<(), Error> { body() }
            }
        "#;
        let text = flat(&built_from(source, "api-bones").expect("expanded"));
        assert!(
            text.contains("<NumbersClient>::from_host(base_url,host,::api_bones_sdk::__private::decode_options(&__options)?)"),
            "{text}"
        );
        assert!(!text.contains("no_options"), "{text}");
        assert!(
            text.contains("const_:()=::core::assert!(::api_bones_sdk::__private::same_name(<generated::contract::v1::ClientOptionsas::api_bones_sdk::__private::MessageName>::FULL_NAME,\"contract.v1.ClientOptions\"),"),
            "{text}"
        );
    }

    #[test]
    fn a_message_path_that_does_not_parse_is_refused_by_the_expansion() {
        let file: syn::File = syn::parse_str("impl C {}").expect("a file");
        let syn::Item::Impl(item) = &file.items[0] else {
            return;
        };
        let client = Client {
            ty: "C".to_owned(),
            module: false,
            surface: "s".to_owned(),
            proto: None,
            exports: Vec::new(),
            options: Some(ProtoRef {
                path: "not a path!".to_owned(),
                full_name: "a.B".to_owned(),
            }),
        };
        expand("svc", &client, &Home::Impl(&item.self_ty), &[]).expect_err("refused");
    }

    #[test]
    fn the_class_reports_its_boundary_version_and_build_info() {
        let text = text();
        assert!(
            text.contains("js_name=\"__api_bones_sdk_boundary\""),
            "{text}"
        );
        assert!(
            text.contains("pubfnboundary_js()->u32{::api_bones_sdk::BOUNDARY}"),
            "{text}"
        );
        assert!(
            text.contains("js_name=\"__api_bones_sdk_build_info\""),
            "{text}"
        );
        assert!(
            text.contains("pubfnbuild_info_js()->::api_bones_sdk::__private::js_sys::Uint8Array{::api_bones_sdk::__private::js_sys::Uint8Array::from(::api_bones_sdk::__private::encode_build_info(::core::env!(\"CARGO_PKG_NAME\"),::core::env!(\"CARGO_PKG_VERSION\"),).as_slice())}"),
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
        assert!(text.contains("pubfnget_js(&self,__args:::api_bones_sdk::__private::js_sys::Uint8Array,__traceparent:::core::option::Option<::std::string::String>,__tracestate:::core::option::Option<::std::string::String>,__signal:::core::option::Option<::api_bones_sdk::__private::web_sys::AbortSignal>,)->::api_bones_sdk::__private::js_sys::Promise"), "{text}");
        assert!(text.contains("::api_bones_sdk::__private::promise(__signal,::api_bones_sdk::CallCtx::scope(::api_bones_sdk::CallCtx{traceparent:__traceparent,tracestate:__tracestate,prefetch:::core::option::Option::None,first_only:false,}"), "{text}");
    }

    #[test]
    fn arguments_are_the_fields_of_one_declared_message_bound_by_name() {
        let text = text();
        assert!(text.contains("letgenerated::contract::sdk::v1::ListArgs{limit,since,..}=::api_bones_sdk::__private::decode_strict::<generated::contract::sdk::v1::ListArgs>(&__args.to_vec())?;"), "{text}");
        assert!(text.contains("__client.list(limit,since)"), "{text}");
        assert!(
            text.contains("letgenerated::contract::sdk::v1::GetAllArgs{ids,..}="),
            "{text}"
        );
        assert!(!text.contains("Args::parse"), "{text}");
        assert!(!text.contains("scalar::<"), "{text}");
    }

    #[test]
    fn a_reference_parameter_is_passed_by_reference() {
        let text = text();
        assert!(
            text.contains("letgenerated::contract::sdk::v1::LabelArgs{kind,..}="),
            "{text}"
        );
        assert!(text.contains("__client.label(&kind)"), "{text}");
    }

    #[test]
    fn an_args_override_names_the_message_of_its_export() {
        let text = text();
        assert!(text.contains("letgenerated::contract::v1::ParseRequest{text,..}=::api_bones_sdk::__private::decode_strict::<generated::contract::v1::ParseRequest>(&__args.to_vec())?;"), "{text}");
        assert!(text.contains("__client.parse(text)"), "{text}");
    }

    #[test]
    fn an_export_without_parameters_takes_the_empty_message() {
        let text = text();
        assert!(
            text.contains("::api_bones_sdk::__private::no_args(&__args.to_vec())?;"),
            "{text}"
        );
        assert!(text.contains("__client.watch()"), "{text}");
    }

    #[test]
    fn every_message_a_shell_names_is_guarded_against_the_recorded_name() {
        let text = text();
        for name in [
            "generated::contract::sdk::v1::ListArgs\",\"contract.sdk.v1.ListArgs",
            "generated::contract::sdk::v1::Number\",\"contract.sdk.v1.Number",
            "generated::contract::v1::ParseRequest\",\"contract.v1.ParseRequest",
        ] {
            let (path, full_name) = name.split_once("\",\"").expect("a pair");
            let guard = format!(
                "const_:()=::core::assert!(::api_bones_sdk::__private::same_name(<{path}as::api_bones_sdk::__private::MessageName>::FULL_NAME,\"{full_name}\"),\"themessageapathnamesisnottheonethemodelrecorded\",);"
            );
            assert!(text.contains(&guard), "{guard} {text}");
        }
        assert!(
            text.contains("#[cfg(all(target_arch=\"wasm32\",any(feature=\"sdk-all\",feature=\"sdk-connect-pagination\",feature=\"sdk-m-list\")))]const_:()=::core::assert!("),
            "{text}"
        );
    }

    /// Rebuilds the sample with `change` applied to the export `method` of its
    /// model, for shapes the model itself never produces.
    fn changed(method: &str, change: impl FnOnce(&mut Export)) -> syn::Result<TokenStream> {
        expanded(SOURCE, "svc", |client| {
            let export = client
                .exports
                .iter_mut()
                .find(|export| export.method == method)
                .expect("the export");
            change(export);
        })
    }

    #[test]
    fn arguments_that_are_not_a_message_are_refused_by_the_expansion() {
        let error = changed("get", |export| {
            export.args = Wire::Wrapper(api_bones_sdk_model::Wrapper::UInt32);
        })
        .expect_err("refused");
        assert!(
            error.to_string().contains("arrive as a message of its own"),
            "{error}"
        );
    }

    #[test]
    fn a_result_is_the_proto_bytes_of_its_message_or_rejects_with_the_error() {
        let text = text();
        assert!(text.contains("let__out=__client.get(id).await.map_err(::core::convert::Into::<::api_bones_sdk::Error>::into)?;let__bytes:::std::vec::Vec<u8>=::api_bones_sdk::__private::encode_message(&__out);::core::result::Result::Ok(::api_bones_sdk::__private::wasm_bindgen::JsValue::from(::api_bones_sdk::__private::js_sys::Uint8Array::from(__bytes.as_slice())))"), "{text}");
    }

    #[test]
    fn a_unit_result_resolves_to_the_empty_message() {
        let text = text();
        assert!(
            text.contains("let__bytes:::std::vec::Vec<u8>={let()=*&__out;::std::vec::Vec::new()};"),
            "{text}"
        );
    }

    #[test]
    fn a_scalar_result_is_its_well_known_wrapper() {
        let source = r#"
            #[client(surface = "s")]
            impl C {
                #[export]
                pub async fn name(&self) -> Result<String, Error> { body() }
            }
        "#;
        let text = flat(&built_from(source, "svc").expect("expanded"));
        assert!(
            text.contains(
                "let__bytes:::std::vec::Vec<u8>=::api_bones_sdk::__private::encode_wrapped(&__out);"
            ),
            "{text}"
        );
        assert!(!text.contains("MessageName"), "{text}");
    }

    #[test]
    fn a_result_that_is_neither_a_message_nor_a_scalar_is_refused() {
        let source = r#"
            #[client(surface = "s")]
            impl C {
                #[export]
                pub async fn many(&self) -> Result<Vec<String>, Error> { body() }
            }
        "#;
        let error = built_from_source_error(source);
        assert!(error.contains("generic type `Vec`"), "{error}");
    }

    fn built_from_source_error(source: &str) -> String {
        let file: syn::File = syn::parse_str(source).expect("a file");
        Model::from_files([&file]).expect_err("refused").to_string()
    }

    #[test]
    fn a_handle_export_returns_its_class_at_once_and_opens_on_the_first_read() {
        let text = text();
        assert!(text.contains("__signal:::core::option::Option<::api_bones_sdk::__private::web_sys::AbortSignal>,)->ApiBonesJsNumbersClientWatchHandle{"), "{text}");
        assert!(text.contains("let__lazy=::api_bones_sdk::__private::Lazy::new(::api_bones_sdk::CallCtx::scope(::api_bones_sdk::CallCtx{traceparent:__traceparent,tracestate:__tracestate,prefetch:::core::option::Option::None,first_only:false,},asyncmove{"), "{text}");
        assert!(
            text.contains("::core::result::Result::Ok(__client.watch().await)"),
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
                    "inner:::api_bones_sdk::__private::Lazy<::api_bones_sdk::{kind}<generated::contract::sdk::v1::Number>>"
                )),
                "{kind} {text}"
            );
        }
    }

    #[test]
    fn a_list_takes_prefetch_and_first_only_as_typed_parameters() {
        let text = text();
        assert!(text.contains("pubfnlist_js(&self,__args:::api_bones_sdk::__private::js_sys::Uint8Array,__traceparent:::core::option::Option<::std::string::String>,__tracestate:::core::option::Option<::std::string::String>,__signal:::core::option::Option<::api_bones_sdk::__private::web_sys::AbortSignal>,__prefetch:::core::option::Option<u8>,__first_only:bool,)->ApiBonesJsNumbersClientListHandle"), "{text}");
        assert!(text.contains("::api_bones_sdk::CallCtx{traceparent:__traceparent,tracestate:__tracestate,prefetch:__prefetch,first_only:__first_only,}"), "{text}");
        let stream = text
            .split("pubfnwatch_js(")
            .nth(1)
            .and_then(|rest| rest.split("->").next())
            .expect("the watch shell");
        assert!(!stream.contains("__prefetch"), "{stream}");
        assert!(!stream.contains("__first_only"), "{stream}");
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
    fn a_list_page_is_the_page_message_of_its_encoded_items() {
        let text = text();
        assert!(
            text.contains("::api_bones_sdk::__private::encode_page(&__page,|__item|::api_bones_sdk::__private::encode_message(__item))"),
            "{text}"
        );
    }

    /// Every handle delivers its bytes alone: a list page keeps no hold on its
    /// items, because the cache owns them.
    #[test]
    fn every_handle_delivers_its_bytes_alone() {
        let text = text();
        assert!(!text.contains("PageHold"), "{text}");
        assert!(!text.contains("Array::of2"), "{text}");
        assert!(!text.contains("from_str"), "{text}");
    }

    #[test]
    fn a_batch_chunk_is_the_chunk_message_in_request_order() {
        let text = text();
        assert!(
            text.contains("::api_bones_sdk::__private::encode_batch_chunk(&__page,|__item|::api_bones_sdk::__private::encode_message(__item))"),
            "{text}"
        );
    }

    #[test]
    fn a_stream_item_is_its_message() {
        let text = text();
        assert!(
            text.contains("let__bytes:::std::vec::Vec<u8>={::api_bones_sdk::__private::encode_message(&__page)};"),
            "{text}"
        );
    }

    #[test]
    fn a_scalar_item_of_a_handle_is_its_well_known_wrapper() {
        let source = r#"
            #[client(surface = "s")]
            impl C {
                #[export]
                pub async fn names(&self) -> Stream<String> { body() }
            }
        "#;
        let text = flat(&built_from(source, "svc").expect("expanded"));
        assert!(
            text.contains("{::api_bones_sdk::__private::encode_wrapped(&__page)};"),
            "{text}"
        );
    }

    #[test]
    fn a_sync_export_returns_the_bytes_or_throws_the_call_error_bytes() {
        let text = text();
        assert!(
            text.contains("js_name=\"api_bones__NumbersClient__label\""),
            "{text}"
        );
        assert!(text.contains("pubfnlabel_js(&self,__args:::api_bones_sdk::__private::js_sys::Uint8Array,)->::core::result::Result<::api_bones_sdk::__private::js_sys::Uint8Array,::api_bones_sdk::__private::wasm_bindgen::JsValue>"), "{text}");
        assert!(text.contains(".map(|__bytes|::api_bones_sdk::__private::js_sys::Uint8Array::from(__bytes.as_slice())).map_err(|__error|::api_bones_sdk::__private::wasm_bindgen::JsValue::from(::api_bones_sdk::__private::js_sys::Uint8Array::from(::api_bones_sdk::__private::encode_call_error(&__error).as_slice())))"), "{text}");
        assert!(text.contains("let__out=__client.label(&kind);::core::result::Result::Ok(::api_bones_sdk::__private::encode_wrapped(&__out))"), "{text}");
        assert!(
            text.contains("__client.reset();::core::result::Result::Ok(::std::vec::Vec::new())"),
            "{text}"
        );
        assert!(text.contains("let__out=__client.check().map_err(::core::convert::Into::<::api_bones_sdk::Error>::into)?;::core::result::Result::Ok({let()=*&__out;"), "{text}");
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
        let functions: Vec<Member> = item
            .items
            .iter()
            .filter_map(|member| match member {
                syn::ImplItem::Fn(function) => Some(Member::of_fn(&function.sig)),
                _ => None,
            })
            .collect();
        let error = expand(
            "svc",
            &model.clients[0],
            &Home::Impl(&item.self_ty),
            &functions,
        )
        .expect_err("refused");
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
        let text = flat(
            &expand("svc", &model.clients[0], &Home::Impl(&item.self_ty), &[]).expect("expanded"),
        );
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
            shape: Shape::Method,
            sync: false,
            surface: "s".to_owned(),
            params: Vec::new(),
            args: Wire::Empty,
            ret: Return::Value(Wire::Wrapper(api_bones_sdk_model::Wrapper::UInt32)),
        };
        let client = Client {
            ty: "C".to_owned(),
            module: false,
            surface: "s".to_owned(),
            proto: None,
            exports: vec![export],
            options: None,
        };
        assert_eq!(model.clients.len(), 0);
        let refused = |client: &Client| {
            expand(
                "svc",
                client,
                &Home::Impl(&item.self_ty),
                &[Member::of_fn(&function.sig)],
            )
            .expect_err("refused")
            .to_string()
        };
        let error = refused(&client);
        assert!(error.contains("async export returns"), "{error}");
        let with = |sync: bool, ret: Return| Client {
            exports: vec![Export {
                sync,
                ret,
                ..client.exports[0].clone()
            }],
            ..client.clone()
        };
        let handle = |item: Wire| Return::Handle {
            kind: HandleKind::List,
            item,
        };
        let error = refused(&with(
            false,
            handle(Wire::Wrapper(api_bones_sdk_model::Wrapper::UInt32)),
        ));
        assert!(error.contains("yields one type"), "{error}");
        let error = refused(&with(false, handle(Wire::Empty)));
        assert!(error.contains("a message or a scalar"), "{error}");
        let error = refused(&with(true, handle(Wire::Empty)));
        assert!(error.contains("never returns a handle"), "{error}");
    }
}
