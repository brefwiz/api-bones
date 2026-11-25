// SPDX-License-Identifier: MIT
//! The types that may cross the wasm boundary, and the one classifier that
//! decides it.
//!
//! The wire is JSON text both ways. A parameter is a scalar, a byte string,
//! a timestamp, a time span, an option, a list, a string-keyed map, or a type
//! that carries `#[record]` or `#[enumeration]`. A return is `Result<T, E>`
//! or a handle (`ListCall<T>`, `Batch<T>`, `Stream<T>`); a sync export returns
//! a plain value instead and never a handle.

use std::collections::BTreeSet;

use proc_macro2::Span;
use syn::{GenericArgument, PathArguments, ReturnType, Type, TypePath};

/// The allowed set, as every refusal names it.
pub const ALLOWED: &str = "bool, i32, u32, f64, String, i64, u64, Vec<u8>, SystemTime, Duration, \
Option<T>, Vec<T>, BTreeMap<String, T> (T itself allowed), or a type that carries #[record] or \
#[enumeration]";

/// A type of the allowed set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireType {
    /// `bool`.
    Bool,
    /// `i32`.
    I32,
    /// `u32`.
    U32,
    /// `f64`.
    F64,
    /// `String`.
    String,
    /// `i64`, which crosses as a decimal string.
    I64,
    /// `u64`, which crosses as a decimal string.
    U64,
    /// `Vec<u8>`, which crosses as a padded base64 string.
    Bytes,
    /// `std::time::SystemTime`, which crosses as an RFC 3339 string.
    SystemTime,
    /// `std::time::Duration`, which crosses as whole milliseconds.
    Duration,
    /// `Option<T>`.
    Option(Box<WireType>),
    /// `Vec<T>`.
    List(Box<WireType>),
    /// `BTreeMap<String, T>`.
    Map(Box<WireType>),
    /// A type that carries `#[record]` or `#[enumeration]`, by name.
    Named(String),
}

/// What the classifier knows about the crate's own types.
///
/// A macro sees one item at a time, so it cannot tell whether a named type
/// carries `#[record]`; it uses [`Env::deferred`] and leaves that to the
/// `Wire` bound its expansion asserts. The generator has parsed the whole
/// crate and uses [`Env::strict`].
#[derive(Debug, Clone, Default)]
pub struct Env {
    named: Option<BTreeSet<String>>,
    errors: Option<BTreeSet<String>>,
}

impl Env {
    /// Every named type and every error type is accepted here and checked
    /// later.
    #[must_use]
    pub fn deferred() -> Self {
        Self::default()
    }

    /// Only the given record and enumeration names, and error type names, are
    /// accepted.
    #[must_use]
    pub fn strict(named: BTreeSet<String>, errors: BTreeSet<String>) -> Self {
        Self {
            named: Some(named),
            errors: Some(errors),
        }
    }

    fn knows(&self, name: &str) -> bool {
        self.named.as_ref().is_none_or(|named| named.contains(name))
    }

    fn knows_error(&self, name: &str) -> bool {
        self.errors
            .as_ref()
            .is_none_or(|errors| errors.contains(name))
    }
}

