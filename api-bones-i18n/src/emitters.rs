// SPDX-License-Identifier: MIT
//! Build-script helper for a crate that holds the error catalogs of several
//! emitters in one directory: embeds them and checks every string against the
//! codes and arguments each emitter's error enum declares.
//!
//! A build script calls [`embed_and_check_error_catalogs`] once, after its
//! proto codegen wrote the descriptor set; the crate then includes the table
//! with [`emitter_error_catalogs!`](crate::emitter_error_catalogs).

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::Write as _;
use std::path::Path;
use std::{env, fs};

use api_bones_protos::error_args::{ErrorArgKind, declared_error_args};

use crate::build::scan;
use crate::{ArgDeclaration, ArgKind, check_error_catalogs};

/// The file under `OUT_DIR` that holds the table, the one
/// [`emitter_error_catalogs!`](crate::emitter_error_catalogs) includes.
const TABLE_FILE: &str = crate::__emitter_table_file!();

/// One emitter whose catalog strings are checked: the `service_name` its
/// binary bootstraps with, the proto enum whose values are its error codes and
/// the prefix those values carry. A code is its value name without the prefix,
/// the form that travels on the wire and keys the catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Emitter<'a> {
    /// The emitter name, the first segment of its catalog keys.
    pub name: &'a str,
    /// The fully qualified error enum, e.g. `acme.types.v1.PaymentError`.
    pub error_enum: &'a str,
    /// The prefix every value of the enum carries, e.g. `PAYMENT_ERROR_`.
    pub prefix: &'a str,
}

impl<'a> Emitter<'a> {
    /// An emitter of `name` whose codes are the values of `error_enum` less
    /// `prefix`.
    #[must_use]
    pub const fn new(name: &'a str, error_enum: &'a str, prefix: &'a str) -> Self {
        Self {
            name,
            error_enum,
            prefix,
        }
    }
}

/// Embed the catalogs in `<CARGO_MANIFEST_DIR>/<dir>` for every emitter and
/// check each against the codes its error enum declares in `descriptor_set`,
/// so a string cannot name a code that does not exist or an argument no code
/// declares.
///
/// Writes `OUT_DIR/emitter_error_catalogs.rs`, the `(emitter, locale, json)`
/// table [`emitter_error_catalogs!`](crate::emitter_error_catalogs) includes.
/// Each `<locale>.json` is embedded once and listed under every emitter, since
/// one file holds all the emitters' keys; `<locale>.sources.json` sidecars are
/// not catalogs. Prints `cargo:rerun-if-changed=<dir>` and the content-hash
/// rerun key `BREFWIZ_CODEGEN_INPUTS_SHA`.
///
/// # Errors
///
/// When `OUT_DIR` or `CARGO_MANIFEST_DIR` is unset, the directory cannot be
/// read or has no `en.json`, the descriptor set does not decode, an emitter's
/// enum is not in it, one of its values lacks the prefix or declares an
/// argument with no kind.
///
/// # Panics
///
/// When a string fails the check, as [`check_error_catalogs`] does: the panic
/// lists each locale, key and problem.
pub fn embed_and_check_error_catalogs(
    dir: &str,
    descriptor_set: &[u8],
    emitters: &[Emitter<'_>],
) -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed={dir}");
    println!("cargo:rerun-if-env-changed=BREFWIZ_CODEGEN_INPUTS_SHA");
    let out_dir = env::var("OUT_DIR")?;
    embed_and_check_in(dir, Path::new(&out_dir), descriptor_set, emitters)
}

