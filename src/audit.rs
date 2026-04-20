//! Audit metadata for API resources.
//!
//! Provides [`AuditInfo`], an embeddable struct that tracks when a resource
//! was created and last updated, and by whom, plus [`Principal`] — the
//! canonical actor-identity newtype threaded through audit events across
//! services.
//!
//! # Standards
//! - Timestamps: [RFC 3339](https://www.rfc-editor.org/rfc/rfc3339)

use crate::common::Timestamp;
#[cfg(feature = "uuid")]
use crate::org_id::OrgId;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::borrow::Cow;
#[cfg(all(not(feature = "std"), feature = "alloc", feature = "uuid"))]
use alloc::borrow::ToOwned;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::string::String;
#[cfg(all(not(feature = "std"), feature = "alloc", feature = "uuid"))]
use alloc::string::ToString;
#[cfg(all(not(feature = "std"), feature = "alloc", feature = "uuid"))]
use alloc::vec::Vec;

#[cfg(feature = "std")]
use std::borrow::Cow;

#[cfg(feature = "uuid")]
use uuid::Uuid;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// PrincipalId
// ---------------------------------------------------------------------------

/// Opaque principal identifier. Flexible string storage:
/// UUID strings for User principals, static service names for System/Service.
#[derive(Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize), serde(transparent))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "utoipa", schema(value_type = String))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(transparent))]
pub struct PrincipalId(Cow<'static, str>);

impl PrincipalId {
    /// Wrap a `&'static str` — for compile-time system/service names.
    #[must_use]
    pub const fn static_str(s: &'static str) -> Self {
        Self(Cow::Borrowed(s))
    }

    /// Wrap an owned String — for DB round-trips and dynamic construction.
    #[must_use]
    pub fn from_owned(s: String) -> Self {
        Self(Cow::Owned(s))
    }

    /// Borrow the id as `&str`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Construct from a UUID (stores as hyphenated string).
    #[cfg(feature = "uuid")]
    #[must_use]
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(Cow::Owned(uuid.to_string()))
    }
}

impl core::fmt::Debug for PrincipalId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("PrincipalId").field(&self.as_str()).finish()
    }
}

impl core::fmt::Display for PrincipalId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Discriminator for the kind of actor a Principal represents.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum PrincipalKind {
    /// Human or end-user identity (id is a UUID string).
    User,
    /// Autonomous service-to-service identity (id is a service name).
    Service,
    /// Platform-level system actor (id is a static name, may be outside any org).
    System,
}

// ---------------------------------------------------------------------------
// PrincipalParseError
// ---------------------------------------------------------------------------

/// Error returned by [`Principal::try_parse`] when the input is not a valid
/// UUID string.
///
/// Wraps the offending input so callers can surface it in diagnostics.
/// The value is included in both `Display` and `Debug` output; callers must
/// not log this in contexts where the input might contain PII.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrincipalParseError {
    /// The string that failed UUID parsing.
    pub input: String,
}

impl core::fmt::Display for PrincipalParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "invalid Principal: expected a UUID string, got {:?}",
            self.input
        )
    }
}

#[cfg(feature = "std")]
impl std::error::Error for PrincipalParseError {}

// ---------------------------------------------------------------------------
// Principal
// ---------------------------------------------------------------------------

