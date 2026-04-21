//! `BearerToken` construction, `Authorization` header parsing, and scope validation.
//!
//! Demonstrates:
//! - `AuthScheme` variant matching
//! - `BearerToken` construction and debug redaction
//! - `BasicCredentials` from username/password
//! - `OAuth2Token` with token type
//! - `AuthorizationHeader` parse + round-trip
//! - `Scope` and `Permission` — compose, subset check, intersection
//!
//! Requires the `auth` feature (default):
//! Run: `cargo run --example auth_flow`

use api_bones::auth::{
    AuthScheme, AuthorizationHeader, BasicCredentials, BearerToken, OAuth2Token, Permission, Scope,
};

fn main() {
    // -------------------------------------------------------------------------
    // 1. AuthScheme — parse and display all variants
    // -------------------------------------------------------------------------
    println!("AuthScheme variants:");
    let schemes = [
        AuthScheme::Bearer,
        AuthScheme::Basic,
        AuthScheme::ApiKey,
        AuthScheme::OAuth2,
        AuthScheme::Digest,
        AuthScheme::Custom("NTLM".to_owned()),
    ];
    for scheme in &schemes {
        let parsed: AuthScheme = scheme.to_string().parse().unwrap();
        println!("  {scheme}  round-trips={}", parsed == *scheme);
    }

    // -------------------------------------------------------------------------
    // 2. BearerToken — construction and debug redaction
    // -------------------------------------------------------------------------
    println!("\nBearerToken:");
    let tok = BearerToken::new("eyJhbGciOiJIUzI1NiJ9.payload.sig");
    println!("  as_str = {:?}", tok.as_str());
    println!("  debug  = {tok:?}"); // must not leak the value
    assert!(!format!("{tok:?}").contains("eyJhbGciOiJIUzI1NiJ9"));

    // -------------------------------------------------------------------------
    // 3. BasicCredentials — construction + password redaction
    // -------------------------------------------------------------------------
    println!("\nBasicCredentials:");
    let creds = BasicCredentials::new("alice", "s3cr3t");    println!("  username = {}", creds.username());    println!("  debug    = {creds:?}");
    assert!(!format!("{creds:?}").contains("s3cr3t"));

    // -------------------------------------------------------------------------
    // 4. OAuth2Token — token with type
    // -------------------------------------------------------------------------
    println!("\nOAuth2Token:");
    let oauth = OAuth2Token::new("access-token-value", Some("Bearer"));
    println!("  token_type = {:?}", oauth.token_type());
    println!("  debug      = {oauth:?}");    assert!(!format!("{oauth:?}").contains("access-token-value"));

    // -------------------------------------------------------------------------
    // 5. AuthorizationHeader — parse Bearer
    // -------------------------------------------------------------------------
    println!("\nAuthorizationHeader (Bearer):");
    let auth: AuthorizationHeader = "Bearer my-secret-token".parse().unwrap();
    println!("  scheme     = {:?}", auth.scheme());
    println!("  to_string  = {auth}");
    if let AuthorizationHeader::Bearer(t) = &auth {
        println!("  token      = {}", t.as_str());
    }
    assert_eq!(auth.to_string(), "Bearer my-secret-token");

    // AuthorizationHeader — parse Basic (base64-decoded automatically)
    println!("\nAuthorizationHeader (Basic):");
    let basic: AuthorizationHeader = "Basic dXNlcjpwYXNz".parse().unwrap();
    if let AuthorizationHeader::Basic(c) = &basic {
        println!("  username = {}", c.username());        println!("  password = [REDACTED in debug]");
    }
    // Round-trip back to the same base64-encoded header value
    println!("  to_string = {basic}");
    assert_eq!(basic.to_string(), "Basic dXNlcjpwYXNz");

    // -------------------------------------------------------------------------
    // 6. Permission — construct and validate
    // -------------------------------------------------------------------------
    println!("\nPermission:");
    let p = Permission::new("orders:read").unwrap();
    println!("  as_str = {}", p.as_str());

    let empty_err = Permission::new("").unwrap_err();
    println!("  empty  error = {empty_err}");

    let ws_err = Permission::new("bad token").unwrap_err();
    println!("  space  error = {ws_err}");

    // -------------------------------------------------------------------------
    // 7. Scope — compose, check, subset, intersection
    // -------------------------------------------------------------------------
    println!("\nScope:");
    let full: Scope = "read write admin openid".parse().unwrap();
    println!("  full  = {full}  (len={})", full.len());

    let required: Scope = "read write".parse().unwrap();
    println!("  required = {required}");
    println!(
        "  required is_subset_of full = {}",
        required.is_subset_of(&full)
    );
    println!(
        "  full is_subset_of required = {}",
        full.is_subset_of(&required)
    );

    let a: Scope = "read write".parse().unwrap();
    let b: Scope = "write admin".parse().unwrap();
    let union = a.union(&b);
    let inter = a.intersection(&b);
    println!("  union({a}, {b})        = {union}");
    println!("  intersection({a}, {b}) = {inter}");

    println!("  full.contains(\"admin\") = {}", full.contains("admin"));
    println!("  full.contains(\"delete\") = {}", full.contains("delete"));

    // -------------------------------------------------------------------------
    // 8. Scope from_permissions
    // -------------------------------------------------------------------------
    let perms = ["read", "write", "openid"]
        .iter()
        .map(|s| Permission::new(s).unwrap())
        .collect::<Vec<_>>();
    let scope = Scope::from_permissions(perms);
    println!("\nScope::from_permissions = {scope}  (len={})", scope.len());
}
