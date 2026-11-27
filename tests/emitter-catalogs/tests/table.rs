// SPDX-License-Identifier: MIT
//! The table `emitter_error_catalogs!` evaluates is the one
//! `embed_and_check_error_catalogs` wrote from the build script.

use api_bones_emitter_catalogs_fixture::ERROR_CATALOGS;

const EN: &str = include_str!("../catalogs/en.json");
const FR: &str = include_str!("../catalogs/fr.json");

#[test]
fn every_locale_is_listed_once_under_each_emitter_in_locale_order() {
    let rows: Vec<(&str, &str)> = ERROR_CATALOGS
        .iter()
        .map(|(emitter, locale, _)| (*emitter, *locale))
        .collect();
    assert_eq!(
        rows,
        [
            ("acme-origin", "en"),
            ("acme-edge", "en"),
            ("acme-origin", "fr"),
            ("acme-edge", "fr"),
        ]
    );
}

#[test]
fn each_row_embeds_the_catalog_file_of_its_locale() {
    for (_, locale, json) in ERROR_CATALOGS {
        let expected = match *locale {
            "en" => EN,
            "fr" => FR,
            other => panic!("unexpected locale {other}"),
        };
        assert_eq!(*json, expected, "{locale}");
    }
}

#[test]
fn the_provenance_sidecar_is_not_a_catalog() {
    assert!(
        ERROR_CATALOGS
            .iter()
            .all(|(_, locale, _)| *locale != "fr.sources")
    );
}
