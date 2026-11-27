// SPDX-License-Identifier: MIT
//! What crosses the wasm boundary, and the one classifier that decides it.
//!
//! Every value that crosses is a message declared in proto, as bytes. A type an
//! export names is therefore one of:
//!
//! - a message, written as the path of its generated Rust type with the
//!   module the generator put it in (`generated::contract::v1::Widget`), so the
//!   proto name is read from the path alone;
//! - a scalar, which crosses as its `google.protobuf` wrapper message
//!   (`UInt32Value`, `StringValue`, `BoolValue`, `BytesValue`, `Timestamp`,
//!   `Duration`, ...), never as an implicit field of a message of the crate's
//!   own;
//! - `()`, which crosses as `google.protobuf.Empty`.
//!
//! A return is `Result<T, E>` or a handle (`ListCall<T>`, `Batch<T>`,
//! `Stream<T>`); a sync export returns a plain value instead and never a
//! handle. Rust types are never checked here beyond their shape: the compiler
//! checks them, because the expansion names them.

use std::collections::BTreeSet;

use proc_macro2::Span;
use syn::{GenericArgument, PathArguments, ReturnType, Type, TypePath};

/// The allowed set, as every refusal names it.
pub const ALLOWED: &str = "a generated message written with its module path \
(`generated::pkg::v1::Message`), bool, i32, u32, i64, u64, f64, String, Vec<u8>, SystemTime or \
Duration (each crosses as its google.protobuf wrapper), or () (google.protobuf.Empty)";

/// A scalar and the `google.protobuf` wrapper message it crosses as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrapper {
    /// `bool`, as `BoolValue`.
    Bool,
    /// `i32`, as `Int32Value`.
    Int32,
    /// `u32`, as `UInt32Value`.
    UInt32,
    /// `i64`, as `Int64Value`.
    Int64,
    /// `u64`, as `UInt64Value`.
    UInt64,
    /// `f64`, as `DoubleValue`.
    Double,
    /// `String`, as `StringValue`.
    String,
    /// `Vec<u8>`, as `BytesValue`.
    Bytes,
    /// `std::time::SystemTime`, as `Timestamp`.
    Timestamp,
    /// `std::time::Duration`, as `Duration`.
    Duration,
}

impl Wrapper {
    /// The full proto name of the wrapper message.
    #[must_use]
    pub fn full_name(self) -> &'static str {
        match self {
            Self::Bool => "google.protobuf.BoolValue",
            Self::Int32 => "google.protobuf.Int32Value",
            Self::UInt32 => "google.protobuf.UInt32Value",
            Self::Int64 => "google.protobuf.Int64Value",
            Self::UInt64 => "google.protobuf.UInt64Value",
            Self::Double => "google.protobuf.DoubleValue",
            Self::String => "google.protobuf.StringValue",
            Self::Bytes => "google.protobuf.BytesValue",
            Self::Timestamp => "google.protobuf.Timestamp",
            Self::Duration => "google.protobuf.Duration",
        }
    }

    /// The scalar's Rust type, as written.
    #[must_use]
    pub fn rust_type(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Int32 => "i32",
            Self::UInt32 => "u32",
            Self::Int64 => "i64",
            Self::UInt64 => "u64",
            Self::Double => "f64",
            Self::String => "String",
            Self::Bytes => "Vec<u8>",
            Self::Timestamp => "SystemTime",
            Self::Duration => "Duration",
        }
    }

    fn of_name(name: &str) -> Option<Self> {
        [
            Self::Bool,
            Self::Int32,
            Self::UInt32,
            Self::Int64,
            Self::UInt64,
            Self::Double,
            Self::String,
            Self::Timestamp,
            Self::Duration,
        ]
        .into_iter()
        .find(|wrapper| wrapper.rust_type() == name)
    }
}

/// The full name of the empty message that stands for no value.
pub const EMPTY: &str = "google.protobuf.Empty";

/// A generated message: the Rust path an export names it by, and the proto
/// full name the path implies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtoRef {
    /// The Rust path, segments joined by `::`, as written (or built from a
    /// client's `proto` module).
    pub path: String,
    /// The full proto name, `package.Message`, read from the path by
    /// [`proto_ref`].
    pub full_name: String,
}

