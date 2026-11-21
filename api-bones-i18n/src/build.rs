// SPDX-License-Identifier: MIT
//! Build-script side of the error catalogs: embeds every
//! `<dir>/<locale>.json` so a locale added to the directory needs no code
//! change.
//!
//! An emitter calls [`emit_error_catalogs`] from its `build.rs`, then
//! `api_bones_i18n::error_catalogs!("<emitter>")` once in its library.

use std::fmt::Write as _;
use std::path::Path;
use std::{env, fs};

/// Scan `<CARGO_MANIFEST_DIR>/<dir>/*.json` and write the sorted table of
/// `(locale, include_str!(absolute path))` to `OUT_DIR/error_catalogs.rs`. Files whose stem
/// holds a `.` (the `<locale>.sources.json` sidecars) are not catalogs.
///
/// Prints `cargo:rerun-if-changed=<dir>` and the content-hash rerun key
/// `BREFWIZ_CODEGEN_INPUTS_SHA`.
///
/// # Panics
///
/// When the directory cannot be read, `OUT_DIR` cannot be written or
/// `en.json` is missing: a build script has no other way to fail the build.
pub fn emit_error_catalogs(dir: &str) {
    println!("cargo:rerun-if-changed={dir}");
    println!("cargo:rerun-if-env-changed=BREFWIZ_CODEGEN_INPUTS_SHA");
    let manifest = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo");
    let out = env::var("OUT_DIR").expect("OUT_DIR is set by cargo");
    let root = Path::new(&manifest).join(dir);
    let locales = scan(&root).unwrap_or_else(|err| {
        panic!(
            "cannot read the error catalogs in {}: {err}",
            root.display()
        )
    });
    assert!(
        locales.iter().any(|locale| locale == "en"),
        "the error catalogs in {} have no en.json",
        root.display()
    );
    fs::write(
        Path::new(&out).join("error_catalogs.rs"),
        table(&root, &locales),
    )
    .expect("OUT_DIR is writable");
}

pub(crate) fn scan(root: &Path) -> std::io::Result<Vec<String>> {
    let mut locales = Vec::new();
    for entry in fs::read_dir(root)? {
        let Ok(name) = entry?.file_name().into_string() else {
            continue;
        };
        let stem = name
            .strip_suffix(".json")
            .filter(|stem| !stem.is_empty() && !stem.contains('.'));
        if let Some(locale) = stem {
            locales.push(locale.to_owned());
        }
    }
    locales.sort();
    Ok(locales)
}

fn table(root: &Path, locales: &[String]) -> String {
    let mut table = String::from("&[\n");
    for locale in locales {
        let path = root.join(format!("{locale}.json"));
        writeln!(
            table,
            "    ({locale:?}, include_str!({:?})),",
            path.display().to_string()
        )
        .expect("writing to a String cannot fail");
    }
    table.push_str("]\n");
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str, files: &[&str]) -> std::path::PathBuf {
        let dir = env::temp_dir().join(format!("api-bones-i18n-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for file in files {
            fs::write(dir.join(file), "{}").unwrap();
        }
        dir
    }

    #[test]
    fn scan_lists_catalogs_sorted_and_skips_sidecars() {
        let dir = scratch(
            "scan",
            &["fr.json", "en.json", "fr.sources.json", "notes.txt"],
        );
        assert_eq!(scan(&dir).unwrap(), ["en", "fr"]);
    }

    #[test]
    fn scan_of_a_missing_directory_is_an_error() {
        assert!(scan(Path::new("/nonexistent/api-bones-i18n")).is_err());
    }

    #[test]
    fn table_embeds_each_locale_by_absolute_path() {
        let rendered = table(Path::new("/c"), &["en".to_owned(), "fr".to_owned()]);
        assert_eq!(
            rendered,
            "&[\n    (\"en\", include_str!(\"/c/en.json\")),\n    (\"fr\", include_str!(\"/c/fr.json\")),\n]\n"
        );
    }
}
