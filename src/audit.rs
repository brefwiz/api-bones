//! Audit metadata for API resources.
//!
//! Provides [`AuditInfo`], an embeddable struct that tracks when a resource
//! was created and last updated, and by whom, plus [`Principal`] — the
//! canonical actor-identity newtype threaded through audit events across the
//! brefwiz ecosystem.
//!
//! # Standards
//! - Timestamps: [RFC 3339](https://www.rfc-editor.org/rfc/rfc3339)

use crate::common::Timestamp;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::borrow::Cow;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::string::String;

#[cfg(feature = "std")]
use std::borrow::Cow;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Principal
// ---------------------------------------------------------------------------

/// Canonical actor-identity newtype for audit events.
///
/// Thread the *same* `Principal` through every downstream audit-emitting
/// crate — sealwiz, batchwiz, itinerwiz, etc. — instead of forking local
/// newtypes.
///
/// # Construction
///
/// - [`Principal::new`] — for end-user / operator IDs pulled from request
///   context (allocates).
/// - [`Principal::system`] — for autonomous or system actors. Infallible
///   and `const`, so it can be used in static/const initializers.
///
/// # Semantics
///
/// Identity-only. `Principal` carries **no authorization semantics**: it
/// names an actor, nothing more. JWT/OIDC parsing, scope checks, and
/// permission resolution all belong in caller layers.
///
/// Principals are **not secrets** — `Debug` is *not* redacted, to preserve
/// visibility in audit logs and tracing output.
///
/// # Examples
///
/// ```rust
/// use api_bones::Principal;
///
/// // End-user principal
/// let alice = Principal::new("alice");
/// assert_eq!(alice.as_str(), "alice");
///
/// // System principal — const-constructible
/// const ROTATION: Principal = Principal::system("sealwiz.rotation-engine");
/// assert_eq!(ROTATION.as_str(), "sealwiz.rotation-engine");
///
/// // Owned and borrowed principals compare and hash equal when their
/// // string contents match.
/// assert_eq!(Principal::new("sealwiz.bootstrap"), Principal::system("sealwiz.bootstrap"));
/// ```
#[derive(Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize), serde(transparent))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "utoipa", schema(value_type = String))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(transparent))]
pub struct Principal(Cow<'static, str>);

impl Principal {
    /// Construct a principal from an owned or borrowed string.
    ///
    /// Use this for end-user / operator identities pulled from a request
    /// (headers, JWT subject, session context). Always allocates.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::Principal;
    ///
    /// let p = Principal::new("alice");
    /// assert_eq!(p.as_str(), "alice");
    /// ```
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(Cow::Owned(id.into()))
    }

    /// Construct a system principal from a `'static` string.
    ///
    /// Infallible and `const`, so it can appear in `const` / `static`
    /// initializers for autonomous actors (rotation engines, bootstrap
    /// workers, cron jobs).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::Principal;
    ///
    /// const BOOTSTRAP: Principal = Principal::system("sealwiz.bootstrap");
    /// assert_eq!(BOOTSTRAP.as_str(), "sealwiz.bootstrap");
    /// ```
    #[must_use]
    pub const fn system(id: &'static str) -> Self {
        Self(Cow::Borrowed(id))
    }

    /// Borrow the principal as a `&str`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::Principal;
    ///
    /// assert_eq!(Principal::new("bob").as_str(), "bob");
    /// ```
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for Principal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl core::fmt::Debug for Principal {
    // Intentionally NOT redacted — principals are identities, not secrets,
    // and must remain visible in audit logs and tracing output.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("Principal").field(&self.as_str()).finish()
    }
}

#[cfg(feature = "arbitrary")]
impl<'a> arbitrary::Arbitrary<'a> for Principal {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        let s = <String as arbitrary::Arbitrary>::arbitrary(u)?;
        Ok(Self::new(s))
    }
}

#[cfg(feature = "proptest")]
impl proptest::arbitrary::Arbitrary for Principal {
    type Parameters = ();
    type Strategy = proptest::strategy::BoxedStrategy<Self>;

    fn arbitrary_with((): ()) -> Self::Strategy {
        use proptest::prelude::*;
        any::<String>().prop_map(Self::new).boxed()
    }
}

// ---------------------------------------------------------------------------
// AuditInfo
// ---------------------------------------------------------------------------

