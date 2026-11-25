// SPDX-License-Identifier: MIT
//! The exports of one Layer 2 crate, read from its source.
//!
//! [`Model::from_files`] is the one walk of the annotated items: the macros'
//! expansion tests and the generator both read a crate through it, so a
//! refusal is worded and located once.

use std::collections::BTreeSet;

use syn::{
    Attribute, Fields, FnArg, ImplItem, ImplItemFn, Item, ItemEnum, ItemImpl, ItemStruct, Pat, Type,
};

use crate::attrs::{self, ClientArgs, ErrorArgs, ErrorVariantArgs, ExportArgs, find, tokens};
use crate::names::{enum_value, json_name};
use crate::ty::{Env, Return, WireType, classify, classify_return, last_ident};

/// A named parameter or record field and its type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    /// The Rust name.
    pub name: String,
    /// The classified type.
    pub ty: WireType,
}

impl Param {
    /// The key this parameter or field has in the JSON.
    #[must_use]
    pub fn json_name(&self) -> String {
        json_name(&self.name)
    }
}

/// A `#[record]` struct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// The type name.
    pub name: String,
    /// The fields, in declaration order.
    pub fields: Vec<Param>,
}

/// An `#[enumeration]` enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enumeration {
    /// The type name.
    pub name: String,
    /// The snake case strings the variants cross as, in declaration order.
    pub values: Vec<String>,
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

    fn of(ty: &Type) -> Option<Self> {
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

/// One `#[export]` method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Export {
    /// The Rust function name.
    pub method: String,
    /// `#[export(sync)]`.
    pub sync: bool,
    /// The `sdk_surfaces` id: the method's own, else its client's.
    pub surface: String,
    /// The parameters after `&self`.
    pub params: Vec<Param>,
    /// The classified return type.
    pub ret: Return,
}

/// An `impl` that carries `#[client]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Client {
    /// The type the impl is for.
    pub ty: String,
    /// The default surface of its exports.
    pub surface: String,
    /// The exports, in declaration order.
    pub exports: Vec<Export>,
}

/// Everything a Layer 2 crate annotates.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Model {
    /// The `#[record]` structs.
    pub records: Vec<Record>,
    /// The `#[enumeration]` enums.
    pub enumerations: Vec<Enumeration>,
    /// The `#[error]` enums.
    pub errors: Vec<ErrorType>,
    /// The `#[client]` impls.
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
    /// the allowed set, a missing or misplaced argument, an export outside a
    /// `#[client]` impl, an async mismatch, or a receiver other than `&self`.
    pub fn from_files<'a>(files: impl IntoIterator<Item = &'a syn::File>) -> syn::Result<Self> {
        let mut items = Vec::new();
        for file in files {
            flatten(&file.items, &mut items);
        }
        let (named, errors) = declared(&items);
        let env = Env::strict(named, errors);
        let mut model = Self::default();
        for item in items {
            match item {
                Item::Struct(item) => {
                    if let Some(attr) = find(&item.attrs, "record") {
                        model.records.push(record(item, attr, &env)?);
                    }
                }
                Item::Enum(item) => model.enum_item(item)?,
                Item::Impl(item) => {
                    if let Some(client) = client(item, &env)? {
                        model.clients.push(client);
                    }
                }
                Item::Fn(item) if find(&item.attrs, "export").is_some() => {
                    return Err(outside(item.sig.ident.span()));
                }
                _ => {}
            }
        }
        Ok(model)
    }

    fn enum_item(&mut self, item: &ItemEnum) -> syn::Result<()> {
        match (find(&item.attrs, "enumeration"), find(&item.attrs, "error")) {
            (Some(_), Some(_)) => Err(syn::Error::new_spanned(
                &item.ident,
                "an enum is either `#[enumeration]` or `#[error]`, not both",
            )),
            (Some(attr), None) => {
                self.enumerations.push(enumeration(item, attr)?);
                Ok(())
            }
            (None, Some(attr)) => {
                self.errors.push(error_type(item, attr)?);
                Ok(())
            }
            (None, None) => Ok(()),
        }
    }
}

/// The items of a file and of its inline modules, in source order.
fn flatten<'a>(items: &'a [Item], out: &mut Vec<&'a Item>) {
    for item in items {
        out.push(item);
        if let Item::Mod(syn::ItemMod {
            content: Some((_, inner)),
            ..
        }) = item
        {
            flatten(inner, out);
        }
    }
}

/// The names of the records and enumerations, and of the error types.
fn declared(items: &[&Item]) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut named = BTreeSet::new();
    let mut errors = BTreeSet::new();
    for item in items {
        match item {
            Item::Struct(item) if find(&item.attrs, "record").is_some() => {
                named.insert(item.ident.to_string());
            }
            Item::Enum(item) => {
                if find(&item.attrs, "enumeration").is_some() {
                    named.insert(item.ident.to_string());
                }
                if find(&item.attrs, "error").is_some() {
                    errors.insert(item.ident.to_string());
                }
            }
            _ => {}
        }
    }
    (named, errors)
}