/// Canonical actor identity. Carries id, kind, and org-tree position.
///
/// Thread the *same* `Principal` through every downstream audit-emitting
/// crate — sealwiz, batchwiz, itinerwiz, etc. — instead of forking local
/// newtypes.
///
/// `org_path` is root-to-self inclusive. Platform-internal actors outside
/// any org tree use `org_path: vec![]`.
///
/// # Construction
///
/// - [`Principal::human`] — for human / end-user identities. Accepts a
///   [`uuid::Uuid`] to prevent PII (emails, display names) from entering
///   audit logs. Requires the `uuid` feature.
/// - [`Principal::try_parse`] — parse a UUID string into a `Principal`.
///   Returns [`PrincipalParseError`] for non-UUID input. Requires `uuid`.
/// - [`Principal::system`] — for autonomous or system actors. Infallible but
///   no longer `const` due to `Vec` in `org_path`.
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
/// # #[cfg(feature = "uuid")] {
/// use api_bones::Principal;
/// use uuid::Uuid;
///
/// // Human principal — UUID only, no emails or display names
/// let id = Uuid::new_v4();
/// let alice = Principal::human(id);
/// assert_eq!(alice.as_str(), id.to_string().as_str());
///
/// // System principal
/// let rotation = Principal::system("sealwiz.rotation-engine");
/// assert_eq!(rotation.as_str(), "sealwiz.rotation-engine");
/// # }
/// ```
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Principal {
    /// The opaque principal identifier.
    pub id: PrincipalId,
    /// The kind of actor this principal represents.
    pub kind: PrincipalKind,
    /// Org path from root to the acting org (inclusive). Empty = platform scope.
    /// Only present when the `uuid` feature is enabled.
    #[cfg(feature = "uuid")]
    #[cfg_attr(feature = "serde", serde(default))]
    pub org_path: Vec<OrgId>,
}

impl Principal {
    /// Construct a principal for a human actor from a [`uuid::Uuid`].
    ///
    /// This is the correct constructor for end-user / operator identities.
    /// By requiring a `Uuid` the API prevents callers from accidentally
    /// passing emails, display names, or other PII that would propagate into
    /// audit logs and OTEL spans (see issue #204).
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[cfg(feature = "uuid")] {
    /// use api_bones::Principal;
    /// use uuid::Uuid;
    ///
    /// let id = Uuid::new_v4();
    /// let p = Principal::human(id);
    /// assert_eq!(p.as_str(), id.to_string().as_str());
    /// # }
    /// ```
    #[cfg(feature = "uuid")]
    #[must_use]
    pub fn human(uuid: Uuid) -> Self {
        Self {
            id: PrincipalId::from_uuid(uuid),
            kind: PrincipalKind::User,
            #[cfg(feature = "uuid")]
            org_path: Vec::new(),
        }
    }

    /// Parse a UUID string into a `Principal`.
    ///
    /// Accepts any UUID text form that [`uuid::Uuid::parse_str`] recognises
    /// (hyphenated, simple, URN, braced). Returns [`PrincipalParseError`] for
    /// anything else, including emails and empty strings.
    ///
    /// # Errors
    ///
    /// Returns [`PrincipalParseError`] when `s` is not a valid UUID string.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[cfg(feature = "uuid")] {
    /// use api_bones::Principal;
    ///
    /// let p = Principal::try_parse("550e8400-e29b-41d4-a716-446655440000").unwrap();
    /// assert_eq!(p.as_str(), "550e8400-e29b-41d4-a716-446655440000");
    ///
    /// assert!(Principal::try_parse("alice@example.com").is_err());
    /// # }
    /// ```
    #[cfg(feature = "uuid")]
    pub fn try_parse(s: &str) -> Result<Self, PrincipalParseError> {
        Uuid::parse_str(s)
            .map(Self::human)
            .map_err(|_| PrincipalParseError {
                input: s.to_owned(),
            })
    }

    /// Construct a system principal from a `&'static` string.
    ///
    /// Infallible but no longer `const` since `org_path` is a `Vec`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::Principal;
    ///
    /// let bootstrap = Principal::system("sealwiz.bootstrap");
    /// assert_eq!(bootstrap.as_str(), "sealwiz.bootstrap");
    /// ```
    #[must_use]
    pub fn system(id: &'static str) -> Self {
        Self {
            id: PrincipalId::static_str(id),
            kind: PrincipalKind::System,
            #[cfg(feature = "uuid")]
            org_path: Vec::new(),
        }
    }

    /// Borrow the principal as a `&str`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::Principal;
    ///
    /// assert_eq!(Principal::system("bob").as_str(), "bob");
    /// ```
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.id.as_str()
    }

