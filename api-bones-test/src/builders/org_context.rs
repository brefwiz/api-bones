use api_bones::audit::Principal;
use api_bones::org_context::OrganizationContext;
use api_bones::org_id::OrgId;
use api_bones::request_id::RequestId;

/// Convenience shortcut for building a fake [`OrganizationContext`].
///
/// Derives `org_id` from the first entry in `principal.org_path` (or generates
/// a fresh one) so the context is internally consistent.
///
/// # Quick start
///
/// ```rust
/// use uuid::Uuid;
/// use api_bones_test::builders::{FakePrincipal, FakeOrgContext};
///
/// let p = FakePrincipal::user(Uuid::new_v4()).build();
/// let ctx = FakeOrgContext::for_principal(&p);
/// assert!(!ctx.org_id.to_string().is_empty());
/// ```
pub struct FakeOrgContext;

impl FakeOrgContext {
    #[must_use]
    pub fn for_principal(principal: &Principal) -> OrganizationContext {
        let org_id = principal
            .org_path
            .first()
            .copied()
            .unwrap_or_else(OrgId::generate);
        let org_path = principal.org_path.clone();
        let mut ctx = OrganizationContext::new(org_id, principal.clone(), RequestId::new());
        ctx.org_path = org_path;
        ctx
    }
}
