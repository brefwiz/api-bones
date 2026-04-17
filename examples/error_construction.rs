//! Build `ApiError` with custom codes, validation errors, and request IDs.
//!
//! Demonstrates the full RFC 9457 Problem Details construction surface:
//! - All `ErrorCode` convenience constructors
//! - The builder pattern
//! - Attaching `ValidationError` details
//! - Attaching a request-ID instance field
//!
//! Run: `cargo run --example error_construction`

use api_bones::{
    ApiError, ErrorCode, ErrorTypeMode, ValidationError, new_resource_id, set_error_type_mode,
};

fn main() {
    // -------------------------------------------------------------------------
    // 1. Configure the global error-type mode to emit URNs
    // -------------------------------------------------------------------------
    set_error_type_mode(ErrorTypeMode::Urn {
        namespace: "example".into(),
    });

    // -------------------------------------------------------------------------
    // 2. Convenience constructors
    // -------------------------------------------------------------------------
    let not_found = ApiError::not_found("booking 42 not found");
    println!(
        "not_found  status={} title={:?}",
        not_found.status, not_found.title
    );
    print_json(&not_found);

    let bad_req = ApiError::bad_request("page must be > 0");
    println!("\nbad_request  status={}", bad_req.status);
    print_json(&bad_req);

    let unauth = ApiError::unauthorized("missing Bearer token");
    println!("\nunauthorized  status={}", unauth.status);

    let forbidden = ApiError::forbidden("insufficient permissions for resource");
    println!("forbidden  status={}", forbidden.status);

    let conflict = ApiError::conflict("email already registered");
    println!("conflict  status={}", conflict.status);

    let internal = ApiError::internal("database connection failed");
    println!("internal  status={}", internal.status);

    // -------------------------------------------------------------------------
    // 3. Builder pattern
    // -------------------------------------------------------------------------
    let built = ApiError::builder()
        .code(ErrorCode::ValidationFailed)
        .detail("request body failed schema validation")
        .build();
    println!("\nbuilder  status={} code={:?}", built.status, built.code);
    print_json(&built);

    // -------------------------------------------------------------------------
    // 4. Validation errors
    // -------------------------------------------------------------------------
    let validation_err = ApiError::validation_failed("invalid input").with_errors(vec![
        ValidationError {
            field: "/email".into(),
            message: "must be a valid email address".into(),
            rule: Some("email".into()),
        },
        ValidationError {
            field: "/age".into(),
            message: "must be at least 18".into(),
            rule: Some("min".into()),
        },
    ]);
    println!(
        "\nvalidation_failed  errors={}",
        validation_err.errors.len()
    );
    print_json(&validation_err);

    // -------------------------------------------------------------------------
    // 5. Attach a request-ID (populates the RFC 9457 `instance` field)
    // -------------------------------------------------------------------------
    let with_req_id = ApiError::internal("unexpected failure").with_request_id(new_resource_id());
    println!("\nwith_request_id  request_id={:?}", with_req_id.request_id);
    print_json(&with_req_id);

    // -------------------------------------------------------------------------
    // 6. URL mode — type field becomes a documentation URL
    // -------------------------------------------------------------------------
    set_error_type_mode(ErrorTypeMode::Url {
        base_url: "https://docs.example.com/errors".into(),
    });
    let url_err = ApiError::not_found("item 99 not found");
    println!("\nURL mode  code={:?}", url_err.code);

    // -------------------------------------------------------------------------
    // 7. Classify errors
    // -------------------------------------------------------------------------
    let client_err = ApiError::bad_request("oops");
    let server_err = ApiError::internal("boom");
    println!(
        "\nis_client_error: bad_request={}, internal={}",
        client_err.is_client_error(),
        server_err.is_client_error()
    );
    println!(
        "is_server_error: bad_request={}, internal={}",
        client_err.is_server_error(),
        server_err.is_server_error()
    );
}

fn print_json(value: &impl serde::Serialize) {
    let json = serde_json::to_string_pretty(value).expect("serialization failed");
    for line in json.lines() {
        println!("  {line}");
    }
}
