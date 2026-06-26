// SPDX-License-Identifier: MIT
//! Client transport construction for Connect RPC services.
use std::sync::{Arc, OnceLock};

use connectrpc::ErrorCode;
use connectrpc::client::{ClientConfig, HttpClient};
use connectrpc::rustls::{ClientConfig as RustlsClientConfig, RootCertStore};
use http::Uri;

/// Build an [`HttpClient`] for a URI.
///
/// `https://` → TLS via [`client_tls_config`]; else cleartext h2c (prior knowledge).
#[must_use]
pub fn connect_http_client(uri: &Uri) -> HttpClient {
    if uri.scheme_str() == Some("https") {
        HttpClient::with_tls(client_tls_config())
    } else {
        HttpClient::plaintext_http2_only()
    }
}

/// Build a shared rustls [`RustlsClientConfig`] trusting the OS root store.
///
/// Built once via [`OnceLock`]. Honors `SSL_CERT_FILE`/`SSL_CERT_DIR`
/// (openssl-probe in `rustls-native-certs`) so dev/e2e can inject a private CA
/// via env. ALPN offers h2 then http/1.1.
#[must_use]
pub fn client_tls_config() -> Arc<RustlsClientConfig> {
    static TLS_CONFIG: OnceLock<Arc<RustlsClientConfig>> = OnceLock::new();
    TLS_CONFIG
        .get_or_init(|| {
            let mut roots = RootCertStore::empty();
            let loaded = rustls_native_certs::load_native_certs();
            for cert in loaded.certs {
                let _ = roots.add(cert);
            }
            let mut config = RustlsClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth();
            config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
            Arc::new(config)
        })
        .clone()
}

/// brefwiz Connect header conventions on connectrpc's `ClientConfig`.
pub trait ConnectConfigExt {
    /// Add `Authorization: Bearer <token>` as a default header.
    #[must_use]
    fn with_bearer(self, token: &str) -> Self;
    /// Add `x-org-id` as a default header.
    #[must_use]
    fn with_org(self, org_id: &str) -> Self;
    /// Add `x-subject-id` as a default header.
    #[must_use]
    fn with_subject(self, subject_id: &str) -> Self;
}

impl ConnectConfigExt for ClientConfig {
    fn with_bearer(self, token: &str) -> Self {
        self.with_default_header("authorization", format!("Bearer {token}"))
    }

    fn with_org(self, org_id: &str) -> Self {
        self.with_default_header("x-org-id", org_id.to_owned())
    }

    fn with_subject(self, s: &str) -> Self {
        self.with_default_header("x-subject-id", s.to_owned())
    }
}

/// Whether a Connect error code is a caller-facing rejection vs transport/server failure.
#[must_use]
pub fn is_caller_rejection(code: ErrorCode) -> bool {
    matches!(
        code,
        ErrorCode::InvalidArgument
            | ErrorCode::Unauthenticated
            | ErrorCode::PermissionDenied
            | ErrorCode::FailedPrecondition
            | ErrorCode::NotFound
            | ErrorCode::Unimplemented
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use connectrpc::ErrorCode;

    /// Install the aws-lc-rs crypto provider once per process so rustls can
    /// build TLS configs in tests. Idempotent — later calls are no-ops.
    fn install_crypto_provider() {
        let _ = connectrpc::rustls::crypto::aws_lc_rs::default_provider().install_default();
    }

    #[test]
    fn connect_http_client_https_uses_tls() {
        install_crypto_provider();
        let uri: Uri = "https://example.com".parse().unwrap();
        let client = connect_http_client(&uri);
        // Debug impl on HttpClient prints the mode
        let debug = format!("{client:?}");
        assert!(debug.contains("tls"), "expected tls mode, got: {debug}");
    }

    #[test]
    fn connect_http_client_http_uses_plaintext() {
        install_crypto_provider();
        let uri: Uri = "http://localhost:8080".parse().unwrap();
        let client = connect_http_client(&uri);
        let debug = format!("{client:?}");
        assert!(
            debug.contains("plaintext"),
            "expected plaintext mode, got: {debug}"
        );
    }

    #[test]
    fn client_tls_config_returns_same_arc() {
        install_crypto_provider();
        let a = client_tls_config();
        let b = client_tls_config();
        assert!(Arc::ptr_eq(&a, &b), "expected same Arc from OnceLock");
    }

    #[test]
    fn client_tls_config_has_h2_alpn() {
        install_crypto_provider();
        let cfg = client_tls_config();
        assert!(
            cfg.alpn_protocols.iter().any(|p| p == b"h2"),
            "h2 must be in ALPN"
        );
    }

    #[test]
    fn connect_config_ext_with_bearer() {
        let uri: Uri = "http://localhost:8080".parse().unwrap();
        let cfg = ClientConfig::new(uri).with_bearer("tok123");
        let headers = cfg.default_headers();
        let auth = headers.get("authorization").and_then(|v| v.to_str().ok());
        assert_eq!(auth, Some("Bearer tok123"));
    }

    #[test]
    fn connect_config_ext_with_org() {
        let uri: Uri = "http://localhost:8080".parse().unwrap();
        let cfg = ClientConfig::new(uri).with_org("org-abc");
        let headers = cfg.default_headers();
        let val = headers.get("x-org-id").and_then(|v| v.to_str().ok());
        assert_eq!(val, Some("org-abc"));
    }

    #[test]
    fn connect_config_ext_with_subject() {
        let uri: Uri = "http://localhost:8080".parse().unwrap();
        let cfg = ClientConfig::new(uri).with_subject("sub-xyz");
        let headers = cfg.default_headers();
        let val = headers.get("x-subject-id").and_then(|v| v.to_str().ok());
        assert_eq!(val, Some("sub-xyz"));
    }

    #[test]
    fn connect_config_ext_chained() {
        let uri: Uri = "http://localhost:8080".parse().unwrap();
        let cfg = ClientConfig::new(uri)
            .with_bearer("tok")
            .with_org("org")
            .with_subject("sub");
        let headers = cfg.default_headers();
        assert!(headers.contains_key("authorization"));
        assert!(headers.contains_key("x-org-id"));
        assert!(headers.contains_key("x-subject-id"));
    }

    #[test]
    fn is_caller_rejection_true_for_rejection_codes() {
        let rejection_codes = [
            ErrorCode::InvalidArgument,
            ErrorCode::Unauthenticated,
            ErrorCode::PermissionDenied,
            ErrorCode::FailedPrecondition,
            ErrorCode::NotFound,
            ErrorCode::Unimplemented,
        ];
        for code in rejection_codes {
            assert!(
                is_caller_rejection(code),
                "{code:?} should be a caller rejection"
            );
        }
    }

    #[test]
    fn is_caller_rejection_false_for_server_codes() {
        assert!(!is_caller_rejection(ErrorCode::Internal));
        assert!(!is_caller_rejection(ErrorCode::Unavailable));
    }
}
