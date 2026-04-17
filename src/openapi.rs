//! OpenAPI schema helpers: [`Example`] wrapper and [`DeprecatedField`] marker.
//!
//! ## `Example<T>`
//!
//! A transparent newtype that carries a typed value alongside schema metadata
//! so OpenAPI generators can surface inline examples.
//!
//! ```rust
//! use api_bones::openapi::Example;
//!
//! let ex: Example<u32> = Example::new(42);
//! assert_eq!(*ex, 42);
//! ```
//!
//! ## `DeprecatedField`
//!
//! A transparent newtype that marks a schema field as deprecated in the
//! generated OpenAPI output and optionally carries a replacement hint.
//!
//! ```rust
//! use api_bones::openapi::DeprecatedField;
//!
//! let d = DeprecatedField::new("old_name").with_replacement("new_name");
//! assert_eq!(d.replacement(), Some("new_name"));
//! ```

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::string::String;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Example<T>
// ---------------------------------------------------------------------------

/// A transparent wrapper that carries a typed value for OpenAPI `example`.
///
/// Serializes/deserializes identically to `T` (transparent serde).
///
/// When the `utoipa` feature is enabled, the value is exposed as a schema
/// example on the inner type.  When the `schemars` feature is enabled, the
/// wrapper delegates to `T`'s schema.
///
/// # Examples
///
/// ```rust
/// use api_bones::openapi::Example;
///
/// let ex = Example::new("hello");
/// assert_eq!(*ex, "hello");
/// assert_eq!(ex.into_inner(), "hello");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct Example<T>(pub T);

impl<T> Example<T> {
    /// Wrap a value as an `Example`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::openapi::Example;
    ///
    /// let ex = Example::new(42u32);
    /// assert_eq!(ex.value(), &42);
    /// ```
    #[must_use]
    #[inline]
    pub const fn new(value: T) -> Self {
        Self(value)
    }

    /// Borrow the inner value.
    #[must_use]
    #[inline]
    pub const fn value(&self) -> &T {
        &self.0
    }

    /// Consume the wrapper and return the inner value.
    #[must_use]
    #[inline]
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> core::ops::Deref for Example<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> core::ops::DerefMut for Example<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> From<T> for Example<T> {
    #[inline]
    fn from(value: T) -> Self {
        Self(value)
    }
}

// ---------------------------------------------------------------------------
// DeprecatedField
// ---------------------------------------------------------------------------

/// A transparent wrapper that marks a schema field as deprecated in the
/// generated OpenAPI output.
///
/// The inner value is the **field name** (a string) that is being deprecated.
/// Optionally carries a replacement hint shown in documentation.
///
/// Serializes/deserializes the field name transparently.
///
/// # Examples
///
/// ```rust
/// use api_bones::openapi::DeprecatedField;
///
/// let d = DeprecatedField::new("legacy_id").with_replacement("resource_id");
/// assert_eq!(d.field_name(), "legacy_id");
/// assert_eq!(d.replacement(), Some("resource_id"));
/// ```
#[cfg(any(feature = "std", feature = "alloc"))]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct DeprecatedField {
    /// The name of the deprecated field.
    pub field_name: String,
    /// Optional migration hint pointing to the replacement field or endpoint.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub replacement: Option<String>,
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl DeprecatedField {
    /// Create a new `DeprecatedField` marker.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::openapi::DeprecatedField;
    ///
    /// let d = DeprecatedField::new("old_id");
    /// assert_eq!(d.field_name(), "old_id");
    /// assert!(d.replacement().is_none());
    /// ```
    #[must_use]
    pub fn new(field_name: impl Into<String>) -> Self {
        Self {
            field_name: field_name.into(),
            replacement: None,
        }
    }

    /// Attach a replacement hint (builder-style).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::openapi::DeprecatedField;
    ///
    /// let d = DeprecatedField::new("old_id").with_replacement("resource_id");
    /// assert_eq!(d.replacement(), Some("resource_id"));
    /// ```
    #[must_use]
    pub fn with_replacement(mut self, replacement: impl Into<String>) -> Self {
        self.replacement = Some(replacement.into());
        self
    }

    /// The deprecated field name.
    #[must_use]
    pub fn field_name(&self) -> &str {
        &self.field_name
    }

    /// The optional replacement hint.
    #[must_use]
    pub fn replacement(&self) -> Option<&str> {
        self.replacement.as_deref()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_new_and_deref() {
        let ex = Example::new(42u32);
        assert_eq!(*ex, 42);
        assert_eq!(ex.value(), &42);
    }

    #[test]
    fn example_into_inner() {
        let ex = Example::new("hello");
        assert_eq!(ex.into_inner(), "hello");
    }

    #[test]
    fn example_from() {
        let ex: Example<u32> = 7u32.into();
        assert_eq!(*ex, 7);
    }

    #[test]
    fn example_deref_mut() {
        let mut ex = Example::new(1u32);
        *ex = 2;
        assert_eq!(*ex, 2);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn example_transparent_serde() {
        let ex = Example::new(42u32);
        let json = serde_json::to_value(&ex).unwrap();
        // Transparent: serializes as 42, not {"0": 42}
        assert_eq!(json, serde_json::json!(42));
        let back: Example<u32> = serde_json::from_value(json).unwrap();
        assert_eq!(back, ex);
    }

    #[cfg(any(feature = "std", feature = "alloc"))]
    #[test]
    fn deprecated_field_new() {
        let d = DeprecatedField::new("old_id");
        assert_eq!(d.field_name(), "old_id");
        assert!(d.replacement().is_none());
    }

    #[cfg(any(feature = "std", feature = "alloc"))]
    #[test]
    fn deprecated_field_with_replacement() {
        let d = DeprecatedField::new("old_id").with_replacement("resource_id");
        assert_eq!(d.replacement(), Some("resource_id"));
    }

    #[cfg(all(feature = "serde", any(feature = "std", feature = "alloc")))]
    #[test]
    fn deprecated_field_serde_omits_none_replacement() {
        let d = DeprecatedField::new("legacy");
        let json = serde_json::to_value(&d).unwrap();
        assert!(json.get("replacement").is_none());
    }

    #[cfg(all(feature = "serde", any(feature = "std", feature = "alloc")))]
    #[test]
    fn deprecated_field_serde_round_trip() {
        let d = DeprecatedField::new("old").with_replacement("new");
        let json = serde_json::to_value(&d).unwrap();
        let back: DeprecatedField = serde_json::from_value(json).unwrap();
        assert_eq!(back, d);
    }
}