/// Audit metadata embedded in API resource structs.
///
/// Tracks creation and last-update times (RFC 3339) and the [`Principal`]
/// that performed each action. Both actor fields are **non-optional** —
/// system processes are still actors and must declare themselves via
/// [`Principal::system`].
///
/// # Example
///
/// ```rust
/// # #[cfg(feature = "chrono")] {
/// use api_bones::{AuditInfo, Principal};
///
/// let mut info = AuditInfo::now(Principal::new("alice"));
/// info.touch(Principal::system("sealwiz.rotation-engine"));
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
// When chrono is disabled, Timestamp = String which implements Arbitrary/proptest.
#[cfg_attr(
    all(feature = "arbitrary", not(feature = "chrono")),
    derive(arbitrary::Arbitrary)
)]
#[cfg_attr(
    all(feature = "proptest", not(feature = "chrono")),
    derive(proptest_derive::Arbitrary)
)]
pub struct AuditInfo {
    /// When the resource was created (RFC 3339).
    #[cfg_attr(
        feature = "utoipa",
        schema(value_type = String, format = DateTime)
    )]
    pub created_at: Timestamp,
    /// When the resource was last updated (RFC 3339).
    #[cfg_attr(
        feature = "utoipa",
        schema(value_type = String, format = DateTime)
    )]
    pub updated_at: Timestamp,
    /// Identity of the actor who created the resource.
    pub created_by: Principal,
    /// Identity of the actor who last updated the resource.
    pub updated_by: Principal,
}

impl AuditInfo {
    /// Construct an `AuditInfo` with explicit timestamps and principals.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[cfg(feature = "chrono")] {
    /// use api_bones::{AuditInfo, Principal};
    /// use chrono::Utc;
    ///
    /// let now = Utc::now();
    /// let info = AuditInfo::new(
    ///     now,
    ///     now,
    ///     Principal::new("alice"),
    ///     Principal::new("alice"),
    /// );
    /// assert_eq!(info.created_by.as_str(), "alice");
    /// # }
    /// ```
    #[must_use]
    pub fn new(
        created_at: Timestamp,
        updated_at: Timestamp,
        created_by: Principal,
        updated_by: Principal,
    ) -> Self {
        Self {
            created_at,
            updated_at,
            created_by,
            updated_by,
        }
    }

    /// Construct an `AuditInfo` with `created_at` and `updated_at` set to
    /// the current UTC time. `updated_by` is initialized to a clone of
    /// `created_by`.
    ///
    /// Requires the `chrono` feature.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[cfg(feature = "chrono")] {
    /// use api_bones::{AuditInfo, Principal};
    ///
    /// let info = AuditInfo::now(Principal::new("alice"));
    /// assert_eq!(info.created_by.as_str(), "alice");
    /// assert_eq!(info.updated_by.as_str(), "alice");
    /// # }
    /// ```
    #[cfg(feature = "chrono")]
    #[must_use]
    pub fn now(created_by: Principal) -> Self {
        let now = chrono::Utc::now();
        let updated_by = created_by.clone();
        Self {
            created_at: now,
            updated_at: now,
            created_by,
            updated_by,
        }
    }

    /// Update `updated_at` to the current UTC time and set `updated_by`.
    ///
    /// Requires the `chrono` feature.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[cfg(feature = "chrono")] {
    /// use api_bones::{AuditInfo, Principal};
    ///
    /// let mut info = AuditInfo::now(Principal::new("alice"));
    /// info.touch(Principal::system("sealwiz.rotation-engine"));
    /// assert_eq!(info.updated_by.as_str(), "sealwiz.rotation-engine");
    /// # }
    /// ```
    #[cfg(feature = "chrono")]
    pub fn touch(&mut self, updated_by: Principal) {
        self.updated_at = chrono::Utc::now();
        self.updated_by = updated_by;
    }
}

// ---------------------------------------------------------------------------
// arbitrary / proptest impls — chrono Timestamp requires manual impl
// ---------------------------------------------------------------------------

/// When `chrono` is enabled, `Timestamp = chrono::DateTime<Utc>` which does
/// not implement `arbitrary::Arbitrary`, so we provide a hand-rolled impl.
#[cfg(all(feature = "arbitrary", feature = "chrono"))]
impl<'a> arbitrary::Arbitrary<'a> for AuditInfo {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        // Generate timestamps as i64 seconds in a sane range (year 2000–3000).
        let created_secs = <i64 as arbitrary::Arbitrary>::arbitrary(u)? % 32_503_680_000i64;
        let updated_secs = <i64 as arbitrary::Arbitrary>::arbitrary(u)? % 32_503_680_000i64;
        let created_at = chrono::DateTime::from_timestamp(created_secs.abs(), 0)
            .unwrap_or_else(chrono::Utc::now);
        let updated_at = chrono::DateTime::from_timestamp(updated_secs.abs(), 0)
            .unwrap_or_else(chrono::Utc::now);
        Ok(Self {
            created_at,
            updated_at,
            created_by: Principal::arbitrary(u)?,
            updated_by: Principal::arbitrary(u)?,
        })
    }
}

#[cfg(all(feature = "proptest", feature = "chrono"))]
impl proptest::arbitrary::Arbitrary for AuditInfo {
    type Parameters = ();
    type Strategy = proptest::strategy::BoxedStrategy<Self>;

