// SPDX-License-Identifier: MIT
//! The exports of one Layer 2 crate, read from its source.
//!
//! [`Model::from_files`] is the one walk of the annotated items: the macros'
//! expansion tests and the generator both read a crate through it, so a
//! refusal is worded and located once.
//!
//! # The contract the generator reads
//!
//! Every value that crosses the wasm boundary is a message declared in proto:
//!
//! - [`Export::args`] is the message the export's parameters arrive as:
//!   [`Wire::Empty`] when the export has no parameter and no `args = ...`;
//!   otherwise [`Wire::Message`] of the `args = <message path>` override of
//!   `#[export]`, else of `<proto>::<PascalMethod>Args`, where `proto` is the
//!   module path of `#[client(proto = ...)]`. The message's fields are the
//!   parameters, by name.
//! - [`Export::ret`] is a [`Return`] whose success value or handle item is a
//!   [`Wire`].
//! - [`Client::options`] is the options message of the impl's `from_host`, the
//!   one parameter after `base_url` and `host`.
//! - A [`Param`] is the Rust name and the type as written (`ty`), and whether
//!   the type is a reference (`by_ref`). The type is not classified: the
//!   expansion names it, so the compiler checks it, and the generator takes
//!   the field types from the proto message.

use std::collections::BTreeSet;

use heck::ToUpperCamelCase;
use quote::ToTokens;
use syn::{
    Attribute, Fields, FnArg, Ident, ImplItem, Item, ItemEnum, ItemImpl, ItemMod, Pat, Signature,
    Type,
};

use crate::attrs::{ClientArgs, ErrorArgs, ErrorVariantArgs, ExportArgs, find, tokens};
use crate::names::json_name;
use crate::ty::{
    Env, HandleKind, ProtoRef, Return, Wire, classify_return, classify_wire, handle_kind,
    last_ident, proto_ref,
};

/// A named parameter of an export and its type as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    /// The Rust name.
    pub name: String,
    /// The type, as the tokens print it (`Option < SystemTime >`).
    pub ty: String,
    /// The type is a reference, so the expansion passes `&field`.
    pub by_ref: bool,
}

impl Param {
    /// The key this parameter has in the JSON.
    #[must_use]
    pub fn json_name(&self) -> String {
        json_name(&self.name)
    }
}

/// The kind of an argument of the error JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgKind {
    /// `String`.
    String,
    /// `i64`, which crosses as a decimal string.
    Int64,
    /// `f64`.
    Double,
    /// `SystemTime`.
    Timestamp,
}

impl ArgKind {
    /// The `kind` the error JSON states.
    #[must_use]
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Int64 => "int64",
            Self::Double => "double",
            Self::Timestamp => "timestamp",
        }
    }

    /// The kind a field type states, `None` for a type an error argument
    /// cannot be.
    #[must_use]
    pub fn of(ty: &Type) -> Option<Self> {
        match last_ident(ty)?.as_str() {
            "String" => Some(Self::String),
            "i64" => Some(Self::Int64),
            "f64" => Some(Self::Double),
            "SystemTime" => Some(Self::Timestamp),
            _ => None,
        }
    }
}

/// A named field of an error variant, which becomes one entry of `args`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorField {
    /// The field name, which is the key in `args`.
    pub name: String,
    /// The argument kind.
    pub kind: ArgKind,
}

/// A variant of an `#[error]` enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorVariant {
    /// The variant name.
    pub name: String,
    /// The `code` of the error JSON.
    pub code: String,
    /// The `platform` of the error JSON, an `ErrorCode` name.
    pub platform: String,
    /// The fields, in declaration order.
    pub fields: Vec<ErrorField>,
}

/// An `#[error]` enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorType {
    /// The type name.
    pub name: String,
    /// The `emitter` of the error JSON.
    pub emitter: String,
    /// The variants, in declaration order.
    pub variants: Vec<ErrorVariant>,
}

/// What an export is in the Rust source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// A method taking `&self`.
    Method,
    /// A function with no receiver: an associated function of a client impl or
    /// a free function of a client module. Always `#[export(sync)]`.
    Function,
    /// An associated or free `const`, exported as a function that takes
    /// nothing. Always `#[export(sync)]`.
    Constant,
}

/// One `#[export]` method, function or constant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Export {
    /// The Rust function name.
    pub method: String,
    /// Whether the export is a method, a receiver-less function or a constant.
    pub shape: Shape,
    /// `#[export(sync)]`.
    pub sync: bool,
    /// The `sdk_surfaces` id: the method's own, else its client's.
    pub surface: String,
    /// The parameters after `&self`; every parameter of a function.
    pub params: Vec<Param>,
    /// The message the parameters arrive as; see the module docs.
    pub args: Wire,
    /// The classified return type.
    pub ret: Return,
}