/// What one crossing value is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wire {
    /// No value: `google.protobuf.Empty`.
    Empty,
    /// A scalar in its `google.protobuf` wrapper.
    Wrapper(Wrapper),
    /// A generated message.
    Message(ProtoRef),
}

impl Wire {
    /// The full proto name of the message the value crosses as.
    #[must_use]
    pub fn full_name(&self) -> &str {
        match self {
            Self::Empty => EMPTY,
            Self::Wrapper(wrapper) => wrapper.full_name(),
            Self::Message(message) => &message.full_name,
        }
    }
}

/// The proto name a Rust path implies.
///
/// The segments after the last `generated` (the module the generator's output
/// is included in) are the package and the message; without one, the leading
/// `crate`, `self` and `super` are dropped. At least a package segment and the
/// message are needed. A path that implies a wrong name is no silent error:
/// the expansion asserts the type's own `FULL_NAME` equals it.
///
/// # Errors
///
/// A message naming what the path lacks.
pub fn proto_ref(path: &str) -> Result<ProtoRef, String> {
    let segments: Vec<&str> = path.split("::").collect();
    let rest = if let Some(at) = segments.iter().rposition(|segment| *segment == "generated") {
        &segments[at + 1..]
    } else {
        let skipped = segments
            .iter()
            .take_while(|segment| matches!(**segment, "crate" | "self" | "super"))
            .count();
        &segments[skipped..]
    };
    if rest.len() < 2 {
        return Err(format!(
            "`{path}` does not name a package and a message: write the generated type with its \
module path, such as `generated::contract::v1::Widget`"
        ));
    }
    Ok(ProtoRef {
        path: path.to_owned(),
        full_name: rest.join("."),
    })
}

/// The text of a plain path, `a::b::C`.
///
/// # Errors
///
/// A [`syn::Error`] for a path with type arguments, a `Self` or a qualified
/// self.
pub fn path_text(path: &syn::Path) -> syn::Result<String> {
    let mut parts = Vec::new();
    for segment in &path.segments {
        if !segment.arguments.is_none() {
            return Err(syn::Error::new_spanned(
                segment,
                "a message path has no type arguments",
            ));
        }
        parts.push(segment.ident.to_string());
    }
    Ok(parts.join("::"))
}

/// What the classifier knows about the crate's own error types.
///
/// A macro sees one item at a time, so it cannot tell whether a named type
/// carries `#[error]`; it uses [`Env::deferred`] and leaves that to the
/// `Into<api_bones_sdk::Error>` bound its expansion asserts. The generator
/// has parsed the whole crate and uses [`Env::strict`].
#[derive(Debug, Clone, Default)]
pub struct Env {
    errors: Option<BTreeSet<String>>,
}

impl Env {
    /// Every error type is accepted here and checked later.
    #[must_use]
    pub fn deferred() -> Self {
        Self::default()
    }

    /// Only the given error type names are accepted.
    #[must_use]
    pub fn strict(errors: BTreeSet<String>) -> Self {
        Self {
            errors: Some(errors),
        }
    }

    fn knows_error(&self, name: &str) -> bool {
        self.errors
            .as_ref()
            .is_none_or(|errors| errors.contains(name))
    }
}

