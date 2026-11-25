// SPDX-License-Identifier: MIT
//! The `l2-methods.json` document: which wasm export serves which endpoints,
//! and what the TypeScript method is called.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::model::Model;
use crate::names::{NameError, export_name, typescript_name};

/// The schema id of the document.
pub const SCHEMA: &str = "api-bones.l2-methods/v1";

/// What the SPEC states that the document is built against.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpecL2Methods {
    /// The ids of `sdk_surfaces`.
    pub surfaces: BTreeSet<String>,
    /// For each endpoint (`pkg.Svc/Method`), the Rust method names its
    /// `l2_methods` entry lists under `rust`.
    pub endpoints: BTreeMap<String, Vec<String>>,
}

/// One method of the document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct L2Method {
    /// The wasm export, `<svc>__<Type>__<method>`.
    pub export: String,
    /// The TypeScript method.
    pub typescript: String,
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
}

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
    /// Two exports share a TypeScript spelling.
    DuplicateTypescript {
        /// The TypeScript method.
        typescript: String,
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
            Self::DuplicateTypescript { typescript } => {
                write!(
                    f,
                    "two exports are both the TypeScript method `{typescript}`"
                )
            }
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
/// Sync exports (the constructor, build info and any `#[export(sync)]`) have no
/// endpoint and are left out of `methods`.
///
/// # Errors
///
/// An [`L2Error`] for an export that names a surface the SPEC does not list, a
/// non-sync export no entry lists, a duplicate export or TypeScript spelling,
/// or a part of a name that contains `__`.
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
            if export.sync {
                continue;
            }
            let endpoints: Vec<String> = spec
                .endpoints
                .iter()
                .filter(|(_, rust)| rust.contains(&export.method))
                .map(|(endpoint, _)| endpoint.clone())
                .collect();
            if endpoints.is_empty() {
                return Err(L2Error::NoEndpoint {
                    method: export.method.clone(),
                });
            }
            methods.push(L2Method {
                export: export_name(service, &client.ty, &export.method)?,
                typescript: typescript_name(&export.method),
                endpoints,
            });
        }
    }
    methods.sort_by(|a, b| a.export.cmp(&b.export));
    let mut exports = BTreeSet::new();
    let mut spellings = BTreeSet::new();
    for method in &methods {
        if !exports.insert(&method.export) {
            return Err(L2Error::DuplicateExport {
                export: method.export.clone(),
            });
        }
        if !spellings.insert(&method.typescript) {
            return Err(L2Error::DuplicateTypescript {
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

    fn spec(entries: &[(&str, &[&str])]) -> SpecL2Methods {
        SpecL2Methods {
            surfaces: ["s".to_owned(), "t".to_owned()].into_iter().collect(),
            endpoints: entries
                .iter()
                .map(|(endpoint, rust)| {
                    (
                        (*endpoint).to_owned(),
                        rust.iter().map(|m| (*m).to_owned()).collect(),
                    )
                })
                .collect(),
        }
    }

    fn build(source: &str, spec: &SpecL2Methods) -> Result<L2Methods, L2Error> {
        let model = Model::from_source(source).expect("a model");
        l2_methods(&model, spec, "api-bones-test", "test-l2", "@brefwiz/test")
    }

    const CLIENT: &str = r#"
        #[record] struct R { x: i32 }
        #[client(surface = "s")]
        impl ZClient {
            #[export] async fn zeta(&self) -> Result<R, Error> { body() }
            #[export] async fn alpha_beta(&self) -> Result<R, Error> { body() }
            #[export(sync)] fn label(&self) -> String { body() }
        }
    "#;

    #[test]
    fn methods_are_sorted_spelled_and_given_every_endpoint_listing_them() {
        let spec = spec(&[
            ("p.S/Z", &["zeta"]),
            ("p.S/A", &["alpha_beta", "zeta"]),
            ("p.S/Other", &["unrelated"]),
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
    fn the_document_round_trips_through_its_text() {
        let spec = spec(&[("p.S/A", &["alpha_beta", "zeta"])]);
        let document = build(CLIENT, &spec).expect("ok");
        let text = document.to_json();
        assert!(text.ends_with("}\n"));
        assert!(text.contains("\"crate\": \"test-l2\""));
        let back: L2Methods = serde_json::from_str(&text).expect("json");
        assert_eq!(back, document);
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
        let error = build(CLIENT, &spec(&[("p.S/A", &["alpha_beta"])])).expect_err("refused");
        assert_eq!(
            error,
            L2Error::NoEndpoint {
                method: "zeta".to_owned()
            }
        );
        assert!(error.to_string().contains("no `l2_methods` entry"));
    }

    #[test]
    fn a_name_with_a_double_underscore_is_refused() {
        let source = r#"
            #[record] struct R { x: i32 }
            #[client(surface = "s")]
            impl C { #[export] async fn a__b(&self) -> Result<R, Error> { body() } }
        "#;
        let error = build(source, &spec(&[("p.S/A", &["a__b"])])).expect_err("refused");
        assert!(matches!(error, L2Error::Name(_)));
        assert!(error.to_string().contains("`__`"));
    }

    #[test]
    fn a_repeated_client_type_and_a_repeated_spelling_are_refused() {
        let twice = r#"
            #[record] struct R { x: i32 }
            #[client(surface = "s")]
            impl C { #[export] async fn go(&self) -> Result<R, Error> { body() } }
            #[client(surface = "t")]
            impl C { #[export] async fn go(&self) -> Result<R, Error> { body() } }
        "#;
        let spec_go = spec(&[("p.S/A", &["go"])]);
        let error = build(twice, &spec_go).expect_err("refused");
        assert_eq!(
            error,
            L2Error::DuplicateExport {
                export: "api_bones_test__C__go".to_owned()
            }
        );
        assert!(error.to_string().contains("defined twice"));
        let spelled = r#"
            #[record] struct R { x: i32 }
            #[client(surface = "s")]
            impl C { #[export] async fn go(&self) -> Result<R, Error> { body() } }
            #[client(surface = "s")]
            impl D { #[export] async fn go(&self) -> Result<R, Error> { body() } }
        "#;
        let error = build(spelled, &spec_go).expect_err("refused");
        assert_eq!(
            error,
            L2Error::DuplicateTypescript {
                typescript: "go".to_owned()
            }
        );
        assert!(error.to_string().contains("TypeScript method `go`"));
    }
}
