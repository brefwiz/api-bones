// SPDX-License-Identifier: LicenseRef-Proprietary
//! Cross-cutting platform context bundle.
//!
//! [`OrganizationContext`] carries the tenant, principal, request-id, roles,
//! and an optional opaque attestation in a single, cheap-to-clone bundle.
//! Every downstream crate (service-kit, quorumauth, distributed-ratelimit,
//! otel-bootstrap, sqlx-switchboard) consumes this type instead of threading
//! `(org_id, principal)` pairs through every function.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::string::String;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::sync::Arc;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;
use core::fmt;

#[cfg(feature = "std")]
use std::sync::Arc;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::audit::Principal;
use crate::org_id::OrgId;
use crate::request_id::RequestId;

// ---------------------------------------------------------------------------
// Role
// ---------------------------------------------------------------------------

/// Authorization role identifier.
///
/// A lightweight, cloneable wrapper around a role name string.
/// Roles are typically used in [`OrganizationContext`] to authorize
/// operations on behalf of a principal.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Role(Arc<str>);

impl Role {
    /// Construct a `Role` from a string reference.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::Role;
    ///
    /// let admin = Role::from("admin");
    /// assert_eq!(admin.as_str(), "admin");
    /// ```
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for Role {
    fn from(s: &str) -> Self {
        Self(Arc::from(s))
    }
}

impl From<String> for Role {
    fn from(s: String) -> Self {
        Self(Arc::from(s.as_str()))
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(feature = "serde")]
impl Serialize for Role {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for Role {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(Self(Arc::from(s.as_str())))
    }
}

// ---------------------------------------------------------------------------
// AttestationKind
// ---------------------------------------------------------------------------

/// Kind of attestation token or credential.
///
/// Describes the format and origin of the raw bytes in [`Attestation::raw`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub enum AttestationKind {
    /// Biscuit capability token
    Biscuit,
    /// JWT token
    Jwt,
    /// API key
    ApiKey,
    /// mTLS certificate
    Mtls,
}

// ---------------------------------------------------------------------------
// Attestation
// ---------------------------------------------------------------------------

/// Opaque attestation / credential bundle.
///
/// Carries the raw bytes of a credential token (JWT, Biscuit, API key, etc.)
/// along with metadata about its kind. This is a convenience wrapper to avoid
/// threading `(kind, raw_bytes)` pairs separately through middleware.
#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Attestation {
    /// The kind of attestation
    pub kind: AttestationKind,
    /// The raw attestation bytes
    pub raw: Vec<u8>,
}

// ---------------------------------------------------------------------------
// OrganizationContext
// ---------------------------------------------------------------------------

/// Platform context bundle — org, principal, request-id, roles, attestation.
///
/// Carries the cross-cutting request context (tenant ID, actor identity,
/// request tracing ID, authorization roles, and optional credential) in a
/// single, cheap-to-clone value. Avoids threading `(org_id, principal)`
/// pairs separately through every function and middleware layer.
///
/// # Examples
///
/// ```rust
/// # #[cfg(feature = "uuid")] {
/// use api_bones::{OrganizationContext, OrgId, Principal, RequestId, Role, Attestation, AttestationKind};
/// use uuid::Uuid;
///
/// let org_id = OrgId::generate();
/// let principal = Principal::human(Uuid::new_v4());
/// let request_id = RequestId::new();
///
/// let ctx = OrganizationContext::new(org_id, principal, request_id)
///     .with_roles(vec![Role::from("admin")])
///     .with_attestation(Attestation {
///         kind: AttestationKind::Jwt,
///         raw: vec![1, 2, 3],
///     });
///
/// assert_eq!(ctx.roles.len(), 1);
/// assert!(ctx.attestation.is_some());
/// # }
/// ```
#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct OrganizationContext {
    /// Tenant ID
    pub org_id: OrgId,
    /// Actor identity
    pub principal: Principal,
    /// Request tracing ID
    pub request_id: RequestId,
    /// Authorization roles
    pub roles: Vec<Role>,
    /// Optional credential/attestation
    pub attestation: Option<Attestation>,
}