    /// Set the org path on this principal (builder-style).
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[cfg(feature = "uuid")] {
    /// use api_bones::{Principal, OrgId};
    /// use uuid::Uuid;
    ///
    /// let p = Principal::human(Uuid::nil())
    ///     .with_org_path(vec![OrgId::generate()]);
    /// assert!(!p.org_path.is_empty());
    /// # }
    /// ```
    #[cfg(feature = "uuid")]
    #[must_use]
    pub fn with_org_path(mut self, org_path: Vec<OrgId>) -> Self {
        self.org_path = org_path;
        self
    }

    /// Returns the org ancestry path as a comma-separated UUID string.
    ///
    /// Produces `""` for platform-internal actors with no org affiliation,
    /// and `"<uuid1>,<uuid2>,..."` (root-to-self) for org-scoped actors.
    /// Intended for use as an OTEL span attribute value (`enduser.org_path`).
    ///
    /// ```
    /// # #[cfg(feature = "uuid")] {
    /// use api_bones::Principal;
    ///
    /// let p = Principal::system("svc");
    /// assert_eq!(p.org_path_display(), "");
    /// # }
    /// ```
    #[cfg(feature = "uuid")]
    #[must_use]
    pub fn org_path_display(&self) -> String {
        self.org_path
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",")
    }
}

impl core::fmt::Display for Principal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// When `uuid` is available, generate UUID-backed principals so the fuzzer
/// never produces PII-shaped values (emails, display names, etc.).
#[cfg(all(feature = "arbitrary", feature = "uuid"))]
impl<'a> arbitrary::Arbitrary<'a> for Principal {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        let bytes = <[u8; 16] as arbitrary::Arbitrary>::arbitrary(u)?;
        Ok(Self::human(Uuid::from_bytes(bytes)))
    }
}

/// Fallback when `uuid` feature is disabled: generate an arbitrary system principal.
/// This path should rarely be reached in practice since `uuid` is in the
/// default feature set.
#[cfg(all(feature = "arbitrary", not(feature = "uuid")))]
impl<'a> arbitrary::Arbitrary<'a> for Principal {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        let s = <String as arbitrary::Arbitrary>::arbitrary(u)?;
        Ok(Self {
            id: PrincipalId::from_owned(s),
            kind: PrincipalKind::System,
            #[cfg(feature = "uuid")]
            org_path: Vec::new(),
        })
    }
}

/// arbitrary impl for `PrincipalId`
#[cfg(all(feature = "arbitrary", feature = "uuid"))]
impl<'a> arbitrary::Arbitrary<'a> for PrincipalId {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        let bytes = <[u8; 16] as arbitrary::Arbitrary>::arbitrary(u)?;
        Ok(Self::from_uuid(Uuid::from_bytes(bytes)))
    }
}

#[cfg(all(feature = "arbitrary", not(feature = "uuid")))]
impl<'a> arbitrary::Arbitrary<'a> for PrincipalId {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        let s = <String as arbitrary::Arbitrary>::arbitrary(u)?;
        Ok(Self::from_owned(s))
    }
}

/// arbitrary impl for `PrincipalKind`
#[cfg(feature = "arbitrary")]
impl<'a> arbitrary::Arbitrary<'a> for PrincipalKind {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        match <u8 as arbitrary::Arbitrary>::arbitrary(u)? % 3 {
            0 => Ok(Self::User),
            1 => Ok(Self::Service),
            _ => Ok(Self::System),
        }
    }
}

/// When `uuid` is available, generate UUID-backed principals so proptest
/// never generates PII-shaped values (emails, display names, etc.).
#[cfg(all(feature = "proptest", feature = "uuid"))]
impl proptest::arbitrary::Arbitrary for Principal {
    type Parameters = ();
    type Strategy = proptest::strategy::BoxedStrategy<Self>;

    fn arbitrary_with((): ()) -> Self::Strategy {
        use proptest::prelude::*;
        any::<[u8; 16]>()
            .prop_map(|b| Self::human(Uuid::from_bytes(b)))
            .boxed()
    }
}

