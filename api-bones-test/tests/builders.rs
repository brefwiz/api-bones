#![cfg(feature = "builders")]

use api_bones::error::ErrorCode;
use api_bones::org_id::OrgId;
use api_bones_test::builders::{
    FakeApiResponse, FakeETag, FakePaginated, FakePrincipal, FakeProblem,
};
use chrono::Utc;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// FakeApiResponse
// ---------------------------------------------------------------------------

#[test]
fn fake_api_response_defaults() {
    let resp = FakeApiResponse::new(42u32).build();
    assert_eq!(resp.data, 42);
    assert!(resp.meta.request_id.is_some());
    assert!(resp.meta.timestamp.is_some());
    assert!(resp.links.is_none());
}

#[test]
fn fake_api_response_with_request_id() {
    let resp = FakeApiResponse::new("hi")
        .with_request_id("req-001")
        .build();
    assert_eq!(resp.meta.request_id.as_deref(), Some("req-001"));
}

#[test]
fn fake_api_response_serde_round_trip() {
    let resp = FakeApiResponse::new(99u32).build();
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["data"], 99);
    let back: api_bones::response::ApiResponse<u32> = serde_json::from_value(json).unwrap();
    assert_eq!(back.data, 99);
}

// ---------------------------------------------------------------------------
// FakePaginated
// ---------------------------------------------------------------------------

#[test]
fn fake_paginated_defaults() {
    let page = FakePaginated::new(vec![1u32, 2, 3]).build();
    assert_eq!(page.items.len(), 3);
    assert_eq!(page.total_count, 3);
    assert_eq!(page.limit, 3);
    assert_eq!(page.offset, 0);
    assert!(!page.has_more);
}

#[test]
fn fake_paginated_custom_total() {
    let page = FakePaginated::new(vec![1u32, 2, 3])
        .total(10)
        .limit(3)
        .offset(0)
        .build();
    assert!(page.has_more);
    assert_eq!(page.total_count, 10);
}

#[test]
fn fake_paginated_serde_round_trip() {
    let page = FakePaginated::new(vec!["a", "b"]).build();
    let json = serde_json::to_value(&page).unwrap();
    assert_eq!(json["total_count"], 2);
    let back: api_bones::pagination::PaginatedResponse<String> =
        serde_json::from_value(json).unwrap();
    assert_eq!(back.items.len(), 2);
}

// ---------------------------------------------------------------------------
// FakeProblem
// ---------------------------------------------------------------------------

#[test]
fn fake_problem_defaults() {
    let err = FakeProblem::new(ErrorCode::ResourceNotFound).build();
    assert_eq!(err.code, ErrorCode::ResourceNotFound);
    assert_eq!(err.status, 404);
    assert!(err.errors.is_empty());
}

#[test]
fn fake_problem_with_fields() {
    let err = FakeProblem::new(ErrorCode::ValidationFailed)
        .field("/email", "must be a valid email")
        .field("/name", "required")
        .build();
    assert_eq!(err.errors.len(), 2);
    assert_eq!(err.errors[0].field, "/email");
    assert_eq!(err.errors[1].field, "/name");
}

#[test]
fn fake_problem_validation_failed_rfc9457_shape() {
    let err = FakeProblem::new(ErrorCode::ValidationFailed)
        .field("/email", "invalid format")
        .build();
    let json = serde_json::to_value(&err).unwrap();
    let errors = json["errors"].as_array().expect("errors array missing");
    assert!(!errors.is_empty());
    assert_eq!(errors[0]["field"], "/email");
}

// ---------------------------------------------------------------------------
// FakePrincipal
// ---------------------------------------------------------------------------

#[test]
fn fake_principal_user() {
    use api_bones::audit::PrincipalKind;
    let id = Uuid::new_v4();
    let p = FakePrincipal::user(id).build();
    assert_eq!(p.as_str(), id.to_string().as_str());
    assert!(matches!(p.kind, PrincipalKind::User));
    assert!(p.org_path.is_empty());
}

#[test]
fn fake_principal_agent() {
    use api_bones::audit::PrincipalKind;
    let id = Uuid::new_v4();
    let p = FakePrincipal::agent(id).build();
    assert!(matches!(p.kind, PrincipalKind::Agent));
}

#[test]
fn fake_principal_with_org_path() {
    let org = OrgId::generate();
    let p = FakePrincipal::user(Uuid::new_v4())
        .org_path(vec![org])
        .build();
    assert_eq!(p.org_path.len(), 1);
    assert_eq!(p.org_path[0], org);
}

#[test]
fn fake_principal_scopes_is_no_op() {
    let p = FakePrincipal::user(Uuid::new_v4())
        .scopes(&["read:items", "write:items"])
        .build();
    // Principal has no scopes field — just verify it builds without panic
    assert!(p.org_path.is_empty());
}

// ---------------------------------------------------------------------------
// FakeETag
// ---------------------------------------------------------------------------

#[test]
fn fake_etag_for_updated_at_is_strong() {
    let tag = FakeETag::for_updated_at(Utc::now());
    assert!(!tag.weak);
    assert!(!tag.value.is_empty());
}

#[test]
fn fake_etag_weak() {
    let tag = FakeETag::weak("v42");
    assert!(tag.weak);
    assert_eq!(tag.value, "v42");
}