/// Classifies what one value crosses as.
///
/// `what` names the position in a refusal, such as ``the handle item``.
///
/// # Errors
///
/// A [`syn::Error`] spanning the type, whose message names `what`, the reason
/// and the allowed set.
pub fn classify_wire(ty: &Type, what: &str) -> syn::Result<Wire> {
    let refuse =
        |reason: &str| syn::Error::new_spanned(ty, format!("{what}: {reason}; allowed: {ALLOWED}"));
    match ty {
        Type::Tuple(tuple) if tuple.elems.is_empty() => Ok(Wire::Empty),
        Type::Paren(inner) => classify_wire(&inner.elem, what),
        Type::Path(TypePath { qself: None, path }) => {
            let last = path
                .segments
                .last()
                .ok_or_else(|| refuse("an empty path is not a type"))?;
            if let PathArguments::AngleBracketed(args) = &last.arguments {
                return match (
                    last.ident.to_string().as_str(),
                    type_args(args.args.iter()).as_deref(),
                ) {
                    ("Vec", Some(&[item])) if last_ident(item).as_deref() == Some("u8") => {
                        Ok(Wire::Wrapper(Wrapper::Bytes))
                    }
                    (name, _) => Err(refuse(&format!("generic type `{name}` is not allowed"))),
                };
            }
            if !last.arguments.is_none() {
                return Err(refuse("closures and function types cannot cross"));
            }
            let name = last.ident.to_string();
            let std_scalar = path.segments.len() == 1
                || matches!(
                    path.segments
                        .first()
                        .map(|first| first.ident.to_string())
                        .as_deref(),
                    Some("std" | "core" | "alloc")
                );
            if let Some(wrapper) = Wrapper::of_name(&name).filter(|_| std_scalar) {
                return Ok(Wire::Wrapper(wrapper));
            }
            if path.segments.len() < 2 {
                return Err(refuse(&format!(
                    "`{name}` is a bare name: write the message with its module path"
                )));
            }
            let text = path_text(path)?;
            proto_ref(&text)
                .map(Wire::Message)
                .map_err(|reason| refuse(&reason))
        }
        Type::Reference(_) => Err(refuse(
            "references and lifetimes are not allowed, take the value",
        )),
        Type::Tuple(_) | Type::Array(_) | Type::Slice(_) => {
            Err(refuse("tuples, arrays and slices are not allowed"))
        }
        Type::BareFn(_) => Err(refuse("function pointers cannot cross")),
        Type::ImplTrait(_) | Type::TraitObject(_) | Type::Infer(_) => {
            Err(refuse("generics and trait types are not allowed"))
        }
        _ => Err(refuse("this form of type is not allowed")),
    }
}

/// The type arguments, or `None` when one is a lifetime, a constant or a
/// binding.
fn type_args<'a>(args: impl Iterator<Item = &'a GenericArgument>) -> Option<Vec<&'a Type>> {
    args.map(|arg| match arg {
        GenericArgument::Type(ty) => Some(ty),
        _ => None,
    })
    .collect()
}

/// The last path segment of a type written as a plain path.
pub(crate) fn last_ident(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(TypePath { qself: None, path }) => path
            .segments
            .last()
            .filter(|segment| segment.arguments.is_none())
            .map(|segment| segment.ident.to_string()),
        _ => None,
    }
}

/// The kind of a handle an async export returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleKind {
    /// `ListCall<T>`, which yields pages.
    List,
    /// `Batch<T>`, which yields chunks of results.
    Batch,
    /// `Stream<T>`, which yields items.
    Stream,
}

impl HandleKind {
    /// The Rust type name.
    #[must_use]
    pub fn type_name(self) -> &'static str {
        match self {
            Self::List => "ListCall",
            Self::Batch => "Batch",
            Self::Stream => "Stream",
        }
    }

    fn of(name: &str) -> Option<Self> {
        [Self::List, Self::Batch, Self::Stream]
            .into_iter()
            .find(|kind| kind.type_name() == name)
    }
}

/// The error type of a `Result` return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorRef {
    /// `api_bones_sdk::Error`.
    Sdk,
    /// A type that carries `#[error]`, by name.
    Named(String),
}

/// A classified return type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Return {
    /// `()` or no return type: a sync export only.
    Unit,
    /// A plain value: a sync export only.
    Value(Wire),
    /// `Result<T, E>`; `ok` is [`Wire::Empty`] for `()`.
    Result {
        /// What the success value crosses as.
        ok: Wire,
        /// The error type.
        error: ErrorRef,
    },
    /// A handle over a message or a scalar: an async export only.
    Handle {
        /// Which handle.
        kind: HandleKind,
        /// What one item the handle yields crosses as.
        item: Wire,
    },
}