/// An `impl` or an inline `mod` that carries `#[client]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Client {
    /// The type the impl is for, or the name of the module.
    pub ty: String,
    /// The client is a `mod`: it holds only receiver-less exports, has no
    /// instance, no constructor and no options.
    pub module: bool,
    /// The default surface of its exports.
    pub surface: String,
    /// The Rust module path of the impl's `<Method>Args` messages, from
    /// `#[client(proto = ...)]`.
    pub proto: Option<String>,
    /// The exports, in declaration order.
    pub exports: Vec<Export>,
    /// The options message of the impl's own `from_host`, the one parameter
    /// after `base_url` and `host`; `None` when the impl has no `from_host` or
    /// it takes no options.
    pub options: Option<ProtoRef>,
}

/// Everything a Layer 2 crate annotates.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Model {
    /// The `#[error]` enums.
    pub errors: Vec<ErrorType>,
    /// The `#[client]` impls and modules.
    pub clients: Vec<Client>,
}

impl Model {
    /// Reads one source text.
    ///
    /// # Errors
    ///
    /// A [`syn::Error`] for a parse failure or any refusal of
    /// [`Model::from_files`].
    pub fn from_source(text: &str) -> syn::Result<Self> {
        let file: syn::File = syn::parse_str(text)?;
        Self::from_files([&file])
    }

    /// Reads the annotated items of every file, descending into inline
    /// modules.
    ///
    /// # Errors
    ///
    /// A [`syn::Error`] for the first item that breaks a rule: a type outside
    /// the allowed set, a missing or misplaced argument, parameters with no
    /// message to arrive as, an export outside a `#[client]` impl, an async
    /// mismatch, or a receiver other than `&self`.
    pub fn from_files<'a>(files: impl IntoIterator<Item = &'a syn::File>) -> syn::Result<Self> {
        let mut items = Vec::new();
        for file in files {
            flatten(&file.items, &mut items);
        }
        let env = Env::strict(declared(&items));
        let mut model = Self::default();
        for item in items {
            match item {
                Item::Enum(item) => {
                    if let Some(attr) = find(&item.attrs, "error") {
                        model.errors.push(error_type(item, attr)?);
                    }
                }
                Item::Impl(item) => {
                    if let Some(client) = client(item, &env)? {
                        model.clients.push(client);
                    }
                }
                Item::Mod(item) if find(&item.attrs, "client").is_some() => {
                    model.clients.push(module_client(item, &env)?);
                }
                Item::Fn(item) if find(&item.attrs, "export").is_some() => {
                    return Err(outside(item.sig.ident.span()));
                }
                _ => {}
            }
        }
        Ok(model)
    }
}

/// The items of a file and of its inline modules, in source order. A module
/// that carries `#[client]` is one item: its functions are its exports.
fn flatten<'a>(items: &'a [Item], out: &mut Vec<&'a Item>) {
    for item in items {
        out.push(item);
        if let Item::Mod(ItemMod {
            attrs,
            content: Some((_, inner)),
            ..
        }) = item
            && find(attrs, "client").is_none()
        {
            flatten(inner, out);
        }
    }
}

/// The names of the error types.
fn declared(items: &[&Item]) -> BTreeSet<String> {
    items
        .iter()
        .filter_map(|item| match item {
            Item::Enum(item) if find(&item.attrs, "error").is_some() => {
                Some(item.ident.to_string())
            }
            _ => None,
        })
        .collect()
}

fn outside(span: proc_macro2::Span) -> syn::Error {
    syn::Error::new(
        span,
        "`#[export]` belongs on a method of an impl that carries `#[client]`",
    )
}

fn error_type(item: &ItemEnum, attr: &Attribute) -> syn::Result<ErrorType> {
    let args = ErrorArgs::parse(tokens(attr)?)?;
    let mut variants = Vec::new();
    for variant in &item.variants {
        let attr = find(&variant.attrs, "error").ok_or_else(|| {
            syn::Error::new_spanned(
                variant,
                "an `#[error]` variant needs `#[error(code = \"...\", platform = \"...\")]`",
            )
        })?;
        let args = ErrorVariantArgs::parse(tokens(attr)?)?;
        let fields = match &variant.fields {
            Fields::Unit => Vec::new(),
            Fields::Named(named) => named
                .named
                .iter()
                .map(error_field)
                .collect::<syn::Result<_>>()?,
            Fields::Unnamed(_) => {
                return Err(syn::Error::new_spanned(
                    variant,
                    "an `#[error]` variant names its fields: they become `args` by name",
                ));
            }
        };
        variants.push(ErrorVariant {
            name: variant.ident.to_string(),
            code: args.code,
            platform: args.platform,
            fields,
        });
    }
    Ok(ErrorType {
        name: item.ident.to_string(),
        emitter: args.emitter,
        variants,
    })
}

fn error_field(field: &syn::Field) -> syn::Result<ErrorField> {
    let name = field
        .ident
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_default();
    let kind = ArgKind::of(&field.ty).ok_or_else(|| {
        syn::Error::new_spanned(
            &field.ty,
            format!("field `{name}`: an error argument is a String, i64, f64 or SystemTime"),
        )
    })?;
    Ok(ErrorField { name, kind })
}

