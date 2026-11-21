// SPDX-License-Identifier: MIT
//! The catalogs a key resolves from: the platform's own `bones` catalogs,
//! embedded at compile time, and the emitters' catalogs, registered at
//! compile time through [`error_catalogs!`](crate::error_catalogs) or at run
//! time through [`register_catalog`].
//!
//! `bones.*` keys resolve only from the embedded catalogs: a registration for
//! the emitter `bones` is ignored. A registered catalog contributes only keys
//! prefixed `<emitter>.errors.`, and only for a platform locale. Registering
//! the same entries again is a no-op; a second registration with different
//! entries makes that (emitter, locale) unusable, whichever came first.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock, PoisonError};

use crate::locale::PLATFORM_LOCALES;

/// The emitter of the platform's own keys.
pub(crate) const BONES: &str = "bones";

type Entries = BTreeMap<String, String>;

/// The `(locale, json)` pairs of `catalogs/errors`, sorted by locale.
const EMBEDDED: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/error_catalogs.rs"));

/// One emitter's compile-time catalogs, as [`error_catalogs!`](crate::error_catalogs)
/// submits them.
#[doc(hidden)]
pub struct Registration {
    /// The emitter the keys are prefixed with.
    pub emitter: &'static str,
    /// The `(locale, json)` pairs, sorted by locale.
    pub catalogs: &'static [(&'static str, &'static str)],
}

inventory::collect!(Registration);

enum Slot {
    Usable(Entries),
    Conflict,
}

type Registry = BTreeMap<(String, String), Slot>;

/// The string entries of a JSON object. Anything else is not a catalog.
fn parse(json: &str) -> Option<Entries> {
    match serde_json::from_str::<serde_json::Value>(json).ok()? {
        serde_json::Value::Object(map) => Some(
            map.into_iter()
                .filter_map(|(key, value)| Some((key, value.as_str()?.to_owned())))
                .collect(),
        ),
        _ => None,
    }
}

fn register_into(registry: &mut Registry, emitter: &str, locale: &str, json: &str) {
    if emitter.is_empty() || emitter == BONES || !PLATFORM_LOCALES.contains(&locale) {
        return;
    }
    let Some(parsed) = parse(json) else {
        return;
    };
    let prefix = format!("{emitter}.errors.");
    let entries: Entries = parsed
        .into_iter()
        .filter(|(key, _)| key.starts_with(&prefix))
        .collect();
    let slot = registry.entry((emitter.to_owned(), locale.to_owned()));
    match slot {
        std::collections::btree_map::Entry::Vacant(vacant) => {
            vacant.insert(Slot::Usable(entries));
        }
        std::collections::btree_map::Entry::Occupied(mut occupied) => {
            let differs = match occupied.get() {
                Slot::Usable(existing) => *existing != entries,
                Slot::Conflict => false,
            };
            if differs {
                occupied.insert(Slot::Conflict);
            }
        }
    }
}

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut registry = Registry::new();
        for registration in inventory::iter::<Registration> {
            for (locale, json) in registration.catalogs {
                register_into(&mut registry, registration.emitter, locale, json);
            }
        }
        Mutex::new(registry)
    })
}

/// Register one emitter's catalog for one platform locale at run time,
/// through the store [`error_catalogs!`](crate::error_catalogs) feeds.
///
/// `json` is an object of `"<emitter>.errors.<CODE>.title"` /
/// `".description"` strings; other keys, other locales and the emitter
/// `bones` are ignored. The same entries again are a no-op; different entries
/// for the same (emitter, locale) make it unusable for both registrations.
pub fn register_catalog(emitter: &str, locale: &str, json: &str) {
    let mut registry = registry().lock().unwrap_or_else(PoisonError::into_inner);
    register_into(&mut registry, emitter, locale, json);
}

fn embedded() -> &'static BTreeMap<&'static str, Entries> {
    static BONES_CATALOGS: OnceLock<BTreeMap<&'static str, Entries>> = OnceLock::new();
    BONES_CATALOGS.get_or_init(|| {
        EMBEDDED
            .iter()
            .filter_map(|(locale, json)| Some((*locale, parse(json)?)))
            .collect()
    })
}