/// Classifies the return type of an export.
///
/// What a value crosses as is [`classify_wire`]: a message, a scalar in its
/// `google.protobuf` wrapper, or nothing.
///
/// # Errors
///
/// A [`syn::Error`] naming the return type, the reason and the allowed set.
pub fn classify_return(output: &ReturnType, sync: bool, env: &Env) -> syn::Result<Return> {
    const WHAT: &str = "the return type";
    let async_shape = "an async export returns Result<T, E> or a handle (ListCall<T>, Batch<T> \
or Stream<T>)";
    let refuse = |span: Option<&Type>, reason: &str| match span {
        Some(ty) => syn::Error::new_spanned(ty, format!("{WHAT}: {reason}; allowed: {ALLOWED}")),
        None => syn::Error::new(
            Span::call_site(),
            format!("{WHAT}: {reason}; allowed: {ALLOWED}"),
        ),
    };
    let ReturnType::Type(_, boxed) = output else {
        return if sync {
            Ok(Return::Unit)
        } else {
            Err(refuse(None, async_shape))
        };
    };
    let ty: &Type = boxed;
    if matches!(ty, Type::Tuple(tuple) if tuple.elems.is_empty()) {
        return if sync {
            Ok(Return::Unit)
        } else {
            Err(refuse(Some(ty), async_shape))
        };
    }
    let (name, args) = generic_head(ty);
    if let Some(kind) = HandleKind::of(&name) {
        let &[item] = args.as_slice() else {
            return Err(refuse(Some(ty), "a handle takes exactly one type"));
        };
        if sync {
            return Err(refuse(Some(ty), "a sync export never returns a handle"));
        }
        return match classify_wire(item, "the handle item")? {
            Wire::Empty => Err(refuse(Some(item), "a handle yields a message or a scalar")),
            item => Ok(Return::Handle { kind, item }),
        };
    }
    if name == "Result" {
        let &[ok, error] = args.as_slice() else {
            return Err(refuse(Some(ty), "Result takes both T and E"));
        };
        let ok = classify_wire(ok, WHAT)?;
        let error = classify_error(error, env).map_err(|reason| refuse(Some(error), &reason))?;
        return Ok(Return::Result { ok, error });
    }
    if sync {
        classify_wire(ty, WHAT).map(Return::Value)
    } else {
        Err(refuse(Some(ty), async_shape))
    }
}

/// The head name of a path type and its type arguments; an empty name and no
/// arguments for any other form.
fn generic_head(ty: &Type) -> (String, Vec<&Type>) {
    let Type::Path(TypePath { qself: None, path }) = ty else {
        return (String::new(), Vec::new());
    };
    let Some(segment) = path.segments.last() else {
        return (String::new(), Vec::new());
    };
    let args = match &segment.arguments {
        PathArguments::AngleBracketed(args) => type_args(args.args.iter()).unwrap_or_default(),
        _ => Vec::new(),
    };
    (segment.ident.to_string(), args)
}

