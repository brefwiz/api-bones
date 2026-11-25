// SPDX-License-Identifier: MIT
//! The service name that prefixes every wasm export: the `service` of the
//! repository's `SPEC.md`.

use std::path::{Path, PathBuf};

use proc_macro2::Span;

/// The value of the top level `service:` key of a SPEC text.
pub(crate) fn from_spec(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let value = line.strip_prefix("service:")?.trim();
        let value = value.trim_matches(['"', '\'']);
        (!value.is_empty()).then(|| value.to_owned())
    })
}

/// The `service` of the nearest `SPEC.md` at or above `dir`.
pub(crate) fn find(dir: &Path) -> Option<String> {
    dir.ancestors().find_map(|ancestor| {
        let text = std::fs::read_to_string(ancestor.join("SPEC.md")).ok()?;
        from_spec(&text)
    })
}

/// The SPEC service when there is one, else the package name.
pub(crate) fn choose(spec: Option<String>, package: Option<String>) -> syn::Result<String> {
    spec.or(package).ok_or_else(|| {
        syn::Error::new(
            Span::call_site(),
            "cannot name the wasm exports: no SPEC.md `service:` above the crate and no package name",
        )
    })
}

/// The service name for the crate being compiled.
pub(crate) fn resolve() -> syn::Result<String> {
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from);
    choose(
        manifest.as_deref().and_then(find),
        std::env::var("CARGO_PKG_NAME").ok(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_service_is_the_top_level_key_of_the_spec() {
        let spec = "---\nservice: api-bones\nwire_surface: library\n";
        assert_eq!(from_spec(spec).as_deref(), Some("api-bones"));
        assert_eq!(
            from_spec("service: \"quoted-name\"\n").as_deref(),
            Some("quoted-name")
        );
    }

    #[test]
    fn an_indented_or_empty_service_is_not_the_key() {
        assert_eq!(from_spec("a:\n  service: nested\n"), None);
        assert_eq!(from_spec("service:   \n"), None);
        assert_eq!(from_spec(""), None);
    }

    #[test]
    fn the_nearest_spec_above_a_directory_names_the_service() {
        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert_eq!(find(here).as_deref(), Some("api-bones"));
        assert_eq!(find(&here.join("src")).as_deref(), Some("api-bones"));
    }

    #[test]
    fn a_directory_with_no_spec_above_it_names_nothing() {
        let lonely =
            std::env::temp_dir().join(format!("sdk-macros-no-spec-{}", std::process::id()));
        std::fs::create_dir_all(&lonely).expect("a directory");
        let found = find(&lonely);
        std::fs::remove_dir_all(&lonely).expect("removed");
        assert_eq!(found, None);
    }

    #[test]
    fn the_spec_wins_over_the_package_and_the_package_is_the_fallback() {
        let both = choose(Some("spec".to_owned()), Some("pkg".to_owned()));
        assert_eq!(both.expect("named"), "spec");
        let package = choose(None, Some("pkg".to_owned()));
        assert_eq!(package.expect("named"), "pkg");
        let neither = choose(None, None).expect_err("no name");
        assert!(neither.to_string().contains("cannot name"), "{neither}");
    }

    #[test]
    fn resolve_reads_the_spec_of_this_repository() {
        assert_eq!(resolve().expect("named"), "api-bones");
    }
}