fn outside(span: proc_macro2::Span) -> syn::Error {
    syn::Error::new(
        span,
        "`#[export]` belongs on a method of an impl that carries `#[client]`",
    )
}

fn record(item: &ItemStruct, attr: &Attribute, env: &Env) -> syn::Result<Record> {
    attrs::no_args("record", tokens(attr)?)?;
    let Fields::Named(fields) = &item.fields else {
        return Err(syn::Error::new_spanned(
            &item.ident,
            "a `#[record]` has named fields",
        ));
    };
    let mut parsed = Vec::new();
    for field in &fields.named {
        let name = field
            .ident
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default();
        let ty = classify(&field.ty, env, &format!("field `{name}`"))?;
        parsed.push(Param { name, ty });
    }
    Ok(Record {
        name: item.ident.to_string(),
        fields: parsed,
    })
}

fn enumeration(item: &ItemEnum, attr: &Attribute) -> syn::Result<Enumeration> {
    attrs::no_args("enumeration", tokens(attr)?)?;
    Ok(Enumeration {
        name: item.ident.to_string(),
        values: enumeration_values(item)?,
    })
}

/// The wire string of each variant of an `#[enumeration]` enum, in order.
///
/// # Errors
/// A variant carries fields.
pub fn enumeration_values(item: &ItemEnum) -> syn::Result<Vec<String>> {
    item.variants
        .iter()
        .map(|variant| {
            if matches!(variant.fields, Fields::Unit) {
                Ok(enum_value(&variant.ident.to_string()))
            } else {
                Err(syn::Error::new_spanned(
                    variant,
                    "an `#[enumeration]` has only unit variants",
                ))
            }
        })
        .collect()
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

fn exported(item: &ImplItem) -> Option<(&ImplItemFn, &Attribute)> {
    match item {
        ImplItem::Fn(function) => find(&function.attrs, "export").map(|attr| (function, attr)),
        _ => None,
    }
}

fn client(item: &ItemImpl, env: &Env) -> syn::Result<Option<Client>> {
    let Some(attr) = find(&item.attrs, "client") else {
        return match item.items.iter().find_map(exported) {
            Some((function, _)) => Err(outside(function.sig.ident.span())),
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
    for (function, attr) in item.items.iter().filter_map(exported) {
        exports.push(export(function, attr, &args.surface, env)?);
    }
    Ok(Some(Client {
        ty,
        surface: args.surface,
        exports,
    }))
}

fn export(
    function: &ImplItemFn,
    attr: &Attribute,
    default_surface: &str,
    env: &Env,
) -> syn::Result<Export> {
    let args = ExportArgs::parse(tokens(attr)?)?;
    let sig = &function.sig;
    if args.sync == sig.asyncness.is_some() {
        return Err(syn::Error::new_spanned(
            sig,
            if args.sync {
                "a sync export is not `async`"
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
    match sig.receiver() {
        Some(receiver) if receiver.reference.is_some() && receiver.mutability.is_none() => {}
        Some(receiver) => {
            return Err(syn::Error::new_spanned(
                receiver,
                "an export takes `&self`, not `self` or `&mut self`",
            ));
        }
        None => {
            return Err(syn::Error::new_spanned(
                &sig.ident,
                "an export takes `&self`",
            ));
        }
    }
    let mut params = Vec::new();
    for input in &sig.inputs {
        let FnArg::Typed(typed) = input else {
            continue;
        };
        let Pat::Ident(ident) = &*typed.pat else {
            return Err(syn::Error::new_spanned(
                &typed.pat,
                "a parameter is a plain name",
            ));
        };
        let name = ident.ident.to_string();
        let ty = classify(&typed.ty, env, &format!("parameter `{name}`"))?;
        params.push(Param { name, ty });
    }
    Ok(Export {
        method: sig.ident.to_string(),
        sync: args.sync,
        surface: args.surface.unwrap_or_else(|| default_surface.to_owned()),
        params,
        ret: classify_return(&sig.output, args.sync, env)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ty::{ErrorRef, HandleKind};

    const SAMPLE: &str = r#"
        #[record]
        pub struct Number { pub value: i64, pub label: Option<String>, pub page_size: u32 }

        #[enumeration]
        pub enum Kind { Exact, LowerBound }

        #[error(emitter = "widgets")]
        pub enum WidgetError {
            #[error(code = "WIDGET_GONE", platform = "NOT_FOUND")]
            Gone { id: String, at: SystemTime, size: i64, ratio: f64 },
            #[error(code = "WIDGET_BUSY", platform = "CONFLICT")]
            Busy,
        }

        mod inner {
            #[record]
            pub struct Nested { pub n: Vec<u8> }
        }

        #[client(surface = "connect-pagination")]
        impl NumbersClient {
            #[export]
            pub async fn list(&self, limit: u32, since: Option<SystemTime>) -> ListCall<Number> {
                body()
            }
            #[export(sync)]
            pub fn label(&self, kind: Kind) -> String { body() }
            #[export(surface = "other")]
            pub async fn get_number(&self, id: String) -> Result<Number, WidgetError> { body() }
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

    #[test]
    fn a_crate_is_read_into_its_records_enumerations_errors_and_clients() {
        let model = Model::from_source(SAMPLE).expect("ok");
        let names: Vec<_> = model.records.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["Number", "Nested"]);
        assert_eq!(
            model.records[0].fields,
            [
                Param {
                    name: "value".to_owned(),
                    ty: WireType::I64
                },
                Param {
                    name: "label".to_owned(),
                    ty: WireType::Option(Box::new(WireType::String))
                },
                Param {
                    name: "page_size".to_owned(),
                    ty: WireType::U32
                },
            ]
        );
        assert_eq!(model.records[0].fields[2].json_name(), "pageSize");
        assert_eq!(model.records[1].fields[0].ty, WireType::Bytes);
        assert_eq!(model.enumerations.len(), 1);
        assert_eq!(model.enumerations[0].values, ["exact", "lower_bound"]);
    }

    #[test]
    fn an_error_type_carries_its_emitter_variants_and_argument_kinds() {
        let model = Model::from_source(SAMPLE).expect("ok");
        let error = &model.errors[0];
        assert_eq!(error.name, "WidgetError");
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
    fn a_client_carries_its_exports_with_their_surface_params_and_return() {
        let model = Model::from_source(SAMPLE).expect("ok");
        assert_eq!(model.clients.len(), 1);
        let client = &model.clients[0];
        assert_eq!(client.ty, "NumbersClient");
        assert_eq!(client.surface, "connect-pagination");
        let methods: Vec<_> = client.exports.iter().map(|e| e.method.as_str()).collect();
        assert_eq!(methods, ["list", "label", "get_number"]);
        let list = &client.exports[0];
        assert!(!list.sync);
        assert_eq!(list.surface, "connect-pagination");
        assert_eq!(list.params.len(), 2);
        assert_eq!(list.params[0].name, "limit");
        assert_eq!(
            list.ret,
            Return::Handle {
                kind: HandleKind::List,
                item: "Number".to_owned()
            }
        );
        let label = &client.exports[1];
        assert!(label.sync);
        assert_eq!(label.params[0].ty, WireType::Named("Kind".to_owned()));
        assert_eq!(label.ret, Return::Value(WireType::String));
        let get = &client.exports[2];
        assert_eq!(get.surface, "other");
        assert_eq!(
            get.ret,
            Return::Result {
                ok: Some(WireType::Named("Number".to_owned())),
                error: ErrorRef::Named("WidgetError".to_owned())
            }
        );
    }

    #[test]
    fn several_files_form_one_model() {
        let one: syn::File = syn::parse_str("#[record] pub struct A { pub x: i32 }").expect("a");
        let two: syn::File = syn::parse_str(
            "#[client(surface = \"s\")] impl C { #[export] async fn go(&self, a: A) -> Result<A, Error> { body() } }",
        )
        .expect("b");
        let model = Model::from_files([&one, &two]).expect("ok");
        assert_eq!(model.records.len(), 1);
        assert_eq!(
            model.clients[0].exports[0].params[0].ty,
            WireType::Named("A".to_owned())
        );
    }

    #[test]
    fn a_source_that_does_not_parse_is_refused() {
        assert!(Model::from_source("fn (").is_err());
    }

    #[test]
    fn a_record_needs_named_fields_from_the_allowed_set_and_no_arguments() {
        assert!(refused("#[record] struct A(i32);").contains("named fields"));
        assert!(refused("#[record] struct A;").contains("named fields"));
        assert!(refused("#[record] struct A { x: usize }").contains("field `x`"));
        assert!(refused("#[record(camel)] struct A { x: i32 }").contains("takes no arguments"));
        assert!(refused("#[record = 1] struct A { x: i32 }").contains("in parentheses"));
    }

    #[test]
    fn an_enumeration_has_only_unit_variants_and_no_arguments() {
        assert!(refused("#[enumeration] enum E { A(i32) }").contains("only unit variants"));
        assert!(refused("#[enumeration] enum E { A { x: i32 } }").contains("only unit variants"));
        assert!(refused("#[enumeration(x)] enum E { A }").contains("takes no arguments"));
    }

    #[test]
    fn an_enum_is_not_both_an_enumeration_and_an_error() {
        let text = "#[enumeration] #[error(emitter = \"e\")] enum E { A }";
        assert!(refused(text).contains("not both"));
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
        refused(&format!("#[client(surface = \"s\")] impl C {{ {method} }}"))
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
        let message = export_refused("#[export(sync)] fn go() {}");
        assert!(message.contains("takes `&self`"), "{message}");
        let message = export_refused("#[export(sync)] fn go(&self, (a, b): (i32, i32)) {}");
        assert!(message.contains("a parameter is a plain name"), "{message}");
        let message = export_refused("#[export(sync)] fn go(&self, a: &str) {}");
        assert!(message.contains("parameter `a`"), "{message}");
        let message = export_refused("#[export] async fn go(&self) -> Plain { body() }");
        assert!(message.contains("the return type"), "{message}");
        let message = export_refused("#[export(bad)] fn go(&self) {}");
        assert!(message.contains("unknown argument"), "{message}");
    }
}