fn classify_error(ty: &Type, env: &Env) -> Result<ErrorRef, String> {
    match last_ident(ty) {
        Some(name) if name == "Error" => Ok(ErrorRef::Sdk),
        Some(name) if env.knows_error(&name) => Ok(ErrorRef::Named(name)),
        _ => {
            Err("the error type is api_bones_sdk::Error or a type that carries #[error]".to_owned())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ty(text: &str) -> Type {
        syn::parse_str(text).unwrap_or_else(|_| {
            let bound: syn::TraitBound = syn::parse_str(text).expect("a type");
            Type::Path(TypePath {
                qself: None,
                path: bound.path,
            })
        })
    }

    fn ret(text: &str) -> ReturnType {
        syn::parse_str(text).expect("a return type")
    }

    const WIDGET: &str = "generated::contract::v1::Widget";

    fn message(path: &str, full_name: &str) -> Wire {
        Wire::Message(ProtoRef {
            path: path.to_owned(),
            full_name: full_name.to_owned(),
        })
    }

    fn widget() -> Wire {
        message(WIDGET, "contract.v1.Widget")
    }

    fn wire(text: &str) -> Wire {
        classify_wire(&ty(text), "parameter `p`").expect("allowed")
    }

    fn refusal(text: &str) -> String {
        classify_wire(&ty(text), "parameter `p`")
            .expect_err("refused")
            .to_string()
    }

    #[test]
    fn every_scalar_crosses_as_its_wrapper() {
        let rows = [
            ("bool", Wrapper::Bool, "google.protobuf.BoolValue"),
            ("i32", Wrapper::Int32, "google.protobuf.Int32Value"),
            ("u32", Wrapper::UInt32, "google.protobuf.UInt32Value"),
            ("i64", Wrapper::Int64, "google.protobuf.Int64Value"),
            ("u64", Wrapper::UInt64, "google.protobuf.UInt64Value"),
            ("f64", Wrapper::Double, "google.protobuf.DoubleValue"),
            ("String", Wrapper::String, "google.protobuf.StringValue"),
            ("Vec<u8>", Wrapper::Bytes, "google.protobuf.BytesValue"),
            (
                "SystemTime",
                Wrapper::Timestamp,
                "google.protobuf.Timestamp",
            ),
            (
                "std::time::Duration",
                Wrapper::Duration,
                "google.protobuf.Duration",
            ),
        ];
        for (text, wrapper, name) in rows {
            assert_eq!(wire(text), Wire::Wrapper(wrapper), "{text}");
            assert_eq!(wrapper.full_name(), name);
            assert_eq!(Wire::Wrapper(wrapper).full_name(), name);
        }
        assert_eq!(Wrapper::Bytes.rust_type(), "Vec<u8>");
        assert_eq!(Wrapper::Timestamp.rust_type(), "SystemTime");
        assert_eq!(Wrapper::Int32.rust_type(), "i32");
    }

    #[test]
    fn unit_is_the_empty_message_and_a_path_is_a_named_message() {
        assert_eq!(wire("()"), Wire::Empty);
        assert_eq!(Wire::Empty.full_name(), "google.protobuf.Empty");
        assert_eq!(wire(WIDGET), widget());
        assert_eq!(wire(&format!("({WIDGET})")), widget());
        assert_eq!(widget().full_name(), "contract.v1.Widget");
        assert_eq!(
            wire("crate::sdk::v1::GetArgs"),
            message("crate::sdk::v1::GetArgs", "sdk.v1.GetArgs")
        );
    }

    #[test]
    fn a_proto_name_is_read_from_the_path() {
        let nested = proto_ref("crate::generated::contract::sdk::v1::Number").expect("ok");
        assert_eq!(nested.full_name, "contract.sdk.v1.Number");
        let plain = proto_ref("contract::v1::Widget").expect("ok");
        assert_eq!(plain.full_name, "contract.v1.Widget");
        let relative = proto_ref("super::self::contract::Widget").expect("ok");
        assert_eq!(relative.full_name, "contract.Widget");
        let message = proto_ref("generated::Widget").expect_err("no package");
        assert!(message.contains("package and a message"), "{message}");
        assert!(proto_ref("Widget").is_err());
    }

    #[test]
    fn a_path_has_no_type_arguments() {
        let ok: syn::Path = syn::parse_str("a::b::C").expect("path");
        assert_eq!(path_text(&ok).expect("ok"), "a::b::C");
        let generic: syn::Path = syn::parse_str("a::B<C>").expect("path");
        assert!(
            path_text(&generic)
                .expect_err("generic")
                .to_string()
                .contains("no type arguments")
        );
    }

    #[test]
    fn a_type_outside_the_allowed_set_is_refused_with_its_reason() {
        let cases = [
            ("Widget", "bare name"),
            ("Vec<String>", "generic type `Vec`"),
            ("Option<u32>", "generic type `Option`"),
            ("HashMap<String, i32>", "generic type `HashMap`"),
            ("&str", "references and lifetimes"),
            ("(i32, i32)", "tuples, arrays and slices"),
            ("[u8; 4]", "tuples, arrays and slices"),
            ("fn(i32)", "function pointers"),
            ("Fn(i32)", "closures and function types"),
            ("impl Trait", "generics and trait types"),
            ("*const u8", "this form of type"),
            ("Foo<'a>", "generic type `Foo`"),
            ("generated::Widget", "package and a message"),
        ];
        for (text, reason) in cases {
            let message = refusal(text);
            assert!(message.contains("parameter `p`"), "{text}: {message}");
            assert!(message.contains(reason), "{text}: {message}");
            assert!(message.contains("allowed:"), "{text}: {message}");
        }
    }

    fn errors() -> Env {
        Env::strict(["WidgetError".to_owned()].into_iter().collect())
    }

    fn ret_ok(text: &str, sync: bool) -> Return {
        classify_return(&ret(text), sync, &errors()).expect("allowed")
    }

    fn ret_refusal(text: &str, sync: bool) -> String {
        classify_return(&ret(text), sync, &errors())
            .expect_err("refused")
            .to_string()
    }

    #[test]
    fn an_async_export_returns_a_result_or_a_handle() {
        assert_eq!(
            ret_ok(&format!("-> Result<{WIDGET}, Error>"), false),
            Return::Result {
                ok: widget(),
                error: ErrorRef::Sdk
            }
        );
        assert_eq!(
            ret_ok("-> Result<(), api_bones_sdk::Error>", false),
            Return::Result {
                ok: Wire::Empty,
                error: ErrorRef::Sdk
            }
        );
        assert_eq!(
            ret_ok("-> Result<String, WidgetError>", false),
            Return::Result {
                ok: Wire::Wrapper(Wrapper::String),
                error: ErrorRef::Named("WidgetError".to_owned())
            }
        );
        for (text, kind) in [
            ("ListCall", HandleKind::List),
            ("api_bones_sdk::Batch", HandleKind::Batch),
            ("Stream", HandleKind::Stream),
        ] {
            assert_eq!(
                ret_ok(&format!("-> {text}<{WIDGET}>"), false),
                Return::Handle {
                    kind,
                    item: widget()
                }
            );
        }
        assert_eq!(
            ret_ok("-> ListCall<u32>", false),
            Return::Handle {
                kind: HandleKind::List,
                item: Wire::Wrapper(Wrapper::UInt32)
            }
        );
    }

    #[test]
    fn a_sync_export_returns_unit_a_value_or_a_result() {
        assert_eq!(ret_ok("", true), Return::Unit);
        assert_eq!(ret_ok("-> ()", true), Return::Unit);
        assert_eq!(
            ret_ok("-> String", true),
            Return::Value(Wire::Wrapper(Wrapper::String))
        );
        assert_eq!(
            ret_ok("-> Result<u32, Error>", true),
            Return::Result {
                ok: Wire::Wrapper(Wrapper::UInt32),
                error: ErrorRef::Sdk
            }
        );
    }

    #[test]
    fn a_return_outside_the_lock_is_refused_with_its_reason() {
        let cases = [
            ("", false, "an async export returns Result<T, E>"),
            ("-> ()", false, "an async export returns Result<T, E>"),
            ("-> String", false, "an async export returns Result<T, E>"),
            ("-> Missing", true, "bare name"),
            ("-> ListCall<Widget>", true, "never returns a handle"),
            (
                "-> ListCall<()>",
                false,
                "a handle yields a message or a scalar",
            ),
            (
                "-> ListCall<String, String>",
                false,
                "a handle takes exactly one type",
            ),
            ("-> Result<String>", false, "Result takes both T and E"),
            ("-> Result<HashMap<String, i32>, Error>", false, "HashMap"),
            (
                "-> Result<String, String>",
                false,
                "the error type is api_bones_sdk::Error",
            ),
            (
                "-> Result<String, Missing>",
                false,
                "the error type is api_bones_sdk::Error",
            ),
            (
                "-> Result<String, &Error>",
                false,
                "the error type is api_bones_sdk::Error",
            ),
            ("-> &str", true, "references and lifetimes"),
        ];
        for (text, sync, reason) in cases {
            let message = ret_refusal(text, sync);
            assert!(message.contains("the return type"), "{text}: {message}");
            assert!(message.contains(reason), "{text}: {message}");
        }
    }

    #[test]
    fn a_deferred_env_accepts_any_named_error_type() {
        let parsed = classify_return(&ret("-> Result<(), Anything>"), false, &Env::deferred());
        assert_eq!(
            parsed.expect("deferred"),
            Return::Result {
                ok: Wire::Empty,
                error: ErrorRef::Named("Anything".to_owned())
            }
        );
    }

    #[test]
    fn handle_kinds_name_their_rust_types() {
        assert_eq!(HandleKind::List.type_name(), "ListCall");
        assert_eq!(HandleKind::Batch.type_name(), "Batch");
        assert_eq!(HandleKind::Stream.type_name(), "Stream");
        assert_eq!(HandleKind::of("Other"), None);
    }

    #[test]
    fn generic_head_of_a_non_path_has_no_name() {
        let reference = ty("&str");
        let (name, args) = generic_head(&reference);
        assert_eq!(name, "");
        assert!(args.is_empty());
        let closure = ty("Fn(i32)");
        let (name, args) = generic_head(&closure);
        assert_eq!(name, "Fn");
        assert!(args.is_empty());
    }
}