/// What `#[export]` sits on, in an impl or a module: the signature, the
/// attribute, and for a constant its type.
struct Member<'a> {
    sig: Signature,
    attr: &'a Attribute,
    constant: Option<&'a Type>,
}

impl<'a> Member<'a> {
    fn function(sig: &Signature, attr: &'a Attribute) -> Self {
        Self {
            sig: sig.clone(),
            attr,
            constant: None,
        }
    }

    /// A constant has no parameter, so its signature is `fn NAME() -> T`.
    fn constant(ident: &Ident, ty: &'a Type, attr: &'a Attribute) -> Self {
        let function: syn::ItemFn = syn::parse_quote!(
            fn f() {}
        );
        let mut sig = function.sig;
        sig.ident = ident.clone();
        sig.output = syn::ReturnType::Type(syn::token::RArrow::default(), Box::new(ty.clone()));
        Self {
            sig,
            attr,
            constant: Some(ty),
        }
    }
}

fn exported(item: &ImplItem) -> Option<Member<'_>> {
    match item {
        ImplItem::Fn(function) => {
            find(&function.attrs, "export").map(|attr| Member::function(&function.sig, attr))
        }
        ImplItem::Const(constant) => find(&constant.attrs, "export")
            .map(|attr| Member::constant(&constant.ident, &constant.ty, attr)),
        _ => None,
    }
}

fn module_exported(item: &Item) -> Option<Member<'_>> {
    match item {
        Item::Fn(function) => {
            find(&function.attrs, "export").map(|attr| Member::function(&function.sig, attr))
        }
        Item::Const(constant) => find(&constant.attrs, "export")
            .map(|attr| Member::constant(&constant.ident, &constant.ty, attr)),
        _ => None,
    }
}

/// A `#[client]` module: free functions and constants, no instance.
fn module_client(item: &ItemMod, env: &Env) -> syn::Result<Client> {
    let attr = find(&item.attrs, "client").expect("the caller found it");
    let args = ClientArgs::parse(tokens(attr)?)?;
    let Some((_, inner)) = &item.content else {
        return Err(syn::Error::new_spanned(
            &item.ident,
            "`#[client]` goes on a module with its items inline",
        ));
    };
    let mut exports = Vec::new();
    for member in inner.iter().filter_map(module_exported) {
        exports.push(export(&member, &args, env, true)?);
    }
    Ok(Client {
        ty: item.ident.to_string(),
        module: true,
        surface: args.surface,
        proto: args.proto,
        exports,
        options: None,
    })
}

fn client(item: &ItemImpl, env: &Env) -> syn::Result<Option<Client>> {
    let Some(attr) = find(&item.attrs, "client") else {
        return match item.items.iter().find_map(exported) {
            Some(member) => Err(outside(member.sig.ident.span())),
            None => Ok(None),
        };
    };
    let args = ClientArgs::parse(tokens(attr)?)?;
    if item.trait_.is_some() || !item.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &item.self_ty,
            "`#[client]` goes on an inherent, non-generic impl",
        ));
    }
    let ty = last_ident(&item.self_ty).ok_or_else(|| {
        syn::Error::new_spanned(
            &item.self_ty,
            "`#[client]` goes on an impl of a plain type name",
        )
    })?;
    let mut exports = Vec::new();
    for member in item.items.iter().filter_map(exported) {
        exports.push(export(&member, &args, env, false)?);
    }
    Ok(Some(Client {
        ty,
        module: false,
        surface: args.surface,
        proto: args.proto,
        exports,
        options: options(item)?,
    }))
}

/// The options message of the impl's `from_host`.
///
/// The constructor is detected, not required: an impl without one has no
/// options. The options are at most one parameter after `base_url` and `host`,
/// the options message of the client, which crosses as the protobuf bytes of
/// that message; a host that sets none sends the empty message.
fn options(item: &ItemImpl) -> syn::Result<Option<ProtoRef>> {
    let Some(function) = item.items.iter().find_map(|member| match member {
        ImplItem::Fn(function) if function.sig.ident == "from_host" => Some(function),
        _ => None,
    }) else {
        return Ok(None);
    };
    let inputs: Vec<&syn::PatType> = function
        .sig
        .inputs
        .iter()
        .filter_map(|input| match input {
            FnArg::Typed(typed) => Some(typed),
            FnArg::Receiver(_) => None,
        })
        .collect();
    if inputs.len() < 2 {
        return Err(syn::Error::new_spanned(
            &function.sig,
            "`from_host` takes `base_url` and `host`, then its options",
        ));
    }
    let [typed] = &inputs[2..] else {
        return if inputs.len() == 2 {
            Ok(None)
        } else {
            Err(syn::Error::new_spanned(
                &function.sig,
                "`from_host` takes one options message after `base_url` and `host`",
            ))
        };
    };
    let name = plain_name(&typed.pat)?;
    match classify_wire(&typed.ty, &format!("option `{name}`"))? {
        Wire::Message(message) => Ok(Some(message)),
        Wire::Empty | Wire::Wrapper(_) => Err(syn::Error::new_spanned(
            &typed.ty,
            format!("option `{name}` of `from_host` is the options message of the client"),
        )),
    }
}

