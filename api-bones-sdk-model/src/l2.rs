// SPDX-License-Identifier: MIT
//! The `l2-methods.json` document: which wasm export serves which endpoints,
//! what the TypeScript method is called, and which client Type, surface and
//! kind it belongs to.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::model::{Model, Shape};
use crate::names::{NameError, export_name, typescript_name};
use crate::ty::{HandleKind, Return};

/// The schema id of the document.
pub const SCHEMA: &str = "api-bones.l2-methods/v1";

/// What the SPEC states that the document is built against.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpecL2Methods {
    /// The ids of `sdk_surfaces`.
    pub surfaces: BTreeSet<String>,
    /// For each surface id, and within it each endpoint (`pkg.Svc/Method`), the
    /// Rust method names its `l2_methods` entry lists under `rust`. A method
    /// is looked up only among the endpoints of its own surface.
    pub endpoints: BTreeMap<String, BTreeMap<String, Vec<String>>>,
}

/// What shape of call an export is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum L2Kind {
    /// An async export returning `Result<T, E>`.
    Unary,
    /// A plain `fn` export returning `ListCall<T>`.
    List,
    /// A plain `fn` export returning `Batch<T>`.
    Batch,
    /// An async export returning `Stream<T>`.
    Stream,
    /// A sync export; it serves no endpoint, so its `endpoints` list is empty.
    Sync,
}

impl L2Kind {
    fn of(ret: &Return) -> Self {
        match ret {
            Return::Handle { kind, .. } => match kind {
                HandleKind::List => Self::List,
                HandleKind::Batch => Self::Batch,
                HandleKind::Stream => Self::Stream,
            },
            _ => Self::Unary,
        }
    }
}

/// One method of the document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct L2Method {
    /// The wasm export, `<svc>__<Type>__<method>`.
    pub export: String,
    /// The client Type the method belongs to.
    #[serde(rename = "type")]
    pub ty: String,
    /// The Rust method name.
    pub rust: String,
    /// The TypeScript method.
    pub typescript: String,
    /// The `sdk_surfaces` id the method is exported on.
    pub surface: String,
    /// What shape of call the method is.
    pub kind: L2Kind,
    /// The endpoints the method calls, `pkg.Svc/Method`, sorted.
    pub endpoints: Vec<String>,
}

/// The document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct L2Methods {
    /// Always [`SCHEMA`].
    pub schema: String,
    /// The SPEC `service`.
    pub service: String,
    /// The Layer 2 crate.
    #[serde(rename = "crate")]
    pub crate_name: String,
    /// The npm package the wrapper publishes.
    pub package: String,
    /// The methods, sorted by export.
    pub methods: Vec<L2Method>,
}

impl L2Methods {
    /// The document as the text the snapshot holds: two-space indent and a
    /// final newline.
    ///
    /// # Panics
    ///
    /// Never: the document holds only strings.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).expect("strings serialise");
        text.push('\n');
        text
    }

    /// Reads a snapshot.
    ///
    /// # Errors
    ///
    /// An [`L2ReadError`] naming the missing or malformed field, or the schema
    /// the document declares when it is not [`SCHEMA`]. A document of an older
    /// schema is refused, never read through.
    pub fn from_json(text: &str) -> Result<Self, L2ReadError> {
        let document: Self =
            serde_json::from_str(text).map_err(|e| L2ReadError::Malformed(e.to_string()))?;
        if document.schema == SCHEMA {
            Ok(document)
        } else {
            Err(L2ReadError::Schema(document.schema))
        }
    }
}

/// Why a snapshot cannot be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum L2ReadError {
    /// The text is not a document of the current shape; the message names the
    /// field.
    Malformed(String),
    /// The document declares another schema.
    Schema(String),
}

impl fmt::Display for L2ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(message) => write!(f, "not an `{SCHEMA}` snapshot: {message}"),
            Self::Schema(schema) => {
                write!(f, "snapshot schema is `{schema}`, expected `{SCHEMA}`")
            }
        }
    }
}

impl std::error::Error for L2ReadError {}

/// Why a document cannot be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum L2Error {
    /// A part of an export name cannot be spelled.
    Name(NameError),
    /// An export names a surface the SPEC does not list.
    UnknownSurface {
        /// The Rust method.
        method: String,
        /// The surface it names.
        surface: String,
    },
    /// A non-sync export no `l2_methods` entry lists.
    NoEndpoint {
        /// The Rust method.
        method: String,
    },
    /// Two exports share a name.
    DuplicateExport {
        /// The export name.
        export: String,
    },
    /// Two exports of one client Type share a TypeScript spelling.
    DuplicateTypescript {
        /// The client Type.
        ty: String,
        /// The TypeScript method.
        typescript: String,
    },
    /// Two client Types on one surface export the same Rust method name, so
    /// the `rust` lists of that surface cannot tell them apart.
    DuplicateMethod {
        /// The surface.
        surface: String,
        /// The Rust method.
        method: String,
    },
}

