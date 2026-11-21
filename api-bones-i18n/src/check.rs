// SPDX-License-Identifier: MIT
//! The `error-catalog` check: an emitter's catalog strings against the
//! arguments each error code declares.
//!
//! A code declares its arguments with `bones.v1.error_args` on its error enum
//! value. A string of that code may use only those names as placeholders,
//! each with a kind the placeholder accepts, and only the `MessageFormat` subset
//! of [`Message`]. A declared argument a string leaves unused is not an
//! issue: extra arguments are ignored when rendering, and the title and the
//! description of a code may each use a different part of them.
//!
//! This crate takes the declarations as plain data and depends on no proto
//! crate; `api_bones_protos::error_args::declared_error_args` reads them from
//! a descriptor.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::{env, fmt, fs};

use serde_json::Value;

use crate::args::key_ok;
use crate::build::scan;
use crate::format::{Expected, Message, ParseError};

/// The kind of value an error argument carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgKind {
    /// Text.
    String,
    /// A whole number.
    Int64,
    /// A real number.
    Double,
    /// An instant.
    Timestamp,
}

impl ArgKind {
    /// Whether a placeholder needing `expected` accepts this kind.
    #[must_use]
    pub const fn accepts(self, expected: Expected) -> bool {
        matches!(
            (self, expected),
            (Self::String, Expected::String)
                | (Self::Int64 | Self::Double, Expected::Number)
                | (Self::Timestamp, Expected::Timestamp)
        )
    }
}

/// One argument a code declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArgDeclaration {
    /// The placeholder name: `^[a-z][a-z0-9_]{0,63}$`.
    pub name: String,
    /// The kind of value the emitter supplies.
    pub kind: ArgKind,
}

/// What is wrong with a declaration or a catalog string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// The catalog is not a JSON object of strings.
    NotACatalog,
    /// A declared argument name outside the argument key format.
    InvalidArgName(String),
    /// A code declares the same argument name twice.
    DuplicateArgName(String),
    /// A string of a code that has no declaration.
    UnknownCode,
    /// The string does not parse inside the `MessageFormat` subset.
    Unparseable(ParseError),
    /// A placeholder naming no declared argument.
    UndeclaredPlaceholder(String),
    /// A placeholder whose declared kind it does not accept.
    KindMismatch {
        /// The placeholder name.
        name: String,
        /// The kind the code declares.
        declared: ArgKind,
        /// The kind the placeholder needs.
        needs: Expected,
    },
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotACatalog => f.write_str("the catalog is not a JSON object of strings"),
            Self::InvalidArgName(name) => {
                write!(
                    f,
                    "declared argument name `{name}` is not `^[a-z][a-z0-9_]{{0,63}}$`"
                )
            }
            Self::DuplicateArgName(name) => write!(f, "argument `{name}` is declared twice"),
            Self::UnknownCode => f.write_str("the code is not a declared error code"),
            Self::Unparseable(error) => fmt::Display::fmt(error, f),
            Self::UndeclaredPlaceholder(name) => {
                write!(f, "placeholder `{name}` is not a declared argument")
            }
            Self::KindMismatch {
                name,
                declared,
                needs,
            } => write!(
                f,
                "placeholder `{name}` needs a {needs:?} argument but the code declares it {declared:?}"
            ),
        }
    }
}

/// One refusal: the catalog key (or the code's key prefix for a declaration)
/// and what is wrong with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// The catalog key, or `<emitter>.errors.<CODE>` for a declaration.
    pub key: String,
    /// What is wrong.
    pub problem: Problem,
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.key, self.problem)
    }
}