/// Fallback when `uuid` feature is disabled.
#[cfg(all(feature = "proptest", not(feature = "uuid")))]
impl proptest::arbitrary::Arbitrary for Principal {
    type Parameters = ();
    type Strategy = proptest::strategy::BoxedStrategy<Self>;

    fn arbitrary_with((): ()) -> Self::Strategy {
        use proptest::prelude::*;
        any::<String>()
            .prop_map(|s| Self {
                id: PrincipalId::from_owned(s),
                kind: PrincipalKind::System,
                #[cfg(feature = "uuid")]
                org_path: Vec::new(),
            })
            .boxed()
    }
}

/// proptest impl for `PrincipalId`
#[cfg(all(feature = "proptest", feature = "uuid"))]
impl proptest::arbitrary::Arbitrary for PrincipalId {
    type Parameters = ();
    type Strategy = proptest::strategy::BoxedStrategy<Self>;

    fn arbitrary_with((): ()) -> Self::Strategy {
        use proptest::prelude::*;
        any::<[u8; 16]>()
            .prop_map(|b| Self::from_uuid(Uuid::from_bytes(b)))
            .boxed()
    }
}

#[cfg(all(feature = "proptest", not(feature = "uuid")))]
impl proptest::arbitrary::Arbitrary for PrincipalId {
    type Parameters = ();
    type Strategy = proptest::strategy::BoxedStrategy<Self>;

    fn arbitrary_with((): ()) -> Self::Strategy {
        use proptest::prelude::*;
        any::<String>().prop_map(|s| Self::from_owned(s)).boxed()
    }
}

/// proptest impl for `PrincipalKind`
#[cfg(feature = "proptest")]
impl proptest::arbitrary::Arbitrary for PrincipalKind {
    type Parameters = ();
    type Strategy = proptest::strategy::BoxedStrategy<Self>;

