//! Validated `Slug` newtype for URL-friendly identifiers.
//!
//! A `Slug` is a lowercase ASCII string used as a human-readable URL segment.
//! It enforces the following constraints at construction time:
//!
//! - Only lowercase ASCII letters (`a-z`), digits (`0-9`), and hyphens (`-`)
//! - Length: 1–128 characters
//! - No leading, trailing, or consecutive hyphens
//!
//! # Example
//!
//! ```rust
//! use shared_types::slug::{Slug, SlugError};
//!
//! let slug = Slug::new("hello-world").unwrap();
//! assert_eq!(slug.as_str(), "hello-world");
//!
//! let auto = Slug::from_title("Hello, World! 2024");
//! assert_eq!(auto.as_str(), "hello-world-2024");
//!
//! assert!(matches!(Slug::new("Hello"), Err(SlugError::InvalidChars)));
//! assert!(matches!(Slug::new("-bad"), Err(SlugError::LeadingHyphen)));
//! assert!(matches!(Slug::new(""), Err(SlugError::Empty)));
//! ```

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{borrow::ToOwned, string::String};
use core::{fmt, ops::Deref};
#[cfg(feature = "serde")]
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

// ---------------------------------------------------------------------------
// SlugError
// ---------------------------------------------------------------------------

/// Errors that can occur when constructing a [`Slug`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SlugError {
    /// The input was empty.
    #[error("slug must not be empty")]
    Empty,
    /// The input exceeds 128 characters.
    #[error("slug must not exceed 128 characters")]
    TooLong,
    /// The input contains characters other than `[a-z0-9-]`.
    #[error("slug may only contain lowercase ASCII letters, digits, and hyphens")]
    InvalidChars,
    /// The input starts with a hyphen.
    #[error("slug must not start with a hyphen")]
    LeadingHyphen,
    /// The input ends with a hyphen.
    #[error("slug must not end with a hyphen")]
    TrailingHyphen,
    /// The input contains two or more consecutive hyphens.
    #[error("slug must not contain consecutive hyphens")]
    ConsecutiveHyphens,
}

// ---------------------------------------------------------------------------
// Slug
// ---------------------------------------------------------------------------

/// A validated, URL-friendly identifier.
///
/// See the [module-level documentation](self) for the full invariant set.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct Slug(String);

impl Slug {
    /// Construct a `Slug` from a string slice, returning a [`SlugError`] if any
    /// constraint is violated.
    ///
    /// # Errors
    ///
    /// Returns a [`SlugError`] variant that describes which constraint failed.
    pub fn new(s: impl AsRef<str>) -> Result<Self, SlugError> {
        let s = s.as_ref();
        if s.is_empty() {
            return Err(SlugError::Empty);
        }
        if s.len() > 128 {
            return Err(SlugError::TooLong);
        }
        if !s
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(SlugError::InvalidChars);
        }
        if s.starts_with('-') {
            return Err(SlugError::LeadingHyphen);
        }
        if s.ends_with('-') {
            return Err(SlugError::TrailingHyphen);
        }
        if s.contains("--") {
            return Err(SlugError::ConsecutiveHyphens);
        }
        Ok(Self(s.to_owned()))
    }

    /// Automatically convert an arbitrary title string into a valid `Slug`.
    ///
    /// The transformation pipeline:
    /// 1. Lowercase everything.
    /// 2. Replace any character that is not `[a-z0-9]` with a hyphen.
    /// 3. Collapse runs of hyphens into a single hyphen.
    /// 4. Strip leading and trailing hyphens.
    /// 5. Truncate to 128 characters.
    ///
    /// The result is guaranteed to be a valid `Slug` as long as the input
    /// contains at least one alphanumeric ASCII character; otherwise this
    /// returns a `Slug` with the value `"untitled"`.
    #[must_use]
    pub fn from_title(s: impl AsRef<str>) -> Self {
        let lowered = s.as_ref().to_lowercase();
        // Replace non-(a-z0-9) chars with hyphens
        let replaced: String = lowered
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        // Collapse consecutive hyphens
        let mut collapsed = String::with_capacity(replaced.len());
        let mut prev_hyphen = false;
        for c in replaced.chars() {
            if c == '-' {
                if !prev_hyphen {
                    collapsed.push(c);
                }
                prev_hyphen = true;
            } else {
                collapsed.push(c);
                prev_hyphen = false;
            }
        }
        // Strip leading/trailing hyphens and truncate
        let trimmed = collapsed.trim_matches('-');
        let truncated: String = trimmed.chars().take(128).collect();
        // Re-strip after truncation in case the truncation point is a hyphen
        let final_str = truncated.trim_matches('-');
        if final_str.is_empty() {
            Self("untitled".to_owned())
        } else {
            Self(final_str.to_owned())
        }
    }

    /// Return the inner string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the `Slug` and return the underlying `String`.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