/// [`embed_and_check_error_catalogs`] with the output directory given.
fn embed_and_check_in(
    dir: &str,
    out_dir: &Path,
    descriptor_set: &[u8],
    emitters: &[Emitter<'_>],
) -> Result<(), Box<dyn Error>> {
    let root = Path::new(&env::var("CARGO_MANIFEST_DIR")?).join(dir);
    emit_table(&root, out_dir, emitters)?;
    for emitter in emitters {
        check_error_catalogs(dir, emitter.name, &error_codes(descriptor_set, emitter)?);
    }
    Ok(())
}

/// The codes of `emitter` with the arguments each declares: every value of its
/// error enum but the zero one, which names no code.
fn error_codes(
    descriptor_set: &[u8],
    emitter: &Emitter<'_>,
) -> Result<BTreeMap<String, Vec<ArgDeclaration>>, Box<dyn Error>> {
    let pool = prost_reflect::DescriptorPool::decode(descriptor_set)?;
    let descriptor = pool
        .get_enum_by_name(emitter.error_enum)
        .ok_or_else(|| format!("{} is not in the descriptor set", emitter.error_enum))?;
    let mut codes = BTreeMap::new();
    for value in descriptor.values().filter(|value| value.number() != 0) {
        let declared = declared_error_args(&value)?
            .into_iter()
            .map(|declared| ArgDeclaration {
                name: declared.name,
                kind: match declared.kind {
                    ErrorArgKind::String => ArgKind::String,
                    ErrorArgKind::Int64 => ArgKind::Int64,
                    ErrorArgKind::Double => ArgKind::Double,
                    ErrorArgKind::Timestamp => ArgKind::Timestamp,
                },
            })
            .collect();
        let code = value
            .name()
            .strip_prefix(emitter.prefix)
            .ok_or_else(|| format!("{} does not start with {}", value.name(), emitter.prefix))?;
        codes.insert(code.to_owned(), declared);
    }
    Ok(codes)
}

fn emit_table(root: &Path, out_dir: &Path, emitters: &[Emitter<'_>]) -> Result<(), Box<dyn Error>> {
    let locales = scan(root)?;
    if !locales.iter().any(|locale| locale == "en") {
        return Err(format!("{} has no en.json", root.display()).into());
    }
    fs::write(out_dir.join(TABLE_FILE), table(root, &locales, emitters)?)?;
    Ok(())
}

fn table(
    root: &Path,
    locales: &[String],
    emitters: &[Emitter<'_>],
) -> Result<String, std::fmt::Error> {
    let mut table = String::from("{\n");
    for (index, locale) in locales.iter().enumerate() {
        let path = root.join(format!("{locale}.json"));
        writeln!(
            table,
            "const CATALOG_{index}: &str = include_str!({:?});",
            path.display().to_string()
        )?;
    }
    table.push_str("&[\n");
    for (index, locale) in locales.iter().enumerate() {
        for emitter in emitters {
            writeln!(
                table,
                "    ({:?}, {locale:?}, CATALOG_{index}),",
                emitter.name
            )?;
        }
    }
    table.push_str("]\n}\n");
    Ok(table)
}

#[cfg(test)]
#[path = "../../tests/shared/shipped_resolver.rs"]
mod shipped_resolver;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emitters::shipped_resolver::{CONTRACT, compiler};

    const PROTO: &str = "syntax = \"proto3\";\npackage contract.v1;\n\
        import \"bones/v1/annotations.proto\";\n\
        enum OriginError {\n\
          ORIGIN_ERROR_UNSPECIFIED = 0;\n\
          ORIGIN_ERROR_NO_ARGS = 1;\n\
          ORIGIN_ERROR_LIMIT = 2 [\n\
            (bones.v1.error_args) = { name: \"limit\" kind: ERROR_ARG_KIND_INT64 },\n\
            (bones.v1.error_args) = { name: \"plan\" kind: ERROR_ARG_KIND_STRING },\n\
            (bones.v1.error_args) = { name: \"ratio\" kind: ERROR_ARG_KIND_DOUBLE },\n\
            (bones.v1.error_args) = { name: \"ends_at\" kind: ERROR_ARG_KIND_TIMESTAMP }\n\
          ];\n\
        }\n\
        enum EdgeError {\n\
          EDGE_ERROR_UNSPECIFIED = 0;\n\
          EDGE_ERROR_DENIED = 1 [(bones.v1.error_args) = { name: \"seat\" kind: ERROR_ARG_KIND_STRING }];\n\
        }\n\
        enum StrayError {\n\
          STRAY_UNSPECIFIED = 0;\n\
          OTHER_NAME = 1;\n\
        }\n\
        enum NoKindError {\n\
          NO_KIND_ERROR_UNSPECIFIED = 0;\n\
          NO_KIND_ERROR_X = 1 [(bones.v1.error_args) = { name: \"seat\" }];\n\
        }\n";

    fn descriptor_set() -> Vec<u8> {
        let mut compiler = compiler(PROTO.to_owned(), api_bones_protos::files());
        compiler.include_imports(true);
        compiler
            .open_file(CONTRACT)
            .unwrap_or_else(|e| panic!("contract failed to compile: {e}"));
        compiler.encode_file_descriptor_set()
    }

    const ORIGIN: Emitter<'static> =
        Emitter::new("acme-origin", "contract.v1.OriginError", "ORIGIN_ERROR_");
    const EDGE: Emitter<'static> =
        Emitter::new("acme-edge", "contract.v1.EdgeError", "EDGE_ERROR_");

    fn scratch(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
        crate::test_support::scratch_dir("emitters", name, files)
    }

    #[test]
    fn table_lists_each_locale_once_and_under_every_emitter() {
        let rendered = table(
            Path::new("/c"),
            &["en".to_owned(), "fr".to_owned()],
            &[ORIGIN, EDGE],
        )
        .unwrap();
        assert_eq!(
            rendered,
            "{\n\
             const CATALOG_0: &str = include_str!(\"/c/en.json\");\n\
             const CATALOG_1: &str = include_str!(\"/c/fr.json\");\n\
             &[\n    \
             (\"acme-origin\", \"en\", CATALOG_0),\n    \
             (\"acme-edge\", \"en\", CATALOG_0),\n    \
             (\"acme-origin\", \"fr\", CATALOG_1),\n    \
             (\"acme-edge\", \"fr\", CATALOG_1),\n\
             ]\n}\n"
        );
    }

    #[test]
    fn emit_table_writes_the_table_beside_the_catalogs_skipping_sidecars() {
        let root = scratch(
            "emit",
            &[
                ("en.json", "{}"),
                ("fr.json", "{}"),
                ("fr.sources.json", "{}"),
            ],
        );
        let out = scratch("emit-out", &[]);
        emit_table(&root, &out, &[ORIGIN]).unwrap();
        let written = fs::read_to_string(out.join(TABLE_FILE)).unwrap();
        assert!(written.contains("CATALOG_1"), "{written}");
        assert!(!written.contains("CATALOG_2"), "{written}");
        assert!(!written.contains("sources"), "{written}");
    }

    #[test]
    fn emit_table_without_english_is_an_error_naming_the_directory() {
        let root = scratch("no-en", &[("fr.json", "{}")]);
        let out = scratch("no-en-out", &[]);
        let error = emit_table(&root, &out, &[ORIGIN]).unwrap_err().to_string();
        assert!(error.contains("has no en.json"), "{error}");
        assert!(error.contains(&root.display().to_string()), "{error}");
        assert!(!out.join(TABLE_FILE).exists());
    }

    #[test]
    fn emit_table_of_a_missing_directory_is_an_error() {
        let out = scratch("missing-out", &[]);
        assert!(emit_table(Path::new("/nonexistent/api-bones-i18n"), &out, &[ORIGIN]).is_err());
    }

    #[test]
    fn error_codes_skip_the_zero_value_strip_the_prefix_and_carry_declared_kinds() {
        let codes = error_codes(&descriptor_set(), &ORIGIN).unwrap();
        assert_eq!(codes.keys().collect::<Vec<_>>(), ["LIMIT", "NO_ARGS"]);
        assert_eq!(codes["NO_ARGS"], []);
        let limit = &codes["LIMIT"];
        assert_eq!(limit.len(), 4);
        for (declared, (name, kind)) in limit.iter().zip([
            ("limit", ArgKind::Int64),
            ("plan", ArgKind::String),
            ("ratio", ArgKind::Double),
            ("ends_at", ArgKind::Timestamp),
        ]) {
            assert_eq!(declared.name, name);
            assert_eq!(declared.kind, kind);
        }
        let edge = error_codes(&descriptor_set(), &EDGE).unwrap();
        assert_eq!(edge["DENIED"].len(), 1);
        assert_eq!(edge["DENIED"][0].name, "seat");
        assert_eq!(edge["DENIED"][0].kind, ArgKind::String);
    }

    #[test]
    fn error_codes_of_an_enum_missing_from_the_descriptor_set_name_it() {
        let missing = Emitter::new("acme", "contract.v1.GoneError", "GONE_ERROR_");
        let error = error_codes(&descriptor_set(), &missing)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("contract.v1.GoneError is not in the descriptor set"),
            "{error}"
        );
    }

    #[test]
    fn error_codes_refuse_a_value_without_the_prefix() {
        let stray = Emitter::new("acme", "contract.v1.StrayError", "STRAY_ERROR_");
        let error = error_codes(&descriptor_set(), &stray)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("OTHER_NAME does not start with STRAY_ERROR_"),
            "{error}"
        );
    }

    #[test]
    fn error_codes_refuse_a_declaration_with_no_kind() {
        let no_kind = Emitter::new("acme", "contract.v1.NoKindError", "NO_KIND_ERROR_");
        let error = error_codes(&descriptor_set(), &no_kind)
            .unwrap_err()
            .to_string();
        assert!(error.contains("seat"), "{error}");
    }

    #[test]
    fn error_codes_refuse_bytes_that_are_not_a_descriptor_set() {
        assert!(error_codes(b"\xff\xff not a descriptor", &ORIGIN).is_err());
    }

    const CATALOG: &str = "{\"acme-origin.errors.LIMIT.title\": \"Limit {limit, number} on {plan}\", \
        \"acme-edge.errors.DENIED.title\": \"No seat {seat}\"}";

    #[test]
    fn embed_and_check_embeds_and_accepts_catalogs_matching_the_declarations() {
        let root = scratch(
            "ok",
            &[
                ("en.json", CATALOG),
                ("fr.json", CATALOG),
                ("fr.sources.json", "{}"),
            ],
        );
        let out = scratch("ok-out", &[]);
        embed_and_check_in(
            root.to_str().unwrap(),
            &out,
            &descriptor_set(),
            &[ORIGIN, EDGE],
        )
        .unwrap();
        let written = fs::read_to_string(out.join(TABLE_FILE)).unwrap();
        assert!(
            written.contains("(\"acme-edge\", \"fr\", CATALOG_1),"),
            "{written}"
        );
    }

    #[test]
    #[should_panic(expected = "do not match the declared error arguments")]
    fn embed_and_check_fails_the_build_on_an_undeclared_placeholder() {
        let bad = "{\"acme-edge.errors.DENIED.title\": \"No seat {row}\"}";
        let root = scratch("bad", &[("en.json", bad)]);
        let out = scratch("bad-out", &[]);
        let _ = embed_and_check_in(root.to_str().unwrap(), &out, &descriptor_set(), &[EDGE]);
    }

    #[test]
    #[should_panic(expected = "do not match the declared error arguments")]
    fn embed_and_check_fails_the_build_on_a_string_for_an_unknown_code() {
        let bad = "{\"acme-edge.errors.GONE.title\": \"Gone\"}";
        let root = scratch("unknown", &[("en.json", bad)]);
        let out = scratch("unknown-out", &[]);
        let _ = embed_and_check_in(root.to_str().unwrap(), &out, &descriptor_set(), &[EDGE]);
    }

    #[test]
    fn embed_and_check_without_english_is_an_error_before_any_check() {
        let root = scratch("no-english", &[("fr.json", CATALOG)]);
        let out = scratch("no-english-out", &[]);
        let error = embed_and_check_in(root.to_str().unwrap(), &out, &descriptor_set(), &[ORIGIN])
            .unwrap_err()
            .to_string();
        assert!(error.contains("has no en.json"), "{error}");
    }
}