    fn arbitrary_with((): ()) -> Self::Strategy {
        use proptest::prelude::*;
        prop_oneof![Just(Self::User), Just(Self::Service), Just(Self::System),].boxed()
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
/// # #[cfg(feature = "uuid")] {
/// use uuid::Uuid;
/// let info = AuditInfo::now(Principal::human(Uuid::nil()));
/// # }
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
    /// # #[cfg(all(feature = "chrono", feature = "uuid"))] {
    /// use api_bones::{AuditInfo, Principal};
    /// use chrono::Utc;
    /// use uuid::Uuid;
    ///
    /// let now = Utc::now();
    /// let actor = Principal::human(Uuid::nil());
    /// let info = AuditInfo::new(now, now, actor.clone(), actor);
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
    /// # use uuid::Uuid;
    /// let actor = Principal::human(Uuid::nil());
    /// let info = AuditInfo::now(actor.clone());
    /// assert_eq!(info.created_by, actor);
    /// assert_eq!(info.updated_by, actor);
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
    /// # use uuid::Uuid;
    /// let mut info = AuditInfo::now(Principal::human(Uuid::nil()));
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
// ResolvedPrincipal — read-path display helper
// ---------------------------------------------------------------------------

/// A [`Principal`] paired with an optional human-readable display name.
///
/// `Principal` stores only an opaque UUID — never PII. When a presentation
/// layer (API response, audit log UI) needs to show a user-friendly name, an
/// identity service resolves the UUID at read time and wraps it here.
/// The display name is **never persisted**; only the opaque `id` is stored.
///
/// # Examples
///
/// ```rust
/// use api_bones::{Principal, ResolvedPrincipal};
/// # #[cfg(feature = "uuid")] {
/// use uuid::Uuid;
///
/// let id = Principal::human(Uuid::nil());
/// let r = ResolvedPrincipal::new(id, Some("Alice Martin".to_owned()));
/// assert_eq!(r.display(), "Alice Martin");
///
/// let anonymous = ResolvedPrincipal::new(Principal::human(Uuid::nil()), None);
/// assert_eq!(anonymous.display(), anonymous.id.as_str());
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ResolvedPrincipal {
    /// The opaque, stored identity.
    pub id: Principal,
    /// Human-readable display name resolved from the identity service.
    /// `None` when the resolution has not been performed or the actor is
    /// a system principal with no display name.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub display_name: Option<String>,
}

impl ResolvedPrincipal {
    /// Wrap a [`Principal`] with an optional display name.
    #[must_use]
    pub fn new(id: Principal, display_name: Option<String>) -> Self {
        Self { id, display_name }
    }

    /// Return the display name when available, otherwise fall back to the
    /// opaque principal string (UUID or system name).
    #[must_use]
    pub fn display(&self) -> &str {
        self.display_name
            .as_deref()
            .unwrap_or_else(|| self.id.as_str())
    }
}

impl From<Principal> for ResolvedPrincipal {
    fn from(id: Principal) -> Self {
        Self {
            id,
            display_name: None,
        }
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
    #[cfg(feature = "uuid")]
    use uuid::Uuid;

    // -- PrincipalId --------------------------------------------------------

    #[test]
    fn principal_id_static_str() {
        let id = PrincipalId::static_str("foo");
        assert_eq!(id.as_str(), "foo");
    }

    #[test]
    fn principal_id_from_owned() {
        let id = PrincipalId::from_owned("bar".to_owned());
        assert_eq!(id.as_str(), "bar");
    }

    #[cfg(feature = "uuid")]
    #[test]
    fn principal_id_from_uuid() {
        let id = PrincipalId::from_uuid(Uuid::nil());
        assert_eq!(id.as_str(), "00000000-0000-0000-0000-000000000000");
    }

    #[test]
    fn principal_id_display() {
        let id = PrincipalId::static_str("test");
        assert_eq!(format!("{id}"), "test");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn principal_id_serde_transparent() {
        let id = PrincipalId::static_str("myid");
        let json = serde_json::to_value(&id).unwrap();
        assert_eq!(json, serde_json::json!("myid"));
        let back: PrincipalId = serde_json::from_value(json).unwrap();
        assert_eq!(back, id);
    }

    // -- PrincipalKind -------------------------------------------------------

    #[test]
    fn principal_kind_copy_and_eq() {
        let k1 = PrincipalKind::User;
        let k2 = k1;
        assert_eq!(k1, k2);
    }

    #[test]
    fn principal_kind_all_variants() {
        let _ = PrincipalKind::User;
        let _ = PrincipalKind::Service;
        let _ = PrincipalKind::System;
    }

    // -- Principal --------------------------------------------------------

    #[cfg(feature = "uuid")]
    #[test]
    fn principal_human_has_user_kind() {
        let p = Principal::human(Uuid::nil());
        assert_eq!(p.kind, PrincipalKind::User);
    }

    #[cfg(feature = "uuid")]
    #[test]
    fn principal_human_has_empty_org_path() {
        let p = Principal::human(Uuid::nil());
        assert!(p.org_path.is_empty());
    }

    #[test]
    fn principal_system_has_system_kind() {
        let p = Principal::system("s");
        assert_eq!(p.kind, PrincipalKind::System);
    }

    #[test]
    fn principal_system_has_empty_org_path() {
        let p = Principal::system("s");
        assert!(p.org_path.is_empty());
    }

    #[test]
    fn principal_with_org_path_builder() {
        let org_id = crate::org_id::OrgId::generate();
        let p = Principal::system("test").with_org_path(vec![org_id]);
        assert_eq!(p.org_path.len(), 1);
        assert_eq!(p.org_path[0], org_id);
    }

    #[cfg(feature = "uuid")]
    #[test]
    fn org_path_display_empty_for_system_principal() {
        let p = Principal::system("svc");
        assert_eq!(p.org_path_display(), "");
    }

    #[cfg(feature = "uuid")]
    #[test]
    fn org_path_display_single_org() {
        let org_id = crate::org_id::OrgId::generate();
        let p = Principal::system("svc").with_org_path(vec![org_id]);
        assert_eq!(p.org_path_display(), org_id.to_string());
    }

    #[cfg(feature = "uuid")]
    #[test]
    fn org_path_display_multiple_orgs_comma_separated() {
        let root = crate::org_id::OrgId::generate();
        let child = crate::org_id::OrgId::generate();
        let p = Principal::system("svc").with_org_path(vec![root, child]);
        assert_eq!(p.org_path_display(), format!("{root},{child}"));
    }

    #[cfg(feature = "uuid")]
    #[test]
    fn principal_try_parse_accepts_valid_uuid() {
        let s = "550e8400-e29b-41d4-a716-446655440000";
        let p = Principal::try_parse(s).expect("valid UUID should parse");
        assert_eq!(p.as_str(), s);
    }

    #[cfg(feature = "uuid")]
    #[test]
    fn principal_try_parse_sets_user_kind() {
        let p = Principal::try_parse("550e8400-e29b-41d4-a716-446655440000").unwrap();
        assert_eq!(p.kind, PrincipalKind::User);
    }

    #[cfg(feature = "uuid")]
    #[test]
    fn principal_try_parse_rejects_email_string() {
        let err = Principal::try_parse("alice@example.com").expect_err("email must be rejected");
        assert_eq!(err.input, "alice@example.com");
        assert!(err.to_string().contains("alice@example.com"));
    }

    #[cfg(feature = "uuid")]
    #[test]
    fn principal_try_parse_rejects_empty_string() {
        let err = Principal::try_parse("").expect_err("empty string must be rejected");
        assert_eq!(err.input, "");
    }

    #[test]
    fn principal_as_str_returns_id_str() {
        let p = Principal::system("x");
        assert_eq!(p.as_str(), "x");
    }

    #[cfg(feature = "uuid")]
    #[test]
    fn principal_display_forwards_to_as_str() {
        let p = Principal::human(Uuid::nil());
        let s = format!("{p}");
        assert_eq!(s, Uuid::nil().to_string());
    }

    #[cfg(feature = "uuid")]
    #[test]
    fn principal_debug_is_not_redacted() {
        let p = Principal::human(Uuid::nil());
        let s = format!("{p:?}");
        assert!(
            s.contains(&Uuid::nil().to_string()),
            "debug must not redact: {s}"
        );
        assert!(s.contains("Principal"), "debug must name the type: {s}");
    }

    #[test]
    fn principal_equality_and_hash_across_owned_and_borrowed() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let p1 = Principal::system("sealwiz.bootstrap");
        let p2 = Principal::system("sealwiz.bootstrap");
        assert_eq!(p1, p2);

        let mut h1 = DefaultHasher::new();
        p1.hash(&mut h1);
        let mut h2 = DefaultHasher::new();
        p2.hash(&mut h2);
        assert_eq!(h1.finish(), h2.finish());
    }

    #[cfg(feature = "uuid")]
    #[test]
    fn principal_clone_roundtrip() {
        let p = Principal::human(Uuid::nil());
        let q = p.clone();
        assert_eq!(p, q);
    }

    #[cfg(all(feature = "serde", feature = "uuid"))]
    #[test]
    fn principal_serde_struct_roundtrip_human() {
        let p = Principal::human(Uuid::nil());
        let json = serde_json::to_value(&p).unwrap();
        let back: Principal = serde_json::from_value(json).unwrap();
        assert_eq!(back, p);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn principal_serde_struct_roundtrip_system() {
        let p = Principal::system("sealwiz.rotation-engine");
        let json = serde_json::to_value(&p).unwrap();
        let back: Principal = serde_json::from_value(json).unwrap();
        assert_eq!(back, p);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn principal_serde_includes_org_path() {
        let p = Principal::system("test");
        let json = serde_json::to_value(&p).unwrap();
        assert!(json.get("org_path").is_some());
    }

    // -- AuditInfo --------------------------------------------------------

    #[cfg(all(feature = "chrono", feature = "uuid"))]
    #[test]
    fn now_sets_created_at_and_updated_at() {
        let actor = Principal::human(Uuid::nil());
        let before = chrono::Utc::now();
        let info = AuditInfo::now(actor.clone());
        let after = chrono::Utc::now();

        assert!(info.created_at >= before && info.created_at <= after);
        assert!(info.updated_at >= before && info.updated_at <= after);
        assert_eq!(info.created_by, actor);
        assert_eq!(info.updated_by, actor);
    }

    #[cfg(all(feature = "chrono", feature = "serde"))]
    #[test]
    fn now_with_system_principal() {
        let info = AuditInfo::now(Principal::system("sealwiz.rotation-engine"));
        let json = serde_json::to_value(&info).unwrap();
        let back: AuditInfo = serde_json::from_value(json).unwrap();
        assert_eq!(back, info);
    }

    #[cfg(all(feature = "chrono", feature = "uuid"))]
    #[test]
    fn touch_updates_updated_at_and_updated_by() {
        let mut info = AuditInfo::now(Principal::human(Uuid::nil()));
        let engine = Principal::system("sealwiz.rotation-engine");
        let before_touch = chrono::Utc::now();
        info.touch(engine.clone());
        let after_touch = chrono::Utc::now();

        assert!(info.updated_at >= before_touch && info.updated_at <= after_touch);
        assert_eq!(info.updated_by, engine);
    }

    #[cfg(all(feature = "chrono", feature = "uuid"))]
    #[test]
    fn new_constructor() {
        let now = chrono::Utc::now();
        let actor = Principal::human(Uuid::nil());
        let engine = Principal::system("sealwiz.rotation-engine");
        let info = AuditInfo::new(now, now, actor.clone(), engine.clone());
        assert_eq!(info.created_at, now);
        assert_eq!(info.updated_at, now);
        assert_eq!(info.created_by, actor);
        assert_eq!(info.updated_by, engine);
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
        // Principal is now a struct with id, kind, org_path fields
        assert_eq!(
            json["created_by"]["id"],
            serde_json::json!("sealwiz.bootstrap")
        );
    }

    // -- PrincipalParseError ----------------------------------------------

    #[test]
    fn principal_parse_error_display_contains_input() {
        let err = PrincipalParseError {
            input: "bad-value".to_owned(),
        };
        assert!(err.to_string().contains("bad-value"));
    }

    #[cfg(feature = "std")]
    #[test]
    fn principal_parse_error_is_std_error() {
        let err = PrincipalParseError {
            input: "x".to_owned(),
        };
        let _: &dyn std::error::Error = &err;
    }

    // -- ResolvedPrincipal ------------------------------------------------

    #[cfg(feature = "uuid")]
    #[test]
    fn resolved_principal_new_and_display_with_name() {
        let p = Principal::human(Uuid::nil());
        let r = ResolvedPrincipal::new(p, Some("Alice Martin".to_owned()));
        assert_eq!(r.display(), "Alice Martin");
    }

    #[cfg(feature = "uuid")]
    #[test]
    fn resolved_principal_display_falls_back_to_uuid() {
        let p = Principal::human(Uuid::nil());
        let r = ResolvedPrincipal::new(p.clone(), None);
        assert_eq!(r.display(), p.as_str());
    }

    #[cfg(feature = "uuid")]
    #[test]
    fn resolved_principal_from_principal() {
        let p = Principal::human(Uuid::nil());
        let r = ResolvedPrincipal::from(p.clone());
        assert_eq!(r.id, p);
        assert!(r.display_name.is_none());
    }

    #[cfg(all(feature = "uuid", feature = "serde"))]
    #[test]
    fn resolved_principal_serde_omits_none_display_name() {
        let p = Principal::human(Uuid::nil());
        let r = ResolvedPrincipal::from(p);
        let json = serde_json::to_value(&r).unwrap();
        assert!(
            json.get("display_name").is_none(),
            "display_name must be absent when None"
        );
    }

    #[cfg(all(feature = "uuid", feature = "serde"))]
    #[test]
    fn resolved_principal_serde_includes_display_name_when_set() {
        let p = Principal::human(Uuid::nil());
        let r = ResolvedPrincipal::new(p, Some("Bob".to_owned()));
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["display_name"], serde_json::json!("Bob"));
    }
}