// ---------------------------------------------------------------------------
// Standard trait impls
// ---------------------------------------------------------------------------

impl Deref for Slug {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for Slug {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Slug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for Slug {
    type Error = SlugError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

impl TryFrom<&str> for Slug {
    type Error = SlugError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

// ---------------------------------------------------------------------------
// Serde
// ---------------------------------------------------------------------------

#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for Slug {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::new(&s).map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// arbitrary
// ---------------------------------------------------------------------------

#[cfg(feature = "arbitrary")]
impl<'a> arbitrary::Arbitrary<'a> for Slug {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
        let len = u.int_in_range(1usize..=20)?;
        // Build a candidate from allowed chars (no hyphens in arbitrary to keep it simple)
        let inner: String = (0..len)
            .map(|_| -> arbitrary::Result<char> {
                let idx = u.int_in_range(0..=(CHARSET.len() - 1))?;
                Ok(CHARSET[idx] as char)
            })
            .collect::<arbitrary::Result<_>>()?;
        // Optionally intersperse hyphens between segments
        let segments = u.int_in_range(1usize..=3)?;
        if segments == 1 || inner.len() < 2 {
            Ok(Self(inner))
        } else {
            let step = inner.len() / segments;
            let joined: String = inner
                .as_bytes()
                .chunks(step.max(1))
                .map(|c| std::str::from_utf8(c).unwrap_or("a"))
                .collect::<Vec<_>>()
                .join("-");
            // Guaranteed valid because we only used CHARSET and joined with single hyphens
            Ok(Self(joined.trim_matches('-').to_owned()))
        }
    }
}

// ---------------------------------------------------------------------------
// proptest
// ---------------------------------------------------------------------------

#[cfg(feature = "proptest")]
pub mod proptest_strategies {
    use super::Slug;
    use proptest::prelude::*;

    /// A `proptest` strategy that generates valid [`Slug`] values.
    pub fn slug_strategy() -> impl Strategy<Value = Slug> {
        // Pattern: one or more segments of [a-z0-9]{1,20} joined by single hyphens
        prop::collection::vec("[a-z0-9]{1,20}", 1..=4).prop_map(|segs| {
            let s = segs.join("-");
            // Guaranteed valid by construction
            Slug::new(s).expect("generated slug must be valid")
        })
    }
}

#[cfg(feature = "proptest")]
impl proptest::arbitrary::Arbitrary for Slug {
    type Parameters = ();
    type Strategy = proptest::strategy::BoxedStrategy<Self>;