fn plain_name(pat: &Pat) -> syn::Result<String> {
    match pat {
        Pat::Ident(ident) => Ok(ident.ident.to_string()),
        _ => Err(syn::Error::new_spanned(pat, "a parameter is a plain name")),
    }
}

/// The message an export's parameters arrive as.
///
/// An `args = ...` override names it; else, with parameters, it is
/// `<proto>::<PascalMethod>Args`; an export with neither takes nothing.
fn args_message(
    ident: &Ident,
    method: &str,
    args: &ExportArgs,
    client: &ClientArgs,
    has_params: bool,
) -> syn::Result<Wire> {
    let message = |path: &str| {
        proto_ref(path).map(Wire::Message).map_err(|reason| {
            syn::Error::new_spanned(ident, format!("export `{method}`: {reason}"))
        })
    };
    match (&args.args, &client.proto, has_params) {
        (Some(path), _, _) => message(path),
        (None, Some(proto), true) => {
            message(&format!("{proto}::{}Args", method.to_upper_camel_case()))
        }
        (None, None, true) => Err(syn::Error::new_spanned(
            ident,
            format!(
                "export `{method}` has parameters but no message to arrive as: give \
                 `#[client(proto = <module path>)]` or `#[export(args = <message path>)]`"
            ),
        )),
        (None, _, false) => Ok(Wire::Empty),
    }
}

