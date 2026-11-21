// SPDX-License-Identifier: MIT
//! The platform locale registry and Accept-Language negotiation. The registry
//! is the committed copy of core-ui's `platformLocales.ts`; the unit test
//! below asserts both stay equal.

/// The locale every negotiation falls back to.
pub const DEFAULT_LOCALE: &str = "en";

/// The 29 platform locales, in registry order.
pub const PLATFORM_LOCALES: [&str; 29] = [
    "en", "fr", "de", "es", "it", "pt-BR", "nl", "sv", "da", "fi", "nb", "pl", "cs", "sk", "sl",
    "hr", "hu", "ro", "bg", "el", "et", "lv", "lt", "tr", "ja", "ko", "zh-Hans", "he", "ar",
];

/// Lower-case alias to platform locale.
pub const ALIASES: [(&str, &str); 7] = [
    ("no", "nb"),
    ("nn", "nb"),
    ("iw", "he"),
    ("pt", "pt-BR"),
    ("zh", "zh-Hans"),
    ("zh-cn", "zh-Hans"),
    ("zh-sg", "zh-Hans"),
];

fn exact(lower: &str) -> Option<&'static str> {
    PLATFORM_LOCALES
        .iter()
        .copied()
        .find(|code| code.eq_ignore_ascii_case(lower))
        .or_else(|| {
            ALIASES
                .iter()
                .find(|(alias, _)| *alias == lower)
                .map(|&(_, code)| code)
        })
}

/// The platform locale one tag selects: trim, `_` to `-`, case-insensitive;
/// the exact code, else an alias, else the language subtag (through the
/// aliases). A `zh-*` tag selects `zh-Hans` by its language only when a subtag
/// is `hans`. `*` and the empty tag select nothing.
#[must_use]
pub fn match_locale(tag: &str) -> Option<&'static str> {
    let lower = tag.trim().replace('_', "-").to_ascii_lowercase();
    if lower.is_empty() || lower == "*" {
        return None;
    }
    if let Some(code) = exact(&lower) {
        return Some(code);
    }
    let mut subtags = lower.split('-');
    let language = subtags.next()?;
    if language == "zh" {
        return subtags.any(|subtag| subtag == "hans").then_some("zh-Hans");
    }
    exact(language)
}

fn quality(params: &str) -> Option<f64> {
    let mut q = 1.0;
    for param in params.split(';') {
        let param = param.trim();
        if let Some(value) = param
            .strip_prefix("q=")
            .or_else(|| param.strip_prefix("Q="))
        {
            q = value.trim().parse::<f64>().ok()?;
        }
    }
    (q.is_finite() && q > 0.0 && q <= 1.0).then_some(q)
}

/// The platform locale for a BCP 47 tag or an Accept-Language value: entries
/// by descending `q` (ties in header order, `q=0` refused), the first that
/// matches, else [`DEFAULT_LOCALE`].
#[must_use]
pub fn negotiate_locale(requested: &str) -> &'static str {
    let mut entries: Vec<(f64, &str)> = requested
        .split(',')
        .filter_map(|entry| {
            let (tag, params) = entry.split_once(';').unwrap_or((entry, ""));
            Some((quality(params)?, tag))
        })
        .collect();
    entries.sort_by(|a, b| b.0.total_cmp(&a.0));
    entries
        .into_iter()
        .find_map(|(_, tag)| match_locale(tag))
        .unwrap_or(DEFAULT_LOCALE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_equals_the_committed_core_ui_copy() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/features/fixtures/error_localization/platform_locales.json"
        ))
        .unwrap();
        assert_eq!(fixture["default"], DEFAULT_LOCALE);
        let locales: Vec<&str> = fixture["locales"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(locales, PLATFORM_LOCALES);
        let aliases = fixture["aliases"].as_object().unwrap();
        assert_eq!(aliases.len(), ALIASES.len());
        for (alias, code) in ALIASES {
            assert_eq!(aliases[alias], code, "{alias}");
        }
    }

    #[test]
    fn matches_exact_alias_and_language() {
        let cases = [
            ("fr", Some("fr")),
            (" FR ", Some("fr")),
            ("pt_br", Some("pt-BR")),
            ("pt-PT", Some("pt-BR")),
            ("pt", Some("pt-BR")),
            ("fr-CA", Some("fr")),
            ("en-GB", Some("en")),
            ("no", Some("nb")),
            ("no-NO", Some("nb")),
            ("nn-NO", Some("nb")),
            ("iw", Some("he")),
            ("zh", Some("zh-Hans")),
            ("zh-CN", Some("zh-Hans")),
            ("zh-sg", Some("zh-Hans")),
            ("zh-Hans-CN", Some("zh-Hans")),
            ("zh-hans", Some("zh-Hans")),
            ("zh-Hant", None),
            ("zh-TW", None),
            ("*", None),
            ("", None),
            ("  ", None),
            ("xx", None),
        ];
        for (tag, want) in cases {
            assert_eq!(match_locale(tag), want, "{tag:?}");
        }
    }

    #[test]
    fn negotiates_a_tag_and_a_header() {
        assert_eq!(negotiate_locale("fr"), "fr");
        assert_eq!(negotiate_locale("xx"), "en");
        assert_eq!(negotiate_locale(""), "en");
        assert_eq!(negotiate_locale("de;q=0.5, fr;q=0.9"), "fr");
        assert_eq!(negotiate_locale("de, fr"), "de", "ties keep header order");
        assert_eq!(
            negotiate_locale("xx, fr;q=0.3"),
            "fr",
            "an unmatched entry is skipped"
        );
        assert_eq!(negotiate_locale("fr;q=0, de;q=0.1"), "de", "q=0 is refused");
        assert_eq!(negotiate_locale("fr;q=0"), "en");
        assert_eq!(negotiate_locale("*, es;q=0.4"), "es");
        assert_eq!(
            negotiate_locale("fr;q=abc, it"),
            "it",
            "a malformed q refuses the entry"
        );
        assert_eq!(negotiate_locale("zh-TW, ja;q=0.2"), "ja");
    }
}