/// Classifies one parameter or item type.
///
/// `what` names the position in a refusal, such as ``parameter `limit` ``.
///
/// # Errors
///
/// A [`syn::Error`] spanning the type, whose message names `what`, the
/// reason and the allowed set.
pub fn classify(ty: &Type, env: &Env, what: &str) -> syn::Result<WireType> {
    let refuse =
        |reason: &str| syn::Error::new_spanned(ty, format!("{what}: {reason}; allowed: {ALLOWED}"));
    match ty {
        Type::Path(TypePath { qself: None, path }) => {
            let segment = path
                .segments
                .last()
                .ok_or_else(|| refuse("an empty path is not a type"))?;
            let name = segment.ident.to_string();
            match &segment.arguments {
                PathArguments::None => classify_plain(&name, env).map_err(|reason| refuse(&reason)),
                PathArguments::AngleBracketed(args) => {
                    let types = type_args(args.args.iter())
                        .ok_or_else(|| refuse("lifetimes and const generics are not allowed"))?;
                    classify_generic(&name, &types, env, what).map_err(|reason| match reason {
                        Refusal::Reason(text) => refuse(&text),
                        Refusal::Inner(error) => error,
                    })
                }
                PathArguments::Parenthesized(_) => {
                    Err(refuse("closures and function types cannot cross"))
                }
            }
        }
        Type::Paren(inner) => classify(&inner.elem, env, what),
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

/// Why a generic type is refused: the classifier's own reason, or the error
/// of a type argument it recursed into.
enum Refusal {
    Reason(String),
    Inner(syn::Error),
}

fn classify_plain(name: &str, env: &Env) -> Result<WireType, String> {
    match name {
        "bool" => Ok(WireType::Bool),
        "i32" => Ok(WireType::I32),
        "u32" => Ok(WireType::U32),
        "f64" => Ok(WireType::F64),
        "String" => Ok(WireType::String),
        "i64" => Ok(WireType::I64),
        "u64" => Ok(WireType::U64),
        "SystemTime" => Ok(WireType::SystemTime),
        "Duration" => Ok(WireType::Duration),
        "i8" | "i16" | "u8" | "u16" => Err("8 and 16 bit integers are not allowed".to_owned()),
        "i128" | "u128" => Err("128 bit integers are not allowed".to_owned()),
        "usize" | "isize" => Err("pointer-sized integers are not allowed".to_owned()),
        "f32" => Err("f32 is not allowed, use f64".to_owned()),
        "char" => Err("char is not allowed, use String".to_owned()),
        "str" => Err("str is not allowed, use String".to_owned()),
        other if env.knows(other) => Ok(WireType::Named(other.to_owned())),
        other => Err(format!(
            "`{other}` carries neither #[record] nor #[enumeration]"
        )),
    }
}

fn classify_generic(
    name: &str,
    types: &[&Type],
    env: &Env,
    what: &str,
) -> Result<WireType, Refusal> {
    let inner = |ty: &Type| {
        classify(ty, env, what)
            .map(Box::new)
            .map_err(Refusal::Inner)
    };
    match (name, types) {
        ("Option", &[item]) => Ok(WireType::Option(inner(item)?)),
        ("Vec", &[item]) if last_ident(item).as_deref() == Some("u8") => Ok(WireType::Bytes),
        ("Vec", &[item]) => Ok(WireType::List(inner(item)?)),
        ("BTreeMap", &[key, value]) => {
            if last_ident(key).as_deref() == Some("String") {
                Ok(WireType::Map(inner(value)?))
            } else {
                Err(Refusal::Reason("BTreeMap keys must be String".to_owned()))
            }
        }
        ("HashMap" | "HashSet" | "Box" | "Rc" | "Arc" | "Cow", _) => Err(Refusal::Reason(
            "HashMap, HashSet, Box, Rc, Arc and Cow are not allowed".to_owned(),
        )),
        _ => Err(Refusal::Reason(format!(
            "generic type `{name}` is not allowed"
        ))),
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
    Value(WireType),
    /// `Result<T, E>`; `ok` is `None` for `()`.
    Result {
        /// The success type, `None` for `()`.
        ok: Option<WireType>,
        /// The error type.
        error: ErrorRef,
    },
    /// A handle over records or enumerations: an async export only.
    Handle {
        /// Which handle.
        kind: HandleKind,
        /// The record or enumeration the handle yields, by name.
        item: String,
    },
}

/// Classifies the return type of an export.
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
        return match classify(item, env, "the handle item")? {
            WireType::Named(item) => Ok(Return::Handle { kind, item }),
            _ => Err(refuse(
                Some(item),
                "a handle yields a type that carries #[record] or #[enumeration]",
            )),
        };
    }
    if name == "Result" {
        let &[ok, error] = args.as_slice() else {
            return Err(refuse(Some(ty), "Result takes both T and E"));
        };
        let ok = if matches!(ok, Type::Tuple(tuple) if tuple.elems.is_empty()) {
            None
        } else {
            Some(classify(ok, env, WHAT)?)
        };
        let error = classify_error(error, env).map_err(|reason| refuse(Some(error), &reason))?;
        return Ok(Return::Result { ok, error });
    }
    if sync {
        classify(ty, env, WHAT).map(Return::Value)
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

    /// `text` as a type; function sugar such as `Fn(i32)` parses only as a
    /// bound, so it is read as one and kept as the path type it is.
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

    fn named(names: &[&str]) -> Env {
        Env::strict(
            names.iter().map(|name| (*name).to_owned()).collect(),
            ["WidgetError".to_owned()].into_iter().collect(),
        )
    }

    fn ok(text: &str) -> WireType {
        classify(&ty(text), &named(&["Widget"]), "parameter `p`").expect("allowed")
    }

    fn refusal(text: &str) -> String {
        classify(&ty(text), &named(&["Widget"]), "parameter `p`")
            .expect_err("refused")
            .to_string()
    }

    #[test]
    fn every_scalar_row_of_the_allowed_set_classifies() {
        assert_eq!(ok("bool"), WireType::Bool);
        assert_eq!(ok("i32"), WireType::I32);
        assert_eq!(ok("u32"), WireType::U32);
        assert_eq!(ok("f64"), WireType::F64);
        assert_eq!(ok("String"), WireType::String);
        assert_eq!(ok("i64"), WireType::I64);
        assert_eq!(ok("u64"), WireType::U64);
        assert_eq!(ok("Vec<u8>"), WireType::Bytes);
        assert_eq!(ok("std::time::SystemTime"), WireType::SystemTime);
        assert_eq!(ok("Duration"), WireType::Duration);
        assert_eq!(ok("(String)"), WireType::String);
    }

    #[test]
    fn containers_nest_and_named_types_are_known_ones() {
        assert_eq!(ok("Widget"), WireType::Named("Widget".to_owned()));
        assert_eq!(
            ok("crate::models::Widget"),
            WireType::Named("Widget".to_owned())
        );
        assert_eq!(
            ok("Option<Vec<Widget>>"),
            WireType::Option(Box::new(WireType::List(Box::new(WireType::Named(
                "Widget".to_owned()
            )))))
        );
        assert_eq!(
            ok("std::collections::BTreeMap<String, Option<i64>>"),
            WireType::Map(Box::new(WireType::Option(Box::new(WireType::I64))))
        );
        assert_eq!(
            ok("Vec<Vec<u8>>"),
            WireType::List(Box::new(WireType::Bytes))
        );
    }

    #[test]
    fn a_deferred_env_accepts_any_named_type() {
        let parsed = classify(&ty("Anything"), &Env::deferred(), "parameter `p`");
        assert_eq!(
            parsed.expect("deferred"),
            WireType::Named("Anything".to_owned())
        );
    }

    #[test]
    fn every_refusal_names_the_parameter_the_reason_and_the_allowed_set() {
        let cases = [
            ("&str", "references and lifetimes"),
            ("&'a Widget", "references and lifetimes"),
            ("(i32, i32)", "tuples, arrays and slices"),
            ("[u8; 4]", "tuples, arrays and slices"),
            ("[u8]", "tuples, arrays and slices"),
            ("fn(i32) -> i32", "function pointers"),
            ("impl Fn()", "generics and trait types"),
            ("dyn Fn()", "generics and trait types"),
            ("_", "generics and trait types"),
            ("*const u8", "this form of type"),
            ("Fn(i32) -> i32", "closures and function types"),
            ("u8", "8 and 16 bit integers"),
            ("i8", "8 and 16 bit integers"),
            ("u16", "8 and 16 bit integers"),
            ("i16", "8 and 16 bit integers"),
            ("i128", "128 bit integers"),
            ("u128", "128 bit integers"),
            ("usize", "pointer-sized integers"),
            ("isize", "pointer-sized integers"),
            ("f32", "f32 is not allowed"),
            ("char", "char is not allowed"),
            ("str", "str is not allowed"),
            (
                "HashMap<String, i32>",
                "HashMap, HashSet, Box, Rc, Arc and Cow",
            ),
            ("HashSet<String>", "HashMap, HashSet, Box, Rc, Arc and Cow"),
            ("Box<String>", "HashMap, HashSet, Box, Rc, Arc and Cow"),
            ("Rc<String>", "HashMap, HashSet, Box, Rc, Arc and Cow"),
            ("Arc<String>", "HashMap, HashSet, Box, Rc, Arc and Cow"),
            ("Cow<'static, str>", "lifetimes and const generics"),
            ("BTreeMap<i32, String>", "BTreeMap keys must be String"),
            ("Foo<String>", "generic type `Foo` is not allowed"),
            (
                "Option<String, String>",
                "generic type `Option` is not allowed",
            ),
            (
                "serde_json::Value",
                "`Value` carries neither #[record] nor #[enumeration]",
            ),
            (
                "Missing",
                "`Missing` carries neither #[record] nor #[enumeration]",
            ),
            ("Vec<&str>", "references and lifetimes"),
            ("<T as Tr>::Out", "this form of type"),
        ];
        for (text, reason) in cases {
            let message = refusal(text);
            assert!(message.contains("parameter `p`"), "{text}: {message}");
            assert!(message.contains(reason), "{text}: {message}");
            assert!(message.contains(ALLOWED), "{text}: {message}");
        }
    }

    fn ret_ok(text: &str, sync: bool) -> Return {
        classify_return(&ret(text), sync, &named(&["Widget"])).expect("allowed")
    }

    fn ret_refusal(text: &str, sync: bool) -> String {
        classify_return(&ret(text), sync, &named(&["Widget"]))
            .expect_err("refused")
            .to_string()
    }

    #[test]
    fn an_async_export_returns_a_result_or_a_handle() {
        assert_eq!(
            ret_ok("-> Result<Widget, Error>", false),
            Return::Result {
                ok: Some(WireType::Named("Widget".to_owned())),
                error: ErrorRef::Sdk
            }
        );
        assert_eq!(
            ret_ok("-> Result<(), api_bones_sdk::Error>", false),
            Return::Result {
                ok: None,
                error: ErrorRef::Sdk
            }
        );
        assert_eq!(
            ret_ok("-> Result<Vec<String>, WidgetError>", false),
            Return::Result {
                ok: Some(WireType::List(Box::new(WireType::String))),
                error: ErrorRef::Named("WidgetError".to_owned())
            }
        );
        for (text, kind) in [
            ("-> ListCall<Widget>", HandleKind::List),
            ("-> api_bones_sdk::Batch<Widget>", HandleKind::Batch),
            ("-> Stream<Widget>", HandleKind::Stream),
        ] {
            assert_eq!(
                ret_ok(text, false),
                Return::Handle {
                    kind,
                    item: "Widget".to_owned()
                }
            );
        }
    }

    #[test]
    fn a_sync_export_returns_unit_a_value_or_a_result() {
        assert_eq!(ret_ok("", true), Return::Unit);
        assert_eq!(ret_ok("-> ()", true), Return::Unit);
        assert_eq!(ret_ok("-> String", true), Return::Value(WireType::String));
        assert_eq!(
            ret_ok("-> Result<u32, Error>", true),
            Return::Result {
                ok: Some(WireType::U32),
                error: ErrorRef::Sdk
            }
        );
    }

    #[test]
    fn a_return_outside_the_lock_is_refused_with_its_reason() {
        let cases = [
            ("", false, "an async export returns Result<T, E>"),
            ("-> ()", false, "an async export returns Result<T, E>"),
            ("-> Widget", false, "an async export returns Result<T, E>"),
            ("-> Missing", true, "carries neither"),
            ("-> ListCall<Widget>", true, "never returns a handle"),
            (
                "-> ListCall<String>",
                false,
                "a handle yields a type that carries",
            ),
            (
                "-> ListCall<Widget, Widget>",
                false,
                "a handle takes exactly one type",
            ),
            ("-> Result<Widget>", false, "Result takes both T and E"),
            ("-> Result<HashMap<String, i32>, Error>", false, "HashMap"),
            (
                "-> Result<Widget, String>",
                false,
                "the error type is api_bones_sdk::Error",
            ),
            (
                "-> Result<Widget, Missing>",
                false,
                "the error type is api_bones_sdk::Error",
            ),
            (
                "-> Result<Widget, &Error>",
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
                ok: None,
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
