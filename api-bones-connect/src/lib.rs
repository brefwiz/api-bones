// SPDX-License-Identifier: MIT
//! Connect RPC adapter primitives for api-bones services (ADR-0096).
//!
//! This crate re-exports the [`api_bones::connect`] module surface for
//! consumers who want a dedicated dependency rather than enabling the
//! `connect` feature on `api-bones` directly.
//!
//! It also owns the transport's **client identity**, which is acquired from the
//! SPIFFE Workload API rather than assembled by the consumer. See
//! [`workload_identity`] and [`connect_http_client`].
//!
//! # Usage
//!
//! ```toml
//! [dependencies]
//! api-bones-connect = "0.1"
//! ```
//!
//! ```rust,ignore
//! use api_bones_connect::{
//!     DomainErrorKind, IntoDomainErrorKind, domain_to_connect,
//!     chrono_to_timestamp, parse_uuid, build_page, ConnectOptionExt as _,
//! };
//!
//! // In-fleet call. No CA path, socket path, bundle or peer list is named.
//! let http = api_bones_connect::connect_http_client(&uri).await?;
//! ```

pub use api_bones::connect::*;

pub mod workload_identity;

pub use workload_identity::{WorkloadIdentityError, workload_client_tls_config};

use connectrpc::client::HttpClient;
use http::Uri;

/// Build an [`HttpClient`] for `uri`, taking client identity from the Workload
/// API.
///
/// This shadows [`api_bones::connect::connect_http_client`], which trusts the
/// OS root store and presents no client certificate. That function remains
/// available as [`os_trust_http_client`] for callers genuinely talking to the
/// public internet.
///
/// - `https://` → mutual TLS with the live SVID, verifying the peer against the
///   caller's own trust domain.
/// - any other scheme → cleartext h2c, as before. A plaintext URI is an
///   explicit choice by the caller, not a downgrade decided here.
///
/// # Errors
///
/// Propagates [`WorkloadIdentityError`] when the URI is `https://` and no
/// in-fleet identity can be obtained. The call fails rather than silently
/// retrying against the OS root store with no client certificate — a caller
/// that reached this point believes it is talking to an in-fleet peer, and
/// serving that with an anonymous transport is the failure this returns.
pub async fn connect_http_client(uri: &Uri) -> Result<HttpClient, WorkloadIdentityError> {
    if uri.scheme_str() == Some("https") {
        let tls = workload_client_tls_config().await?;
        Ok(HttpClient::with_tls(tls))
    } else {
        Ok(HttpClient::plaintext_http2_only())
    }
}

/// Build an [`HttpClient`] trusting the OS root store, presenting no client
/// certificate.
///
/// The supported path for a caller **outside** the mesh — a CLI on a laptop, a
/// test harness, a public-internet client. It is deliberately a different
/// function rather than a fallback inside [`connect_http_client`], so that
/// choosing an anonymous transport is something a caller writes down.
#[must_use]
pub fn os_trust_http_client(uri: &Uri) -> HttpClient {
    api_bones::connect::connect_http_client(uri)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(s: &str) -> Uri {
        s.parse().expect("valid test uri")
    }

    #[tokio::test]
    async fn plaintext_uri_needs_no_workload_api() {
        // http:// is an explicit caller choice and must not require an agent,
        // so this succeeds on a machine with no Workload API at all.
        let client = connect_http_client(&uri("http://registry.internal:8080"))
            .await
            .expect("cleartext transport must not depend on the Workload API");
        let debug = format!("{client:?}");
        assert!(
            !debug.contains("tls"),
            "http:// must stay cleartext, got: {debug}"
        );
    }

    #[tokio::test]
    async fn https_without_workload_api_fails_closed() {
        // No agent is running in the test environment. The contract is that
        // this reports the absence rather than downgrading to an anonymous
        // transport that would still connect.
        let err = connect_http_client(&uri("https://registry.internal"))
            .await
            .expect_err("https:// with no Workload API must not silently downgrade");

        assert!(
            matches!(err, WorkloadIdentityError::Unavailable),
            "expected a nameable unavailable error, got: {err:?}"
        );
    }

    #[test]
    fn os_trust_client_remains_available_for_outside_callers() {
        let client = os_trust_http_client(&uri("https://example.com"));
        let debug = format!("{client:?}");
        assert!(
            debug.contains("tls"),
            "the outside-the-mesh path still uses TLS, got: {debug}"
        );
    }
}