impl fmt::Display for L2Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name(error) => write!(f, "{error}"),
            Self::UnknownSurface { method, surface } => write!(
                f,
                "export `{method}` names surface `{surface}`, which `sdk_surfaces` does not list"
            ),
            Self::NoEndpoint { method } => write!(
                f,
                "export `{method}` is not sync and no `l2_methods` entry lists it under `rust`"
            ),
            Self::DuplicateExport { export } => write!(f, "export `{export}` is defined twice"),
            Self::DuplicateTypescript { ty, typescript } => write!(
                f,
                "two exports of `{ty}` are both the TypeScript method `{typescript}`"
            ),
            Self::DuplicateMethod { surface, method } => write!(
                f,
                "two client types on surface `{surface}` both export `{method}`"
            ),
        }
    }
}

impl std::error::Error for L2Error {}

impl From<NameError> for L2Error {
    fn from(error: NameError) -> Self {
        Self::Name(error)
    }
}

/// Builds the document for one Layer 2 crate.
///
/// Sync methods (the constructor, build info and any `#[export(sync)]` taking
/// `&self`) are left out of `methods`. A static function or constant export is
/// listed with kind `sync` and no endpoints.
///
/// # Errors
///
/// An [`L2Error`] for an export that names a surface the SPEC does not list, a
/// non-sync export no entry lists, a duplicate export, a TypeScript spelling
/// repeated within one client Type, a Rust method repeated across the client
/// Types of one surface, or a part of a name that contains `__`.
pub fn l2_methods(
    model: &Model,
    spec: &SpecL2Methods,
    service: &str,
    crate_name: &str,
    package: &str,
) -> Result<L2Methods, L2Error> {
    let mut methods = Vec::new();
    for client in &model.clients {
        for export in &client.exports {
            if !spec.surfaces.contains(&export.surface) {
                return Err(L2Error::UnknownSurface {
                    method: export.method.clone(),
                    surface: export.surface.clone(),
                });
            }
            if export.sync && export.shape == Shape::Method {
                continue;
            }
            let endpoints: Vec<String> = if export.sync {
                Vec::new()
            } else {
                spec.endpoints
                    .get(&export.surface)
                    .into_iter()
                    .flatten()
                    .filter(|(_, rust)| rust.contains(&export.method))
                    .map(|(endpoint, _)| endpoint.clone())
                    .collect()
            };
            if endpoints.is_empty() && !export.sync {
                return Err(L2Error::NoEndpoint {
                    method: export.method.clone(),
                });
            }
            methods.push(L2Method {
                export: export_name(service, &client.ty, &export.method)?,
                ty: client.ty.clone(),
                rust: export.method.clone(),
                typescript: typescript_name(&export.method),
                surface: export.surface.clone(),
                kind: if export.sync {
                    L2Kind::Sync
                } else {
                    L2Kind::of(&export.ret)
                },
                endpoints,
            });
        }
    }
    methods.sort_by(|a, b| a.export.cmp(&b.export));
    let mut exports = BTreeSet::new();
    let mut spellings = BTreeSet::new();
    let mut surface_methods = BTreeSet::new();
    for method in &methods {
        if !exports.insert(&method.export) {
            return Err(L2Error::DuplicateExport {
                export: method.export.clone(),
            });
        }
        if !surface_methods.insert((&method.surface, &method.rust)) {
            return Err(L2Error::DuplicateMethod {
                surface: method.surface.clone(),
                method: method.rust.clone(),
            });
        }
        if !spellings.insert((&method.ty, &method.typescript)) {
            return Err(L2Error::DuplicateTypescript {
                ty: method.ty.clone(),
                typescript: method.typescript.clone(),
            });
        }
    }
    Ok(L2Methods {
        schema: SCHEMA.to_owned(),
        service: service.to_owned(),
        crate_name: crate_name.to_owned(),
        package: package.to_owned(),
        methods,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each entry is (surface, endpoint, Rust names the endpoint lists).
    fn spec(entries: &[(&str, &str, &[&str])]) -> SpecL2Methods {
        let mut spec = SpecL2Methods {
            surfaces: ["s".to_owned(), "t".to_owned()].into_iter().collect(),
            endpoints: BTreeMap::new(),
        };
        for (surface, endpoint, rust) in entries {
            spec.endpoints
                .entry((*surface).to_owned())
                .or_default()
                .insert(
                    (*endpoint).to_owned(),
                    rust.iter().map(|m| (*m).to_owned()).collect(),
                );
        }
        spec
    }

    fn build(source: &str, spec: &SpecL2Methods) -> Result<L2Methods, L2Error> {
        let model = Model::from_source(source).expect("a model");
        l2_methods(&model, spec, "api-bones-test", "test-l2", "@brefwiz/test")
    }

    const CLIENT: &str = r#"
        #[client(surface = "s")]
        impl ZClient {
            #[export] async fn zeta(&self) -> Result<generated::p::v1::R, Error> { body() }
            #[export] async fn alpha_beta(&self) -> Result<generated::p::v1::R, Error> { body() }
            #[export(sync)] fn label(&self) -> String { body() }
        }
    "#;

    /// Two Types share the surface `s`; each has one async export.
    const TWO_TYPES: &str = r#"
        #[client(surface = "s")]
        impl AClient {
            #[export] async fn fetch_one(&self) -> Result<generated::p::v1::R, Error> { body() }
        }
        #[client(surface = "s")]
        impl BClient {
            #[export] async fn watch_all(&self) -> Stream<generated::p::v1::R> { body() }
        }
    "#;

    #[test]
    fn a_static_function_and_a_constant_are_sync_methods_with_no_endpoint() {
        let source = r#"
            #[client(surface = "s", proto = generated::p::v1)]
            impl Tools {
                #[export(sync)] pub const LIMIT: u32 = 3;
                #[export(sync)] pub fn clamp(value: u32) -> u32 { body() }
                #[export(sync)] fn label(&self) -> String { body() }
            }
        "#;
        let document = build(source, &spec(&[])).expect("statics need no endpoint");
        let listed: Vec<_> = document
            .methods
            .iter()
            .map(|m| {
                (
                    m.export.as_str(),
                    m.typescript.as_str(),
                    m.kind,
                    m.endpoints.len(),
                )
            })
            .collect();
        assert_eq!(
            listed,
            [
                ("api_bones_test__Tools__LIMIT", "limit", L2Kind::Sync, 0),
                ("api_bones_test__Tools__clamp", "clamp", L2Kind::Sync, 0),
            ]
        );
    }

    #[test]
    fn methods_are_sorted_spelled_and_given_every_endpoint_listing_them() {
        let spec = spec(&[
            ("s", "p.S/Z", &["zeta"]),
            ("s", "p.S/A", &["alpha_beta", "zeta"]),
            ("s", "p.S/Other", &["unrelated"]),
        ]);
        let document = build(CLIENT, &spec).expect("ok");
        assert_eq!(document.schema, SCHEMA);
        assert_eq!(document.crate_name, "test-l2");
        let exports: Vec<_> = document.methods.iter().map(|m| m.export.as_str()).collect();
        assert_eq!(
            exports,
            [
                "api_bones_test__ZClient__alpha_beta",
                "api_bones_test__ZClient__zeta"
            ]
        );
        assert_eq!(document.methods[0].typescript, "alphaBeta");
        assert_eq!(document.methods[0].endpoints, ["p.S/A"]);
        assert_eq!(document.methods[1].endpoints, ["p.S/A", "p.S/Z"]);
    }

    #[test]
    fn each_method_carries_its_type_rust_name_surface_and_kind() {
        let spec = spec(&[
            ("s", "p.S/One", &["fetch_one"]),
            ("s", "p.S/Watch", &["watch_all"]),
        ]);
        let document = build(TWO_TYPES, &spec).expect("ok");
        let [one, watch] = document.methods.as_slice() else {
            panic!("two methods: {:?}", document.methods);
        };
        assert_eq!(one.export, "api_bones_test__AClient__fetch_one");
        assert_eq!(one.ty, "AClient");
        assert_eq!(one.rust, "fetch_one");
        assert_eq!(one.typescript, "fetchOne");
        assert_eq!(one.surface, "s");
        assert_eq!(one.kind, L2Kind::Unary);
        assert_eq!(one.endpoints, ["p.S/One"]);
        assert_eq!(watch.export, "api_bones_test__BClient__watch_all");
        assert_eq!(watch.ty, "BClient");
        assert_eq!(watch.rust, "watch_all");
        assert_eq!(watch.typescript, "watchAll");
        assert_eq!(watch.surface, "s");
        assert_eq!(watch.kind, L2Kind::Stream);
        assert_eq!(watch.endpoints, ["p.S/Watch"]);
    }

    #[test]
    fn a_handle_export_is_its_handle_kind_and_a_result_export_is_unary() {
        let source = r#"
            #[client(surface = "s")]
            impl C {
                #[export] fn a(&self) -> ListCall<generated::p::v1::R> { body() }
                #[export] fn b(&self) -> Batch<generated::p::v1::R> { body() }
                #[export] async fn c(&self) -> Stream<generated::p::v1::R> { body() }
                #[export] async fn d(&self) -> Result<generated::p::v1::R, Error> { body() }
            }
        "#;
        let spec = spec(&[("s", "p.S/X", &["a", "b", "c", "d"])]);
        let kinds: Vec<_> = build(source, &spec)
            .expect("ok")
            .methods
            .iter()
            .map(|m| (m.rust.clone(), m.kind))
            .collect();
        assert_eq!(
            kinds,
            [
                ("a".to_owned(), L2Kind::List),
                ("b".to_owned(), L2Kind::Batch),
                ("c".to_owned(), L2Kind::Stream),
                ("d".to_owned(), L2Kind::Unary),
            ]
        );
    }

    #[test]
    fn the_document_round_trips_through_its_text() {
        let spec = spec(&[("s", "p.S/A", &["alpha_beta", "zeta"])]);
        let document = build(CLIENT, &spec).expect("ok");
        let text = document.to_json();
        assert!(text.ends_with("}\n"));
        assert!(text.contains("\"crate\": \"test-l2\""));
        let first = text.find("\"export\"").expect("export");
        let keys: Vec<_> = [
            "export",
            "type",
            "rust",
            "typescript",
            "surface",
            "kind",
            "endpoints",
        ]
        .iter()
        .map(|key| text[first..].find(&format!("\"{key}\":")).expect("key"))
        .collect();
        assert!(keys.is_sorted(), "{keys:?}");
        assert_eq!(L2Methods::from_json(&text).expect("read"), document);
    }

    #[test]
    fn a_document_of_the_earlier_shape_is_refused_naming_the_missing_field() {
        let old = r#"{
            "schema": "api-bones.l2-methods/v1", "service": "s", "crate": "c", "package": "p",
            "methods": [{"export": "s__C__m", "typescript": "m", "endpoints": ["p.S/M"]}]
        }"#;
        let error = L2Methods::from_json(old).expect_err("refused");
        assert!(matches!(error, L2ReadError::Malformed(_)));
        assert!(
            error.to_string().contains("missing field `type`"),
            "{error}"
        );
    }

    #[test]
    fn a_sync_method_with_no_endpoints_is_read() {
        let text = r#"{
            "schema": "api-bones.l2-methods/v1", "service": "s", "crate": "c", "package": "p",
            "methods": [{"export": "s__C__label", "type": "C", "rust": "label",
                "typescript": "label", "surface": "s", "kind": "sync", "endpoints": []}]
        }"#;
        let document = L2Methods::from_json(text).expect("read");
        assert_eq!(document.methods[0].kind, L2Kind::Sync);
        assert_eq!(document.methods[0].endpoints, Vec::<String>::new());
        assert!(document.to_json().contains("\"kind\": \"sync\""));
    }

    #[test]
    fn a_complete_document_of_another_schema_is_refused() {
        let spec = spec(&[("s", "p.S/A", &["alpha_beta", "zeta"])]);
        let mut document = build(CLIENT, &spec).expect("ok");
        document.schema = "api-bones.l2-methods/v0".to_owned();
        let error = L2Methods::from_json(&document.to_json()).expect_err("refused");
        assert_eq!(
            error,
            L2ReadError::Schema("api-bones.l2-methods/v0".to_owned())
        );
        assert!(
            error
                .to_string()
                .contains("expected `api-bones.l2-methods/v1`")
        );
    }

    #[test]
    fn a_document_missing_any_field_of_a_method_is_refused() {
        let spec = spec(&[("s", "p.S/A", &["alpha_beta", "zeta"])]);
        let text = build(CLIENT, &spec).expect("ok").to_json();
        for field in ["type", "rust", "surface", "kind"] {
            let mut value: serde_json::Value = serde_json::from_str(&text).expect("json");
            value["methods"][0]
                .as_object_mut()
                .expect("object")
                .remove(field);
            let error = L2Methods::from_json(&value.to_string()).expect_err("refused");
            assert!(
                error
                    .to_string()
                    .contains(&format!("missing field `{field}`")),
                "{field}: {error}"
            );
        }
    }

    #[test]
    fn an_unlisted_surface_is_refused_even_for_a_sync_export() {
        let source = r#"
            #[client(surface = "missing")]
            impl C { #[export(sync)] fn label(&self) -> String { body() } }
        "#;
        let error = build(source, &spec(&[])).expect_err("refused");
        assert_eq!(
            error,
            L2Error::UnknownSurface {
                method: "label".to_owned(),
                surface: "missing".to_owned()
            }
        );
        assert!(error.to_string().contains("does not list"));
    }

    #[test]
    fn a_method_the_spec_does_not_list_is_refused() {
        let error = build(CLIENT, &spec(&[("s", "p.S/A", &["alpha_beta"])])).expect_err("refused");
        assert_eq!(
            error,
            L2Error::NoEndpoint {
                method: "zeta".to_owned()
            }
        );
        assert!(error.to_string().contains("no `l2_methods` entry"));
    }

    #[test]
    fn an_endpoint_listed_on_another_surface_is_not_found() {
        let error =
            build(CLIENT, &spec(&[("t", "p.S/A", &["alpha_beta", "zeta"])])).expect_err("refused");
        assert_eq!(
            error,
            L2Error::NoEndpoint {
                method: "zeta".to_owned()
            }
        );
    }

    #[test]
    fn a_name_with_a_double_underscore_is_refused() {
        let source = r#"
            #[client(surface = "s")]
            impl C { #[export] async fn a__b(&self) -> Result<generated::p::v1::R, Error> { body() } }
        "#;
        let error = build(source, &spec(&[("s", "p.S/A", &["a__b"])])).expect_err("refused");
        assert!(matches!(error, L2Error::Name(_)));
        assert!(error.to_string().contains("`__`"));
    }

    #[test]
    fn a_repeated_client_type_is_refused() {
        let twice = r#"
            #[client(surface = "s")]
            impl C { #[export] async fn go(&self) -> Result<generated::p::v1::R, Error> { body() } }
            #[client(surface = "t")]
            impl C { #[export] async fn go(&self) -> Result<generated::p::v1::R, Error> { body() } }
        "#;
        let spec_go = spec(&[("s", "p.S/A", &["go"]), ("t", "p.S/A", &["go"])]);
        let error = build(twice, &spec_go).expect_err("refused");
        assert_eq!(
            error,
            L2Error::DuplicateExport {
                export: "api_bones_test__C__go".to_owned()
            }
        );
        assert!(error.to_string().contains("defined twice"));
    }

    #[test]
    fn a_spelling_repeated_within_one_type_is_refused() {
        let spelled = r#"
            #[client(surface = "s")]
            impl C {
                #[export] async fn a_b(&self) -> Result<generated::p::v1::R, Error> { body() }
                #[export] async fn a_B(&self) -> Result<generated::p::v1::R, Error> { body() }
            }
        "#;
        let error = build(spelled, &spec(&[("s", "p.S/A", &["a_b", "a_B"])])).expect_err("refused");
        assert_eq!(
            error,
            L2Error::DuplicateTypescript {
                ty: "C".to_owned(),
                typescript: "aB".to_owned()
            }
        );
        assert!(error.to_string().contains("two exports of `C`"));
    }

    const SHARED_NAME: &str = r#"
        #[client(surface = "s")]
        impl C { #[export] async fn authorize(&self) -> Result<generated::p::v1::R, Error> { body() } }
        #[client(surface = "t")]
        impl D { #[export] async fn authorize(&self) -> Result<generated::p::v1::R, Error> { body() } }
    "#;

    #[test]
    fn one_method_name_on_two_types_of_different_surfaces_resolves_per_surface() {
        let spec = spec(&[
            ("s", "p.Device/Authorize", &["authorize"]),
            ("t", "p.Flow/Authorize", &["authorize"]),
        ]);
        let document = build(SHARED_NAME, &spec).expect("ok");
        let [c, d] = document.methods.as_slice() else {
            panic!("two methods: {:?}", document.methods);
        };
        assert_eq!((c.ty.as_str(), c.typescript.as_str()), ("C", "authorize"));
        assert_eq!(c.endpoints, ["p.Device/Authorize"]);
        assert_eq!((d.ty.as_str(), d.typescript.as_str()), ("D", "authorize"));
        assert_eq!(d.endpoints, ["p.Flow/Authorize"]);
    }

    #[test]
    fn one_method_name_on_two_types_of_one_surface_is_refused() {
        let same_surface = SHARED_NAME.replace("\"t\"", "\"s\"");
        let spec = spec(&[("s", "p.Device/Authorize", &["authorize"])]);
        let error = build(&same_surface, &spec).expect_err("refused");
        assert_eq!(
            error,
            L2Error::DuplicateMethod {
                surface: "s".to_owned(),
                method: "authorize".to_owned()
            }
        );
        assert!(error.to_string().contains("on surface `s`"));
    }
}