/// The string at `key` in `emitter`'s catalog for `locale`, when the catalog
/// is usable.
pub(crate) fn entry(emitter: &str, locale: &str, key: &str) -> Option<String> {
    if emitter == BONES {
        return embedded().get(locale)?.get(key).cloned();
    }
    let registry = registry().lock().unwrap_or_else(PoisonError::into_inner);
    match registry.get(&(emitter.to_owned(), locale.to_owned()))? {
        Slot::Usable(entries) => entries.get(key).cloned(),
        Slot::Conflict => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    crate::error_catalogs!("bones");

    fn cat(emitter: &str, title: &str) -> String {
        format!(r#"{{"{emitter}.errors.GRANT_MISSING.title": "{title}"}}"#)
    }

    #[test]
    fn every_embedded_catalog_parses_and_keeps_to_platform_keys() {
        assert!(!embedded().is_empty());
        assert_eq!(embedded().len(), EMBEDDED.len());
        for (locale, entries) in embedded() {
            assert!(PLATFORM_LOCALES.contains(locale), "{locale}");
            assert!(
                entries
                    .keys()
                    .all(|key| key.starts_with("bones.errors.")
                        || key.starts_with("bones.validation.")),
                "{locale}"
            );
        }
    }

    #[test]
    fn the_english_catalog_is_embedded() {
        assert_eq!(
            entry(BONES, "en", "bones.errors.BAD_REQUEST.title").as_deref(),
            Some("Invalid request")
        );
    }

    #[test]
    fn a_registered_catalog_resolves_its_own_keys() {
        register_catalog("t-own", "en", &cat("t-own", "t-own"));
        assert_eq!(
            entry("t-own", "en", "t-own.errors.GRANT_MISSING.title").as_deref(),
            Some("t-own")
        );
        assert_eq!(
            entry("t-own", "fr", "t-own.errors.GRANT_MISSING.title"),
            None
        );
    }

    #[test]
    fn a_registration_contributes_only_keys_prefixed_by_its_emitter() {
        register_catalog(
            "t-prefix",
            "en",
            r#"{"t-prefix.errors.A.title": "a", "other.errors.A.title": "x", "bones.errors.BAD_REQUEST.title": "x"}"#,
        );
        assert_eq!(
            entry("t-prefix", "en", "t-prefix.errors.A.title").as_deref(),
            Some("a")
        );
        assert_eq!(entry("t-prefix", "en", "other.errors.A.title"), None);
        assert_eq!(
            entry("t-prefix", "en", "bones.errors.BAD_REQUEST.title"),
            None
        );
    }

    #[test]
    fn registering_the_emitter_bones_changes_nothing() {
        register_catalog(
            BONES,
            "en",
            r#"{"bones.errors.BAD_REQUEST.title": "hijacked"}"#,
        );
        assert_eq!(
            entry(BONES, "en", "bones.errors.BAD_REQUEST.title").as_deref(),
            Some("Invalid request")
        );
        assert!(
            !registry()
                .lock()
                .unwrap()
                .contains_key(&(BONES.to_owned(), "en".to_owned()))
        );
    }

    #[test]
    fn a_locale_outside_the_platform_set_is_ignored() {
        register_catalog("t-locale", "xx", &cat("t-locale", "x"));
        assert_eq!(
            entry("t-locale", "xx", "t-locale.errors.GRANT_MISSING.title"),
            None
        );
    }

    #[test]
    fn a_catalog_that_is_not_an_object_of_strings_is_ignored() {
        register_catalog("t-bad", "en", "[1, 2]");
        register_catalog("t-bad", "en", "not json");
        assert!(
            !registry()
                .lock()
                .unwrap()
                .contains_key(&("t-bad".to_owned(), "en".to_owned()))
        );
    }

    #[test]
    fn the_same_entries_again_are_a_no_op() {
        let json = cat("t-same", "same");
        register_catalog("t-same", "en", &json);
        register_catalog("t-same", "en", &json);
        assert_eq!(
            entry("t-same", "en", "t-same.errors.GRANT_MISSING.title").as_deref(),
            Some("same")
        );
    }

    #[test]
    fn different_entries_make_the_locale_unusable_in_either_order() {
        for (emitter, first, second) in [("t-ab", "a", "b"), ("t-ba", "b", "a")] {
            register_catalog(emitter, "en", &cat(emitter, first));
            register_catalog(emitter, "en", &cat(emitter, second));
            let key = format!("{emitter}.errors.GRANT_MISSING.title");
            assert_eq!(entry(emitter, "en", &key), None, "{emitter}");
            register_catalog(emitter, "en", &cat(emitter, first));
            assert_eq!(entry(emitter, "en", &key), None, "{emitter} stays unusable");
        }
    }

    #[test]
    fn a_conflict_in_one_locale_leaves_the_others_usable() {
        register_catalog("t-iso", "en", &cat("t-iso", "a"));
        register_catalog("t-iso", "en", &cat("t-iso", "b"));
        register_catalog("t-iso", "fr", &cat("t-iso", "fr"));
        assert_eq!(
            entry("t-iso", "en", "t-iso.errors.GRANT_MISSING.title"),
            None
        );
        assert_eq!(
            entry("t-iso", "fr", "t-iso.errors.GRANT_MISSING.title").as_deref(),
            Some("fr")
        );
    }

    #[test]
    fn a_compile_time_registration_for_bones_is_ignored() {
        let submitted = inventory::iter::<Registration>
            .into_iter()
            .any(|registration| registration.emitter == BONES);
        assert!(submitted, "the macro submitted its registration");
        assert!(
            !registry()
                .lock()
                .unwrap()
                .keys()
                .any(|(emitter, _)| emitter == BONES)
        );
    }
}
