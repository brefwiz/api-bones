use api_bones::audit::{Principal, PrincipalId, PrincipalKind};
use api_bones::org_id::OrgId;
use uuid::Uuid;

/// Builder for a fake [`Principal`].
///
/// `Principal` carries no `scopes` field; the `.scopes()` method is provided
/// for ergonomic parity with higher-level test fixtures but has no effect on
/// the built `Principal` value.
///
/// # Quick start
///
/// ```rust
/// use api_bones::audit::PrincipalKind;
/// use api_bones_test::builders::FakePrincipal;
///
/// let p = FakePrincipal::user(uuid::Uuid::new_v4()).build();
/// assert!(matches!(p.kind, PrincipalKind::User));
/// ```
pub struct FakePrincipal {
    id: PrincipalId,
    kind: PrincipalKind,
    org_path: Vec<OrgId>,
}

impl FakePrincipal {
    #[must_use]
    pub fn user(id: Uuid) -> Self {
        Self {
            id: PrincipalId::from_uuid(id),
            kind: PrincipalKind::User,
            org_path: Vec::new(),
        }
    }

    #[must_use]
    pub fn agent(id: Uuid) -> Self {
        Self {
            id: PrincipalId::from_uuid(id),
            kind: PrincipalKind::Agent,
            org_path: Vec::new(),
        }
    }

    #[must_use]
    pub fn org_path(mut self, path: Vec<OrgId>) -> Self {
        self.org_path = path;
        self
    }

    /// Stored for ergonomic parity; `Principal` has no `scopes` field so these
    /// are not propagated to the built value.
    #[must_use]
    pub fn scopes(self, _scopes: &[&str]) -> Self {
        self
    }

    #[must_use]
    pub fn build(self) -> Principal {
        Principal {
            id: self.id,
            kind: self.kind,
            org_path: self.org_path,
        }
    }
}