/// Check the strings of `catalog` (a JSON object, one locale of `emitter`)
/// against `codes`, every error code the emitter declares with the arguments
/// it declares (an empty list for a code with none).
///
/// Only the keys `<emitter>.errors.<CODE>.title` and `.description` are
/// checked, as when rendering; other keys are ignored. A code with no string
/// in the catalog is not an issue here.
///
/// # Errors
///
/// Every [`Issue`] found, declarations first, then strings in key order.
pub fn check_catalog(
    emitter: &str,
    catalog: &str,
    codes: &BTreeMap<String, Vec<ArgDeclaration>>,
) -> Result<(), Vec<Issue>> {
    let mut issues = declaration_issues(emitter, codes);
    let Ok(entries) = serde_json::from_str::<BTreeMap<String, Value>>(catalog) else {
        issues.push(Issue {
            key: emitter.to_owned(),
            problem: Problem::NotACatalog,
        });
        return Err(issues);
    };
    let prefix = format!("{emitter}.errors.");
    for (key, value) in &entries {
        let Some(code) = key.strip_prefix(&prefix).and_then(|rest| {
            rest.strip_suffix(".title")
                .or_else(|| rest.strip_suffix(".description"))
        }) else {
            continue;
        };
        let Value::String(text) = value else {
            issues.push(Issue {
                key: key.clone(),
                problem: Problem::NotACatalog,
            });
            continue;
        };
        issues.extend(
            check_string(codes.get(code).map(Vec::as_slice), text)
                .into_iter()
                .map(|problem| Issue {
                    key: key.clone(),
                    problem,
                }),
        );
    }
    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

/// Run [`check_catalog`] over every `<dir>/<locale>.json` of `emitter`, with
/// `dir` relative to `CARGO_MANIFEST_DIR` as for
/// [`build::emit_error_catalogs`](crate::build::emit_error_catalogs). Call it
/// from the same `build.rs`, with `codes` read from the emitter's descriptor:
/// `<locale>.sources.json` sidecars are not catalogs and are skipped.
///
/// # Panics
///
/// When a catalog cannot be read or any string fails the check: the panic
/// lists each locale, key and problem.
pub fn check_error_catalogs(
    dir: &str,
    emitter: &str,
    codes: &BTreeMap<String, Vec<ArgDeclaration>>,
) {
    let manifest = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo");
    let root = Path::new(&manifest).join(dir);
    let failures = check_dir(&root, emitter, codes);
    assert!(
        failures.is_empty(),
        "the error catalogs in {} do not match the declared error arguments:\n{}",
        root.display(),
        failures.join("\n")
    );
}

fn check_dir(
    root: &Path,
    emitter: &str,
    codes: &BTreeMap<String, Vec<ArgDeclaration>>,
) -> Vec<String> {
    let locales = match scan(root) {
        Ok(locales) => locales,
        Err(err) => return vec![format!("cannot read {}: {err}", root.display())],
    };
    let mut failures = Vec::new();
    for locale in locales {
        let path = root.join(format!("{locale}.json"));
        let json = match fs::read_to_string(&path) {
            Ok(json) => json,
            Err(err) => {
                failures.push(format!("cannot read {}: {err}", path.display()));
                continue;
            }
        };
        if let Err(issues) = check_catalog(emitter, &json, codes) {
            failures.extend(issues.iter().map(|issue| format!("{locale}: {issue}")));
        }
    }
    failures
}

fn declaration_issues(emitter: &str, codes: &BTreeMap<String, Vec<ArgDeclaration>>) -> Vec<Issue> {
    let mut issues = Vec::new();
    for (code, declared) in codes {
        let mut seen = BTreeSet::new();
        for declaration in declared {
            let problem = if !key_ok(&declaration.name) {
                Problem::InvalidArgName(declaration.name.clone())
            } else if !seen.insert(declaration.name.as_str()) {
                Problem::DuplicateArgName(declaration.name.clone())
            } else {
                continue;
            };
            issues.push(Issue {
                key: format!("{emitter}.errors.{code}"),
                problem,
            });
        }
    }
    issues
}

fn check_string(declared: Option<&[ArgDeclaration]>, text: &str) -> Vec<Problem> {
    let Some(declared) = declared else {
        return vec![Problem::UnknownCode];
    };
    let message = match Message::parse(text) {
        Ok(message) => message,
        Err(error) => return vec![Problem::Unparseable(error)],
    };
    message
        .placeholders()
        .into_iter()
        .filter_map(|(name, needs)| {
            match declared.iter().find(|declaration| declaration.name == name) {
                None => Some(Problem::UndeclaredPlaceholder(name)),
                Some(declaration) if declaration.kind.accepts(needs) => None,
                Some(declaration) => Some(Problem::KindMismatch {
                    name,
                    declared: declaration.kind,
                    needs,
                }),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decl(name: &str, kind: ArgKind) -> ArgDeclaration {
        ArgDeclaration {
            name: name.to_owned(),
            kind,
        }
    }

    fn codes() -> BTreeMap<String, Vec<ArgDeclaration>> {
        BTreeMap::from([
            (
                "SEATS".to_owned(),
                vec![
                    decl("count", ArgKind::Int64),
                    decl("ratio", ArgKind::Double),
                    decl("plan", ArgKind::String),
                    decl("ends_at", ArgKind::Timestamp),
                ],
            ),
            ("ARCHIVED".to_owned(), Vec::new()),
        ])
    }

    fn run(catalog: &str) -> Result<(), Vec<Issue>> {
        check_catalog("acme", catalog, &codes())
    }

    fn problems(catalog: &str) -> Vec<Problem> {
        run(catalog)
            .expect_err("the catalog is refused")
            .into_iter()
            .map(|issue| issue.problem)
            .collect()
    }

    #[test]
    fn a_catalog_whose_placeholders_are_declared_with_compatible_kinds_passes() {
        let catalog = r#"{
            "acme.errors.SEATS.title": "{count, plural, one {# seat} other {# seats}} on {plan, select, team {Team} other {Other}}, {ratio, number, percent}",
            "acme.errors.SEATS.description": "Ends {ends_at, date, long} at {ends_at, time, short}; {count, number} and {ratio, number}.",
            "acme.errors.ARCHIVED.title": "Archived.",
            "bones.errors.BAD_REQUEST.title": "{anything}",
            "acme.errors.SEATS.note": "{not_checked}"
        }"#;
        assert_eq!(run(catalog), Ok(()));
    }

    #[test]
    fn a_declared_argument_a_string_leaves_unused_passes() {
        assert_eq!(run(r#"{"acme.errors.SEATS.title": "No seats."}"#), Ok(()));
    }

    #[test]
    fn a_placeholder_naming_no_declared_argument_is_refused() {
        assert_eq!(
            problems(r#"{"acme.errors.SEATS.title": "{cout, number} seats"}"#),
            [Problem::UndeclaredPlaceholder("cout".to_owned())]
        );
        assert_eq!(
            problems(r#"{"acme.errors.ARCHIVED.title": "{who} archived it"}"#),
            [Problem::UndeclaredPlaceholder("who".to_owned())]
        );
    }

    #[test]
    fn a_placeholder_the_declared_kind_does_not_accept_is_refused() {
        let cases = [
            (
                "{count, date, long}",
                "count",
                ArgKind::Int64,
                Expected::Timestamp,
            ),
            ("{plan, number}", "plan", ArgKind::String, Expected::Number),
            ("{ends_at}", "ends_at", ArgKind::Timestamp, Expected::String),
            (
                "{ratio, select, a {x} other {y}}",
                "ratio",
                ArgKind::Double,
                Expected::String,
            ),
            (
                "{plan, plural, one {x} other {y}}",
                "plan",
                ArgKind::String,
                Expected::Number,
            ),
            ("{count}", "count", ArgKind::Int64, Expected::String),
        ];
        for (text, name, declared, needs) in cases {
            let catalog = format!(r#"{{"acme.errors.SEATS.description": "{text}"}}"#);
            assert_eq!(
                problems(&catalog),
                [Problem::KindMismatch {
                    name: name.to_owned(),
                    declared,
                    needs
                }],
                "{text}"
            );
        }
    }

    #[test]
    fn int64_and_double_both_accept_number_placeholders_only() {
        assert!(ArgKind::Int64.accepts(Expected::Number));
        assert!(ArgKind::Double.accepts(Expected::Number));
        assert!(!ArgKind::Int64.accepts(Expected::String));
        assert!(!ArgKind::Double.accepts(Expected::Timestamp));
    }

    #[test]
    fn a_string_outside_the_subset_is_refused_with_its_parse_error() {
        let found = problems(
            r#"{"acme.errors.SEATS.title": "{count, selectordinal, one {#st} other {#th}}"}"#,
        );
        assert!(
            matches!(found.as_slice(), [Problem::Unparseable(_)]),
            "{found:?}"
        );
    }

    #[test]
    fn a_string_of_an_undeclared_code_is_refused() {
        assert_eq!(
            problems(r#"{"acme.errors.SEAT.title": "x"}"#),
            [Problem::UnknownCode]
        );
    }

    #[test]
    fn a_catalog_that_is_not_an_object_of_strings_is_refused() {
        assert_eq!(problems("[1]"), [Problem::NotACatalog]);
        assert_eq!(
            problems(r#"{"acme.errors.SEATS.title": 3}"#),
            [Problem::NotACatalog]
        );
    }

    #[test]
    fn declarations_outside_the_argument_name_format_or_repeated_are_refused() {
        let mut declared = codes();
        declared.insert(
            "BAD".to_owned(),
            vec![
                decl("Seat", ArgKind::String),
                decl("seat_2", ArgKind::Int64),
                decl("seat_2", ArgKind::Int64),
                decl("", ArgKind::String),
            ],
        );
        let issues = check_catalog("acme", "{}", &declared).expect_err("refused");
        assert!(issues.iter().all(|issue| issue.key == "acme.errors.BAD"));
        assert_eq!(
            issues
                .into_iter()
                .map(|issue| issue.problem)
                .collect::<Vec<_>>(),
            [
                Problem::InvalidArgName("Seat".to_owned()),
                Problem::DuplicateArgName("seat_2".to_owned()),
                Problem::InvalidArgName(String::new()),
            ]
        );
    }

    fn catalog_dir(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
        let dir = env::temp_dir().join(format!(
            "api-bones-i18n-check-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for (file, body) in files {
            fs::write(dir.join(file), body).unwrap();
        }
        dir
    }

    #[test]
    fn check_dir_passes_catalogs_that_match_and_skips_sidecars() {
        let dir = catalog_dir(
            "ok",
            &[
                (
                    "en.json",
                    r#"{"acme.errors.SEATS.title": "{count, number} seats"}"#,
                ),
                (
                    "fr.json",
                    r#"{"acme.errors.SEATS.title": "{count, number} places"}"#,
                ),
                ("fr.sources.json", "not json"),
            ],
        );
        assert_eq!(check_dir(&dir, "acme", &codes()).len(), 0);
        check_error_catalogs(dir.to_str().unwrap(), "acme", &codes());
    }

    #[test]
    fn check_dir_names_the_locale_of_each_mismatch() {
        let dir = catalog_dir(
            "bad",
            &[
                (
                    "en.json",
                    r#"{"acme.errors.SEATS.title": "{count, number} seats"}"#,
                ),
                (
                    "fr.json",
                    r#"{"acme.errors.SEATS.title": "{nombre} places"}"#,
                ),
            ],
        );
        let failures = check_dir(&dir, "acme", &codes());
        assert_eq!(failures.len(), 1);
        assert!(
            failures[0].starts_with("fr: acme.errors.SEATS.title: "),
            "{failures:?}"
        );
    }

    #[test]
    fn check_dir_reports_an_unreadable_directory_and_file() {
        let missing = check_dir(Path::new("/nonexistent/api-bones-i18n"), "acme", &codes());
        assert_eq!(missing.len(), 1);
        let dir = catalog_dir("unreadable", &[]);
        fs::create_dir(dir.join("en.json")).unwrap();
        let failures = check_dir(&dir, "acme", &codes());
        assert_eq!(failures.len(), 1);
        assert!(failures[0].starts_with("cannot read "), "{failures:?}");
    }

    #[test]
    #[should_panic(expected = "do not match the declared error arguments")]
    fn check_error_catalogs_fails_the_build_on_a_mismatch() {
        let dir = catalog_dir(
            "panic",
            &[("en.json", r#"{"acme.errors.SEATS.title": "{nombre}"}"#)],
        );
        check_error_catalogs(dir.to_str().unwrap(), "acme", &codes());
    }

    #[test]
    fn issues_name_the_key_and_say_what_is_wrong() {
        let issue = run(r#"{"acme.errors.SEATS.title": "{plan, number}"}"#)
            .expect_err("refused")
            .remove(0);
        assert_eq!(
            issue.to_string(),
            "acme.errors.SEATS.title: placeholder `plan` needs a Number argument but the code declares it String"
        );
        let displayed = [
            Problem::NotACatalog,
            Problem::InvalidArgName("X".to_owned()),
            Problem::DuplicateArgName("x".to_owned()),
            Problem::UnknownCode,
            Problem::UndeclaredPlaceholder("x".to_owned()),
        ]
        .map(|problem| problem.to_string());
        assert!(displayed.iter().all(|text| !text.is_empty()));
        let parse = Message::parse("{x").expect_err("unbalanced");
        assert!(
            Problem::Unparseable(parse)
                .to_string()
                .contains("not supported")
        );
    }
}