    fn arbitrary_with((): Self::Parameters) -> Self::Strategy {
        use proptest::prelude::*;
        proptest_strategies::slug_strategy().boxed()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Slug::new ---

    #[test]
    fn valid_slugs_are_accepted() {
        for s in ["a", "hello", "hello-world", "abc-123", "a1b2c3", "x"] {
            assert!(Slug::new(s).is_ok(), "expected {s:?} to be valid");
        }
    }

    #[test]
    fn empty_string_is_rejected() {
        assert_eq!(Slug::new(""), Err(SlugError::Empty));
    }

    #[test]
    fn too_long_string_is_rejected() {
        let s: String = "a".repeat(129);
        assert_eq!(Slug::new(&s), Err(SlugError::TooLong));
    }

    #[test]
    fn exactly_128_chars_is_accepted() {
        let s: String = "a".repeat(128);
        assert!(Slug::new(&s).is_ok());
    }

    #[test]
    fn uppercase_chars_are_rejected() {
        assert_eq!(Slug::new("Hello"), Err(SlugError::InvalidChars));
    }

    #[test]
    fn special_chars_are_rejected() {
        for s in ["hello_world", "hello world", "héllo", "hello.world"] {
            assert_eq!(
                Slug::new(s),
                Err(SlugError::InvalidChars),
                "expected {s:?} to be invalid"
            );
        }
    }

    #[test]
    fn leading_hyphen_is_rejected() {
        assert_eq!(Slug::new("-hello"), Err(SlugError::LeadingHyphen));
    }

    #[test]
    fn trailing_hyphen_is_rejected() {
        assert_eq!(Slug::new("hello-"), Err(SlugError::TrailingHyphen));
    }

    #[test]
    fn consecutive_hyphens_are_rejected() {
        assert_eq!(
            Slug::new("hello--world"),
            Err(SlugError::ConsecutiveHyphens)
        );
    }

    // --- Slug::from_title ---

    #[test]
    fn from_title_basic() {
        let slug = Slug::from_title("Hello World");
        assert_eq!(slug.as_str(), "hello-world");
    }

    #[test]
    fn from_title_strips_special_chars() {
        let slug = Slug::from_title("Hello, World! 2024");
        assert_eq!(slug.as_str(), "hello-world-2024");
    }

    #[test]
    fn from_title_collapses_multiple_spaces() {
        let slug = Slug::from_title("Hello   World");
        assert_eq!(slug.as_str(), "hello-world");
    }

    #[test]
    fn from_title_strips_leading_trailing_separators() {
        let slug = Slug::from_title("  Hello World  ");
        assert_eq!(slug.as_str(), "hello-world");
    }

    #[test]
    fn from_title_all_non_alnum_returns_untitled() {
        let slug = Slug::from_title("!!! ???");
        assert_eq!(slug.as_str(), "untitled");
    }

    #[test]
    fn from_title_empty_returns_untitled() {
        let slug = Slug::from_title("");
        assert_eq!(slug.as_str(), "untitled");
    }

    #[test]
    fn from_title_truncates_to_128() {
        let long: String = "a ".repeat(200);
        let slug = Slug::from_title(&long);
        assert!(slug.as_str().len() <= 128);
        assert!(Slug::new(slug.as_str()).is_ok());
    }

    // --- Trait impls ---

    #[test]
    fn deref_to_str() {
        let slug = Slug::new("hello").unwrap();
        let s: &str = &slug;
        assert_eq!(s, "hello");
    }

    #[test]
    fn display() {
        let slug = Slug::new("hello-world").unwrap();
        assert_eq!(format!("{slug}"), "hello-world");
    }

    #[test]
    fn as_ref_str() {
        let slug = Slug::new("hello").unwrap();
        let s: &str = slug.as_ref();
        assert_eq!(s, "hello");
    }

    #[test]
    fn try_from_string() {
        let slug = Slug::try_from("hello".to_owned()).unwrap();
        assert_eq!(slug.as_str(), "hello");
    }

    #[test]
    fn try_from_str_ref() {
        let slug = Slug::try_from("world").unwrap();
        assert_eq!(slug.as_str(), "world");
    }

    #[test]
    fn into_string() {
        let slug = Slug::new("hello").unwrap();
        assert_eq!(slug.into_string(), "hello".to_owned());
    }

    // --- Serde ---

    #[cfg(feature = "serde")]
    #[test]
    fn serde_roundtrip() {
        let slug = Slug::new("hello-world").unwrap();
        let json = serde_json::to_string(&slug).unwrap();
        assert_eq!(json, r#""hello-world""#);
        let back: Slug = serde_json::from_str(&json).unwrap();
        assert_eq!(back, slug);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_deserialize_invalid_rejects() {
        let result: Result<Slug, _> = serde_json::from_str(r#""Hello-World""#);
        assert!(result.is_err());
    }

    // --- arbitrary ---

    #[cfg(feature = "arbitrary")]
    mod arbitrary_tests {
        use super::super::Slug;
        use arbitrary::{Arbitrary, Unstructured};

        #[test]
        fn arbitrary_generates_valid_slugs() {
            let raw: Vec<u8> = (0u8..=255).cycle().take(1024).collect();
            let mut u = Unstructured::new(&raw);
            for _ in 0..50 {
                if let Ok(slug) = Slug::arbitrary(&mut u) {
                    assert!(
                        Slug::new(slug.as_str()).is_ok(),
                        "arbitrary produced invalid slug: {slug:?}"
                    );
                }
            }
        }
    }

    // --- proptest ---

    #[cfg(feature = "proptest")]
    mod proptest_tests {
        use super::super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn arbitrary_with_generates_valid_slugs(slug in <Slug as proptest::arbitrary::Arbitrary>::arbitrary_with(())) {
                prop_assert!(Slug::new(slug.as_str()).is_ok());
            }

            #[test]
            fn generated_slugs_are_always_valid(slug in proptest_strategies::slug_strategy()) {
                prop_assert!(Slug::new(slug.as_str()).is_ok());
                prop_assert!(!slug.as_str().is_empty());
                prop_assert!(slug.as_str().len() <= 128);
                prop_assert!(!slug.as_str().starts_with('-'));
                prop_assert!(!slug.as_str().ends_with('-'));
                prop_assert!(!slug.as_str().contains("--"));
            }

            #[test]
            fn from_title_always_produces_valid_slug(title in ".*") {
                let slug = Slug::from_title(&title);
                prop_assert!(Slug::new(slug.as_str()).is_ok());
            }
        }
    }
}