    fn arbitrary_with((): ()) -> Self::Strategy {
        use proptest::prelude::*;
        (
            0i64..=32_503_680_000i64,
            0i64..=32_503_680_000i64,
            any::<Principal>(),
            any::<Principal>(),
        )
            .prop_map(|(cs, us, cb, ub)| Self {
                created_at: chrono::DateTime::from_timestamp(cs, 0)
                    .unwrap_or_else(chrono::Utc::now),
                updated_at: chrono::DateTime::from_timestamp(us, 0)
                    .unwrap_or_else(chrono::Utc::now),
                created_by: cb,
                updated_by: ub,
            })
            .boxed()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Principal --------------------------------------------------------

    #[test]
    fn principal_new_stores_owned_string() {
        let p = Principal::new("alice");
        assert_eq!(p.as_str(), "alice");
    }

    #[test]
    fn principal_system_is_const_and_borrowed() {
        const P: Principal = Principal::system("sealwiz.bootstrap");
        assert_eq!(P.as_str(), "sealwiz.bootstrap");
    }

    #[test]
    fn principal_display_forwards_to_as_str() {
        let s = format!("{}", Principal::new("bob"));
        assert_eq!(s, "bob");
    }

    #[test]
    fn principal_debug_is_not_redacted() {
        let s = format!("{:?}", Principal::new("alice"));
        assert!(s.contains("alice"), "debug must not redact: {s}");
        assert!(s.contains("Principal"), "debug must name the type: {s}");
    }

    #[test]
    fn principal_equality_and_hash_across_owned_and_borrowed() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let owned = Principal::new("sealwiz.bootstrap");
        let borrowed = Principal::system("sealwiz.bootstrap");
        assert_eq!(owned, borrowed);

        let mut h1 = DefaultHasher::new();
        owned.hash(&mut h1);
        let mut h2 = DefaultHasher::new();
        borrowed.hash(&mut h2);
        assert_eq!(h1.finish(), h2.finish());
    }

    #[test]
    fn principal_clone_roundtrip() {
        let p = Principal::new("carol");
        let q = p.clone();
        assert_eq!(p, q);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn principal_serde_transparent_string() {
        let p = Principal::new("alice");
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json, serde_json::json!("alice"));
        let back: Principal = serde_json::from_value(json).unwrap();
        assert_eq!(back, p);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn principal_serde_round_trip_system() {
        let p = Principal::system("sealwiz.rotation-engine");
        let json = serde_json::to_value(&p).unwrap();
        let back: Principal = serde_json::from_value(json).unwrap();
        assert_eq!(back, p);
    }

    // -- AuditInfo --------------------------------------------------------

    #[cfg(feature = "chrono")]
    #[test]
    fn now_sets_created_at_and_updated_at() {
        let before = chrono::Utc::now();
        let info = AuditInfo::now(Principal::new("alice"));
        let after = chrono::Utc::now();

        assert!(info.created_at >= before && info.created_at <= after);
        assert!(info.updated_at >= before && info.updated_at <= after);
        assert_eq!(info.created_by.as_str(), "alice");
        assert_eq!(info.updated_by.as_str(), "alice");
    }

    #[cfg(all(feature = "chrono", feature = "serde"))]
    #[test]
    fn now_with_system_principal() {
        let info = AuditInfo::now(Principal::system("sealwiz.rotation-engine"));
        let json = serde_json::to_value(&info).unwrap();
        let back: AuditInfo = serde_json::from_value(json).unwrap();
        assert_eq!(back, info);
    }

    #[cfg(feature = "chrono")]
    #[test]
    fn touch_updates_updated_at_and_updated_by() {
        let mut info = AuditInfo::now(Principal::new("alice"));
        let before_touch = chrono::Utc::now();
        info.touch(Principal::new("bob"));
        let after_touch = chrono::Utc::now();

        assert!(info.updated_at >= before_touch && info.updated_at <= after_touch);
        assert_eq!(info.updated_by.as_str(), "bob");
    }

    #[cfg(feature = "chrono")]
    #[test]
    fn new_constructor() {
        let now = chrono::Utc::now();
        let info = AuditInfo::new(
            now,
            now,
            Principal::new("alice"),
            Principal::system("sealwiz.rotation-engine"),
        );
        assert_eq!(info.created_at, now);
        assert_eq!(info.updated_at, now);
        assert_eq!(info.created_by.as_str(), "alice");
        assert_eq!(info.updated_by.as_str(), "sealwiz.rotation-engine");
    }

    #[cfg(all(feature = "chrono", feature = "serde"))]
    #[test]
    fn serde_round_trip_with_system_actor() {
        let info = AuditInfo::now(Principal::system("sealwiz.bootstrap"));
        let json = serde_json::to_value(&info).unwrap();
        let back: AuditInfo = serde_json::from_value(json).unwrap();
        assert_eq!(back, info);
    }

    #[cfg(all(feature = "chrono", feature = "serde"))]
    #[test]
    fn serde_actor_fields_are_always_present() {
        let info = AuditInfo::now(Principal::system("sealwiz.bootstrap"));
        let json = serde_json::to_value(&info).unwrap();
        assert!(
            json.get("created_by").is_some(),
            "created_by must always serialize"
        );
        assert!(
            json.get("updated_by").is_some(),
            "updated_by must always serialize"
        );
        assert_eq!(json["created_by"], serde_json::json!("sealwiz.bootstrap"));
    }
}