fn export(
    member: &Member<'_>,
    client: &ClientArgs,
    env: &Env,
    in_module: bool,
) -> syn::Result<Export> {
    let args = ExportArgs::parse(tokens(member.attr)?)?;
    let sig = &member.sig;
    if member.constant.is_some() && !args.sync {
        return Err(syn::Error::new_spanned(
            &sig.ident,
            "a constant export is `#[export(sync)]`: it has no endpoint to call",
        ));
    }
    let is_async = sig.asyncness.is_some();
    if args.sync && is_async {
        return Err(syn::Error::new_spanned(sig, "a sync export is not `async`"));
    }
    // A `ListCall` or `Batch` export only builds its handle, which opens its
    // call on first use, so it is a plain `fn`; every other export that is not
    // sync awaits its call.
    let handle = handle_kind(&sig.output);
    if !args.sync && is_async != handle.is_none_or(HandleKind::is_async) {
        return Err(syn::Error::new_spanned(
            sig,
            if is_async {
                "a ListCall or Batch export is a plain `fn` returning the handle, not `async`: \
                 the handle opens its call on first use"
            } else {
                "an async export is `async`; mark a plain method `#[export(sync)]`"
            },
        ));
    }
    if !sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &sig.generics,
            "an export has no generics",
        ));
    }
    let shape = match sig.receiver() {
        _ if member.constant.is_some() => Shape::Constant,
        Some(receiver) if receiver.reference.is_some() && receiver.mutability.is_none() => {
            Shape::Method
        }
        Some(receiver) => {
            return Err(syn::Error::new_spanned(
                receiver,
                "an export takes `&self`, not `self` or `&mut self`",
            ));
        }
        None if args.sync => Shape::Function,
        None => {
            return Err(syn::Error::new_spanned(
                &sig.ident,
                "an async export takes `&self`: a function with no receiver is `#[export(sync)]`, \
                 and an async one, or one that returns a handle, belongs on a method of a client",
            ));
        }
    };
    if in_module && shape == Shape::Method {
        return Err(syn::Error::new_spanned(
            sig,
            "a function of a `#[client]` module has no receiver",
        ));
    }
    let mut params = Vec::new();
    for input in &sig.inputs {
        let FnArg::Typed(typed) = input else {
            continue;
        };
        params.push(Param {
            name: plain_name(&typed.pat)?,
            ty: typed.ty.to_token_stream().to_string(),
            by_ref: matches!(*typed.ty, Type::Reference(_)),
        });
    }
    let method = sig.ident.to_string();
    let ret = classify_return(&sig.output, args.sync, env)?;
    if shape == Shape::Constant && !matches!(ret, Return::Value(_)) {
        return Err(syn::Error::new_spanned(
            &sig.ident,
            "a constant export is a message or a scalar",
        ));
    }
    Ok(Export {
        args: args_message(&sig.ident, &method, &args, client, !params.is_empty())?,
        shape,
        sync: args.sync,
        surface: args.surface.unwrap_or_else(|| client.surface.clone()),
        params,
        ret,
        method,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ty::{ErrorRef, HandleKind, Wrapper};

    const SAMPLE: &str = r#"
        #[error(emitter = "widgets")]
        pub enum WidgetError {
            #[error(code = "WIDGET_GONE", platform = "NOT_FOUND")]
            Gone { id: String, at: SystemTime, size: i64, ratio: f64 },
            #[error(code = "WIDGET_BUSY", platform = "CONFLICT")]
            Busy,
        }

        mod inner {
            #[error(emitter = "inner")]
            pub enum InnerError {
                #[error(code = "INNER", platform = "INTERNAL")]
                Broken,
            }
        }

        #[client(surface = "connect-pagination", proto = generated::contract::sdk::v1)]
        impl NumbersClient {
            #[export]
            pub fn list_numbers(
                &self, limit: u32, since: Option<SystemTime>,
            ) -> ListCall<generated::contract::sdk::v1::Number> { body() }
            #[export(sync)]
            pub fn label(&self, kind: &str) -> String { body() }
            #[export(surface = "other")]
            pub async fn get_number(
                &self, id: String,
            ) -> Result<generated::contract::v1::Widget, WidgetError> { body() }
            #[export(args = generated::contract::v1::WatchRequest)]
            pub async fn watch(&self, from: String) -> Stream<generated::contract::v1::Tick> { body() }
            #[export]
            pub async fn ping(&self) -> Result<(), Error> { body() }
            pub fn not_exported(&self) {}
            const ID: u32 = 1;
        }

        impl Plain { fn helper(&self) {} }
        fn free() {}
        struct Untagged;
        enum Plainer { A }
    "#;

    fn refused(text: &str) -> String {
        Model::from_source(text).expect_err("refused").to_string()
    }

    fn message(path: &str, full_name: &str) -> Wire {
        Wire::Message(ProtoRef {
            path: path.to_owned(),
            full_name: full_name.to_owned(),
        })
    }

    #[test]
    fn an_error_type_carries_its_emitter_variants_and_argument_kinds() {
        let model = Model::from_source(SAMPLE).expect("ok");
        let names: Vec<_> = model.errors.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["WidgetError", "InnerError"]);
        let error = &model.errors[0];
        assert_eq!(error.emitter, "widgets");
        assert_eq!(error.variants[0].code, "WIDGET_GONE");
        assert_eq!(error.variants[0].platform, "NOT_FOUND");
        let kinds: Vec<_> = error.variants[0]
            .fields
            .iter()
            .map(|f| (f.name.as_str(), f.kind.wire_name()))
            .collect();
        assert_eq!(
            kinds,
            [
                ("id", "string"),
                ("at", "timestamp"),
                ("size", "int64"),
                ("ratio", "double")
            ]
        );
        assert_eq!(error.variants[1].fields, Vec::<ErrorField>::new());
    }

    #[test]
    fn a_client_carries_its_proto_module_surface_and_exports_in_order() {
        let model = Model::from_source(SAMPLE).expect("ok");
        assert_eq!(model.clients.len(), 1);
        let client = &model.clients[0];
        assert_eq!(client.ty, "NumbersClient");
        assert_eq!(client.surface, "connect-pagination");
        assert_eq!(
            client.proto.as_deref(),
            Some("generated::contract::sdk::v1")
        );
        assert_eq!(client.options, None);
        let methods: Vec<_> = client.exports.iter().map(|e| e.method.as_str()).collect();
        assert_eq!(
            methods,
            ["list_numbers", "label", "get_number", "watch", "ping"]
        );
    }

    #[test]
    fn parameters_arrive_as_the_args_message_of_the_client_module() {
        let model = Model::from_source(SAMPLE).expect("ok");
        let list = &model.clients[0].exports[0];
        assert!(!list.sync);
        assert_eq!(list.surface, "connect-pagination");
        assert_eq!(
            list.args,
            message(
                "generated::contract::sdk::v1::ListNumbersArgs",
                "contract.sdk.v1.ListNumbersArgs"
            )
        );
        let names: Vec<_> = list.params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["limit", "since"]);
        assert_eq!(list.params[0].ty, "u32");
        assert!(!list.params[0].by_ref);
        assert_eq!(list.params[1].ty.replace(' ', ""), "Option<SystemTime>");
        assert_eq!(list.params[1].json_name(), "since");
        assert_eq!(
            list.ret,
            Return::Handle {
                kind: HandleKind::List,
                item: message(
                    "generated::contract::sdk::v1::Number",
                    "contract.sdk.v1.Number"
                )
            }
        );
    }

    #[test]
    fn a_reference_parameter_is_passed_by_reference_and_a_sync_export_returns_a_value() {
        let model = Model::from_source(SAMPLE).expect("ok");
        let label = &model.clients[0].exports[1];
        assert!(label.sync);
        assert!(label.params[0].by_ref);
        assert_eq!(label.params[0].ty.replace(' ', ""), "&str");
        assert_eq!(label.ret, Return::Value(Wire::Wrapper(Wrapper::String)));
        assert_eq!(
            label.args,
            message(
                "generated::contract::sdk::v1::LabelArgs",
                "contract.sdk.v1.LabelArgs"
            )
        );
    }

    #[test]
    fn an_export_carries_its_own_surface_and_its_result_and_error() {
        let model = Model::from_source(SAMPLE).expect("ok");
        let get = &model.clients[0].exports[2];
        assert_eq!(get.surface, "other");
        assert_eq!(
            get.ret,
            Return::Result {
                ok: message("generated::contract::v1::Widget", "contract.v1.Widget"),
                error: ErrorRef::Named("WidgetError".to_owned())
            }
        );
        let ping = &model.clients[0].exports[4];
        assert_eq!(
            ping.ret,
            Return::Result {
                ok: Wire::Empty,
                error: ErrorRef::Sdk
            }
        );
    }

    #[test]
    fn an_args_override_names_the_request_message_and_no_params_take_nothing() {
        let model = Model::from_source(SAMPLE).expect("ok");
        let watch = &model.clients[0].exports[3];
        assert_eq!(
            watch.args,
            message(
                "generated::contract::v1::WatchRequest",
                "contract.v1.WatchRequest"
            )
        );
        assert_eq!(model.clients[0].exports[4].args, Wire::Empty);
    }

    #[test]
    fn an_export_without_parameters_needs_no_proto_module() {
        let text = "#[client(surface = \"s\")] impl C { \
            #[export] async fn go(&self) -> Result<(), Error> { body() } }";
        let model = Model::from_source(text).expect("ok");
        assert_eq!(model.clients[0].proto, None);
        assert_eq!(model.clients[0].exports[0].args, Wire::Empty);
    }

    #[test]
    fn several_files_form_one_model() {
        let one: syn::File = syn::parse_str(
            "#[error(emitter = \"e\")] enum E { #[error(code = \"A\", platform = \"P\")] A }",
        )
        .expect("a");
        let two: syn::File = syn::parse_str(
            "#[client(surface = \"s\", proto = generated::p::v1)] impl C { \
             #[export] async fn go(&self, a: String) -> Result<generated::p::v1::R, E> { body() } }",
        )
        .expect("b");
        let model = Model::from_files([&one, &two]).expect("ok");
        assert_eq!(model.errors.len(), 1);
        assert_eq!(model.clients[0].exports[0].params[0].ty, "String");
        assert_eq!(
            model.clients[0].exports[0].ret,
            Return::Result {
                ok: message("generated::p::v1::R", "p.v1.R"),
                error: ErrorRef::Named("E".to_owned())
            }
        );
    }

    #[test]
    fn a_source_that_does_not_parse_is_refused() {
        assert!(Model::from_source("fn (").is_err());
    }

    #[test]
    fn parameters_need_a_message_to_arrive_as() {
        let message = refused(
            "#[client(surface = \"s\")] impl C { \
             #[export] async fn go(&self, a: String) -> Result<(), Error> { body() } }",
        );
        assert!(
            message.contains("`go`") && message.contains("`#[client(proto = <module path>)]`"),
            "{message}"
        );
    }

    #[test]
    fn a_proto_module_or_args_override_that_names_no_package_is_refused() {
        let message = refused(
            "#[client(surface = \"s\", proto = generated)] impl C { \
             #[export] async fn go(&self, a: String) -> Result<(), Error> { body() } }",
        );
        assert!(
            message.contains("export `go`") && message.contains("package and a message"),
            "{message}"
        );
        let message = refused(
            "#[client(surface = \"s\")] impl C { \
             #[export(args = Request)] async fn go(&self) -> Result<(), Error> { body() } }",
        );
        assert!(message.contains("package and a message"), "{message}");
    }

    #[test]
    fn an_error_type_is_complete_and_its_fields_are_error_arguments() {
        assert!(refused("#[error] enum E { A }").contains("needs `emitter"));
        assert!(refused("#[error(emitter = \"e\")] enum E { A }").contains("needs `#[error(code"));
        assert!(
            refused("#[error(emitter = \"e\")] enum E { #[error(code = \"A\")] A }")
                .contains("needs `platform")
        );
        let tuple = "#[error(emitter = \"e\")] enum E { #[error(code = \"A\", platform = \"P\")] A(String) }";
        assert!(refused(tuple).contains("names its fields"));
        let kind = "#[error(emitter = \"e\")] enum E { #[error(code = \"A\", platform = \"P\")] A { n: u32 } }";
        let message = refused(kind);
        assert!(
            message.contains("field `n`") && message.contains("String, i64, f64"),
            "{message}"
        );
    }

    #[test]
    fn a_client_goes_on_an_inherent_non_generic_impl_of_a_named_type() {
        assert!(refused("#[client] impl C {}").contains("needs `surface"));
        assert!(refused("#[client(surface = \"s\")] impl Tr for C {}").contains("inherent"));
        assert!(refused("#[client(surface = \"s\")] impl<T> C<T> {}").contains("inherent"));
        assert!(
            refused("#[client(surface = \"s\")] impl &'static str {}").contains("plain type name")
        );
    }

    #[test]
    fn an_export_outside_a_client_impl_is_refused() {
        assert!(refused("#[export] fn free() {}").contains("carries `#[client]`"));
        assert!(
            refused("impl C { #[export] async fn go(&self) {} }").contains("carries `#[client]`")
        );
    }

    fn export_refused(method: &str) -> String {
        refused(&format!(
            "#[client(surface = \"s\", proto = generated::p::v1)] impl C {{ {method} }}"
        ))
    }

    #[test]
    fn an_export_is_async_unless_sync_and_takes_ref_self_and_plain_names() {
        let message = export_refused("#[export] fn go(&self) -> String { body() }");
        assert!(
            message.contains("mark a plain method `#[export(sync)]`"),
            "{message}"
        );
        let message = export_refused("#[export(sync)] async fn go(&self) -> String { body() }");
        assert!(
            message.contains("a sync export is not `async`"),
            "{message}"
        );
        let message = export_refused("#[export(sync)] fn go<T>(&self) {}");
        assert!(message.contains("no generics"), "{message}");
        for receiver in ["self", "&mut self", "mut self"] {
            let message = export_refused(&format!("#[export(sync)] fn go({receiver}) {{}}"));
            assert!(
                message.contains("not `self` or `&mut self`"),
                "{receiver}: {message}"
            );
        }
        let message = export_refused("#[export] async fn go() -> Result<(), Error> { body() }");
        assert!(
            message.contains("an async export takes `&self`"),
            "{message}"
        );
        let message = export_refused("#[export(sync)] fn go(&self, (a, b): (i32, i32)) {}");
        assert!(message.contains("a parameter is a plain name"), "{message}");
        let message = export_refused("#[export] async fn go(&self) -> Plain { body() }");
        assert!(message.contains("the return type"), "{message}");
        let message =
            export_refused("#[export] fn go() -> ListCall<generated::p::v1::R> { body() }");
        assert!(
            message.contains("or one that returns a handle, belongs on a method"),
            "{message}"
        );
        let message = export_refused("#[export(bad)] fn go(&self) {}");
        assert!(message.contains("unknown argument"), "{message}");
    }

    #[test]
    fn a_list_or_batch_export_is_a_plain_fn_and_a_stream_export_is_async() {
        for handle in ["ListCall", "Batch"] {
            let ret = format!("-> {handle}<generated::p::v1::R>");
            let message =
                export_refused(&format!("#[export] async fn go(&self) {ret} {{ body() }}"));
            assert!(
                message.contains("a ListCall or Batch export is a plain `fn`"),
                "{handle}: {message}"
            );
            let model = Model::from_source(&format!(
                "#[client(surface = \"s\")] impl C {{ #[export] fn go(&self) {ret} {{ body() }} }}"
            ))
            .expect("a plain fn export of a list or batch handle");
            let export = &model.clients[0].exports[0];
            assert!(!export.sync);
            assert!(matches!(export.ret, Return::Handle { .. }));
            let message =
                export_refused(&format!("#[export(sync)] fn go(&self) {ret} {{ body() }}"));
            assert!(
                message.contains("a sync export never returns a handle"),
                "{message}"
            );
        }
        let message =
            export_refused("#[export] fn go(&self) -> Stream<generated::p::v1::R> { body() }");
        assert!(message.contains("an async export is `async`"), "{message}");
        Model::from_source(
            "#[client(surface = \"s\")] impl C { #[export] async fn go(&self) -> Stream<generated::p::v1::R> { body() } }",
        )
        .expect("an async stream export");
    }

    const STATICS: &str = r#"
        #[client(surface = "tools", proto = generated::p::v1)]
        impl Tools {
            #[export(sync)]
            pub fn version() -> String { body() }
            #[export(sync)]
            pub fn clamp(low: u32, high: u32) -> u32 { body() }
            #[export(sync)]
            pub const LIMIT: u32 = 5;
            #[export(sync)]
            pub fn label(&self) -> String { body() }
        }

        #[client(surface = "tools", proto = generated::p::v1)]
        mod text {
            #[export(sync)]
            pub fn upper(value: String) -> String { body() }
            #[export(sync)]
            pub const NAME: String = String::new();
            #[export(sync)]
            pub fn none() -> Result<(), Error> { body() }
        }
    "#;

    #[test]
    fn a_function_a_constant_and_a_method_are_told_apart() {
        let model = Model::from_source(STATICS).expect("ok");
        let tools = &model.clients[0];
        assert!(!tools.module);
        let shapes: Vec<_> = tools
            .exports
            .iter()
            .map(|e| (e.method.as_str(), e.shape))
            .collect();
        assert_eq!(
            shapes,
            [
                ("version", Shape::Function),
                ("clamp", Shape::Function),
                ("LIMIT", Shape::Constant),
                ("label", Shape::Method),
            ]
        );
        assert_eq!(tools.exports[0].args, Wire::Empty);
        assert_eq!(
            tools.exports[1].args,
            message("generated::p::v1::ClampArgs", "p.v1.ClampArgs")
        );
        assert_eq!(tools.exports[1].params.len(), 2);
        assert_eq!(tools.exports[2].params, []);
        assert_eq!(tools.exports[2].args, Wire::Empty);
        assert!(matches!(
            tools.exports[2].ret,
            Return::Value(Wire::Wrapper(_))
        ));
    }

    #[test]
    fn a_client_module_holds_its_functions_and_is_not_walked_as_a_plain_module() {
        let model = Model::from_source(STATICS).expect("ok");
        let text = &model.clients[1];
        assert!(text.module);
        assert_eq!(text.ty, "text");
        assert_eq!(text.options, None);
        let names: Vec<_> = text.exports.iter().map(|e| e.method.as_str()).collect();
        assert_eq!(names, ["upper", "NAME", "none"]);
        assert_eq!(text.exports[0].surface, "tools");
    }

    #[test]
    fn an_export_inside_a_module_without_client_is_still_outside() {
        assert!(refused("mod m { #[export(sync)] fn f() {} }").contains("carries `#[client]`"));
    }

    #[test]
    fn a_client_module_refuses_what_a_module_cannot_hold() {
        let module = |body: &str| {
            refused(&format!(
                "#[client(surface = \"s\", proto = generated::p::v1)] mod m {{ {body} }}"
            ))
        };
        assert!(
            module("#[export] async fn f() -> Result<(), Error> { body() }")
                .contains("belongs on a method")
        );
        assert!(module("#[export(sync)] fn f(&self) {}").contains("no receiver"));
        assert!(
            module("#[export] const C: u32 = 1;")
                .contains("a constant export is `#[export(sync)]`")
        );
        assert!(
            module("#[export(sync)] const C: Result<(), Error> = Ok(());")
                .contains("a message or a scalar")
        );
        assert!(module("#[export(sync)] const C: () = ();").contains("a message or a scalar"));
        assert!(refused("#[client(surface = \"s\")] mod m;").contains("items inline"));
        assert!(refused("#[client] mod m {}").contains("needs `surface"));
    }

    #[test]
    fn a_constant_outside_a_client_is_refused() {
        assert!(
            refused("impl C { #[export(sync)] const A: u32 = 1; }").contains("carries `#[client]`")
        );
    }

    fn options_of(constructor: &str) -> syn::Result<Option<ProtoRef>> {
        let text = format!("#[client(surface = \"s\")] impl C {{ {constructor} }}");
        Model::from_source(&text).map(|model| model.clients[0].options.clone())
    }

    #[test]
    fn a_client_without_from_host_has_no_options() {
        let model = Model::from_source(SAMPLE).expect("ok");
        assert_eq!(model.clients[0].options, None);
    }

    #[test]
    fn the_options_of_from_host_are_one_message_after_base_url_and_host() {
        let options = options_of(
            "pub fn from_host(base_url: String, host: JsValue, \
             options: generated::contract::v1::ClientOptions) -> Result<Self, Error> { body() }",
        )
        .expect("ok");
        assert_eq!(
            options,
            Some(ProtoRef {
                path: "generated::contract::v1::ClientOptions".to_owned(),
                full_name: "contract.v1.ClientOptions".to_owned()
            })
        );
        let none = options_of(
            "pub fn from_host(base_url: String, host: JsValue) -> Result<Self, Error> { body() }",
        )
        .expect("ok");
        assert_eq!(none, None);
    }

    #[test]
    fn the_options_of_from_host_are_not_more_than_one_message() {
        let message = options_of(
            "pub fn from_host(base_url: String, host: JsValue, \
             a: generated::p::v1::A, b: generated::p::v1::B) -> Result<Self, Error> { body() }",
        )
        .expect_err("refused")
        .to_string();
        assert!(message.contains("one options message"), "{message}");
    }

    #[test]
    fn an_option_of_from_host_is_a_message_not_a_scalar() {
        for (ty, what) in [("u32", "options message"), ("()", "options message")] {
            let message = options_of(&format!(
                "pub fn from_host(base_url: String, host: JsValue, size: {ty}) -> Result<Self, Error> {{ body() }}"
            ))
            .expect_err("refused")
            .to_string();
            assert!(
                message.contains("option `size`") && message.contains(what),
                "{message}"
            );
        }
        let message = options_of(
            "pub fn from_host(base_url: String, host: JsValue, size: Option<usize>) -> Result<Self, Error> { body() }",
        )
        .expect_err("refused")
        .to_string();
        assert!(message.contains("option `size`"), "{message}");
        let message = options_of(
            "pub fn from_host(base_url: String, host: JsValue, size: Kind) -> Result<Self, Error> { body() }",
        )
        .expect_err("refused")
        .to_string();
        assert!(message.contains("bare name"), "{message}");
        let message = options_of(
            "pub fn from_host(base_url: String, host: JsValue, (a, b): (i32, i32)) -> Result<Self, Error> { body() }",
        )
        .expect_err("refused")
        .to_string();
        assert!(message.contains("a parameter is a plain name"), "{message}");
        let message =
            options_of("pub fn from_host(base_url: String) -> Result<Self, Error> { body() }")
                .expect_err("refused")
                .to_string();
        assert!(message.contains("`base_url` and `host`"), "{message}");
    }
}