impl OrganizationContext {
    /// Construct a new context with org, principal, and request-id.
    ///
    /// Roles default to an empty vec, attestation to `None`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[cfg(feature = "uuid")] {
    /// use api_bones::{OrganizationContext, OrgId, Principal, RequestId};
    /// use uuid::Uuid;
    ///
    /// let ctx = OrganizationContext::new(
    ///     OrgId::generate(),
    ///     Principal::human(Uuid::new_v4()),
    ///     RequestId::new(),
    /// );
    ///
    /// assert!(ctx.roles.is_empty());
    /// assert!(ctx.attestation.is_none());
    /// # }
    /// ```
    #[must_use]
    pub fn new(org_id: OrgId, principal: Principal, request_id: RequestId) -> Self {
        Self {
            org_id,
            principal,
            request_id,
            roles: Vec::new(),
            attestation: None,
        }
    }

    /// Set the roles on this context (builder-style).
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[cfg(feature = "uuid")] {
    /// use api_bones::{OrganizationContext, OrgId, Principal, RequestId, Role};
    /// use uuid::Uuid;
    ///
    /// let ctx = OrganizationContext::new(
    ///     OrgId::generate(),
    ///     Principal::human(Uuid::new_v4()),
    ///     RequestId::new(),
    /// ).with_roles(vec![Role::from("editor")]);
    ///
    /// assert_eq!(ctx.roles.len(), 1);
    /// # }
    /// ```
    #[must_use]
    pub fn with_roles(mut self, roles: Vec<Role>) -> Self {
        self.roles = roles;
        self
    }

    /// Set the attestation on this context (builder-style).
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[cfg(feature = "uuid")] {
    /// use api_bones::{OrganizationContext, OrgId, Principal, RequestId, Attestation, AttestationKind};
    /// use uuid::Uuid;
    ///
    /// let ctx = OrganizationContext::new(
    ///     OrgId::generate(),
    ///     Principal::human(Uuid::new_v4()),
    ///     RequestId::new(),
    /// ).with_attestation(Attestation {
    ///     kind: AttestationKind::ApiKey,
    ///     raw: vec![42],
    /// });
    ///
    /// assert!(ctx.attestation.is_some());
    /// # }
    /// ```
    #[must_use]
    pub fn with_attestation(mut self, attestation: Attestation) -> Self {
        self.attestation = Some(attestation);
        self
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use core::hash::{Hash, Hasher};
    use std::collections::hash_map::DefaultHasher;

    // Role tests
    #[test]
    fn role_construction_from_str() {
        let role = Role::from("admin");
        assert_eq!(role.as_str(), "admin");
    }

    #[test]
    fn role_construction_from_string() {
        let role = Role::from("viewer".to_owned());
        assert_eq!(role.as_str(), "viewer");
    }

    #[test]
    fn role_clone_eq() {
        let role1 = Role::from("editor");
        let role2 = role1.clone();
        assert_eq!(role1, role2);
    }

    #[test]
    fn role_hash_eq() {
        let role1 = Role::from("admin");
        let role2 = Role::from("admin");

        let mut hasher1 = DefaultHasher::new();
        role1.hash(&mut hasher1);
        let hash1 = hasher1.finish();

        let mut hasher2 = DefaultHasher::new();
        role2.hash(&mut hasher2);
        let hash2 = hasher2.finish();

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn role_display() {
        let role = Role::from("admin");
        assert_eq!(format!("{role}"), "admin");
    }

    #[test]
    fn role_debug() {
        let role = Role::from("admin");
        let debug_str = format!("{role:?}");
        assert!(debug_str.contains("admin"));
    }

    // AttestationKind tests
    #[test]
    fn attestation_kind_copy() {
        let kind1 = AttestationKind::Jwt;
        let kind2 = kind1;
        assert_eq!(kind1, kind2);
    }

    #[test]
    fn attestation_kind_all_variants() {
        match AttestationKind::Biscuit {
            AttestationKind::Biscuit => {}
            _ => panic!("expected Biscuit"),
        }
        match AttestationKind::Jwt {
            AttestationKind::Jwt => {}
            _ => panic!("expected Jwt"),
        }
        match AttestationKind::ApiKey {
            AttestationKind::ApiKey => {}
            _ => panic!("expected ApiKey"),
        }
        match AttestationKind::Mtls {
            AttestationKind::Mtls => {}
            _ => panic!("expected Mtls"),
        }
    }

    // Attestation tests
    #[test]
    fn attestation_construction() {
        let att = Attestation {
            kind: AttestationKind::Jwt,
            raw: vec![1, 2, 3],
        };
        assert_eq!(att.kind, AttestationKind::Jwt);
        assert_eq!(att.raw, vec![1, 2, 3]);
    }

    #[test]
    fn attestation_clone_eq() {
        let att1 = Attestation {
            kind: AttestationKind::ApiKey,
            raw: vec![42],
        };
        let att2 = att1.clone();
        assert_eq!(att1, att2);
    }

    #[test]
    fn attestation_debug() {
        let att = Attestation {
            kind: AttestationKind::Jwt,
            raw: vec![],
        };
        let debug_str = format!("{att:?}");
        assert!(debug_str.contains("Jwt"));
    }

    // OrganizationContext tests
    #[test]
    fn org_context_construction() {
        let org_id = OrgId::new(uuid::Uuid::nil());
        let principal = Principal::system("test");
        let request_id = RequestId::new();

        let ctx = OrganizationContext::new(org_id, principal.clone(), request_id);

        assert_eq!(ctx.org_id, org_id);
        assert_eq!(ctx.principal, principal);
        assert_eq!(ctx.request_id, request_id);
        assert!(ctx.roles.is_empty());
        assert!(ctx.attestation.is_none());
    }

    #[test]
    fn org_context_with_roles() {
        let org_id = OrgId::generate();
        let principal = Principal::system("test");
        let request_id = RequestId::new();
        let roles = vec![Role::from("admin"), Role::from("editor")];

        let ctx = OrganizationContext::new(org_id, principal, request_id).with_roles(roles);

        assert_eq!(ctx.roles.len(), 2);
        assert_eq!(ctx.roles[0], Role::from("admin"));
        assert_eq!(ctx.roles[1], Role::from("editor"));
    }

    #[test]
    fn org_context_with_attestation() {
        let org_id = OrgId::generate();
        let principal = Principal::system("test");
        let request_id = RequestId::new();
        let att = Attestation {
            kind: AttestationKind::ApiKey,
            raw: vec![42],
        };

        let ctx =
            OrganizationContext::new(org_id, principal, request_id).with_attestation(att.clone());

        assert!(ctx.attestation.is_some());
        assert_eq!(ctx.attestation.unwrap(), att);
    }

    #[test]
    fn org_context_clone_eq() {
        let org_id = OrgId::generate();
        let principal = Principal::system("test");
        let request_id = RequestId::new();

        let ctx1 = OrganizationContext::new(org_id, principal, request_id)
            .with_roles(vec![Role::from("viewer")]);
        let ctx2 = ctx1.clone();

        assert_eq!(ctx1, ctx2);
    }

    #[test]
    fn org_context_debug() {
        let org_id = OrgId::generate();
        let principal = Principal::system("test");
        let request_id = RequestId::new();

        let ctx = OrganizationContext::new(org_id, principal, request_id);
        let debug_str = format!("{ctx:?}");
        assert!(debug_str.contains("OrganizationContext"));
    }

    #[test]
    fn org_context_no_attestation() {
        let org_id = OrgId::generate();
        let principal = Principal::system("test");
        let request_id = RequestId::new();

        let ctx = OrganizationContext::new(org_id, principal, request_id);

        assert!(ctx.attestation.is_none());
    }

    // Serde tests
    #[cfg(feature = "serde")]
    #[test]
    fn role_serde_roundtrip() {
        let role = Role::from("admin");
        let json = serde_json::to_string(&role).unwrap();
        let back: Role = serde_json::from_str(&json).unwrap();
        assert_eq!(role, back);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn attestation_kind_serde_roundtrip_jwt() {
        let kind = AttestationKind::Jwt;
        let json = serde_json::to_string(&kind).unwrap();
        let back: AttestationKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, back);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn attestation_serde_roundtrip() {
        let att = Attestation {
            kind: AttestationKind::ApiKey,
            raw: vec![1, 2, 3],
        };
        let json = serde_json::to_string(&att).unwrap();
        let back: Attestation = serde_json::from_str(&json).unwrap();
        assert_eq!(att, back);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn org_context_serde_roundtrip() {
        let org_id = OrgId::new(uuid::Uuid::nil());
        let principal = Principal::system("test");
        let request_id = RequestId::new();

        let ctx = OrganizationContext::new(org_id, principal, request_id)
            .with_roles(vec![Role::from("admin")])
            .with_attestation(Attestation {
                kind: AttestationKind::Jwt,
                raw: vec![42],
            });

        let json = serde_json::to_string(&ctx).unwrap();
        let back: OrganizationContext = serde_json::from_str(&json).unwrap();
        assert_eq!(ctx, back);
    }
}
