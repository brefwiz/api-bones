//! RFC 9457 error handling with `ApiError`.
//!
//! Demonstrates all error constructors, the builder pattern, validation errors,
//! request IDs, error type modes (URN vs URL), and all `ErrorCode` variants.
//!
//! Run: `cargo run --example error_handling`

use api_bones::{
    ApiError, ErrorCode, ErrorTypeMode, ValidationError, new_resource_id, set_error_type_mode,
};

fn main() {
    // -- Configure error type mode (URN) --
    set_error_type_mode(ErrorTypeMode::Urn {
        namespace: "api-bones".into(),
    });
    println!(
        "Error type mode: URN (namespace = {:?})",
        api_bones::urn_namespace()
    );

    // -- Convenience constructors --
    let not_found = ApiError::not_found("Booking 42 not found");
    println!("\nApiError::not_found:");
    println!("  title:  {}", not_found.title);
    println!("  status: {}", not_found.status);
    println!("  code:   {:?}", not_found.code);
    println!("  detail: {}", not_found.detail);
    print_json("  ", &not_found);

    // -- Builder pattern --
    let built = ApiError::builder()
        .code(ErrorCode::ValidationFailed)
        .detail("Request body failed validation")
        .build();
    println!("\nApiError::builder:");
    print_json("  ", &built);

    // -- Validation errors --
    let validation_err = ApiError::validation_failed("Invalid input").with_errors(vec![
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
    println!("\nWith validation errors:");
    print_json("  ", &validation_err);

    // -- Request ID --
    let with_req_id = ApiError::internal("unexpected failure").with_request_id(new_resource_id());
    println!("\nWith request ID (instance field):");
    print_json("  ", &with_req_id);

    // -- All ErrorCode variants --
    println!("\nAll ErrorCode variants:");
    let codes = [
        ErrorCode::BadRequest,
        ErrorCode::ValidationFailed,
        ErrorCode::Unauthorized,
        ErrorCode::InvalidCredentials,
        ErrorCode::TokenExpired,
        ErrorCode::TokenInvalid,
        ErrorCode::Forbidden,
        ErrorCode::InsufficientPermissions,
        ErrorCode::ResourceNotFound,
        ErrorCode::Conflict,
        ErrorCode::ResourceAlreadyExists,
        ErrorCode::UnprocessableEntity,
        ErrorCode::RateLimited,
        ErrorCode::InternalServerError,
        ErrorCode::ServiceUnavailable,
    ];
    for code in &codes {
        let err = ApiError::new(code.clone(), "example");
        println!(
            "  {:?} => status={}, title={:?}",
            code, err.status, err.title
        );
    }

    // -- Client vs server error helpers --
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

    // -- URL mode --
    set_error_type_mode(ErrorTypeMode::Url {
        base_url: "https://docs.example.com/errors".into(),
    });
    let url_mode_err = ApiError::not_found("example");
    println!("\nURL mode — type field becomes a URL:");
    print_json("  ", &url_mode_err);
}

fn print_json(prefix: &str, value: &impl serde::Serialize) {
    let json = serde_json::to_string_pretty(value).expect("serialization");
    for line in json.lines() {
        println!("{prefix}{line}");
    }
}
