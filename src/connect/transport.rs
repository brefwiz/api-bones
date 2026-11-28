// SPDX-License-Identifier: MIT
//! Client TLS configuration and header conventions for Connect RPC services.
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use connectrpc::ErrorCode;
use connectrpc::client::ClientConfig;
use connectrpc::rustls::{ClientConfig as RustlsClientConfig, RootCertStore};

/// Root certificates named by `SSL_CERT_FILE` or `SSL_CERT_DIR` could not be
/// loaded.
#[derive(Debug, thiserror::Error)]
#[error("cannot load root certificates from {}: {reason}", .path.display())]
pub struct RootStoreError {
    /// File or directory the certificates were read from.
    pub path: PathBuf,
    /// What went wrong, as reported by the reader.
    pub reason: String,
}

/// The root certificates TLS server verification trusts.
///
/// With `tls-native-roots` this is the operating system store, honoring
/// `SSL_CERT_FILE`/`SSL_CERT_DIR` (`rustls-native-certs`) so dev/e2e can inject
/// a private CA via env. It wins when `tls-webpki-roots` is enabled as well.
///
/// With `tls-webpki-roots` alone it is the bundled Mozilla set, unless
/// `SSL_CERT_FILE` or `SSL_CERT_DIR` is set: then only the certificates they
/// name are trusted. `SSL_CERT_DIR` may list several directories, separated as
/// in `PATH`.
///
/// # Errors
///
/// Never fails with `tls-native-roots`. With `tls-webpki-roots` alone it fails
/// when a variable points at a missing path, at malformed PEM, or (for
/// `SSL_CERT_FILE`) at a file with no certificate.
pub fn root_store() -> Result<RootCertStore, RootStoreError> {
    // The public item is the same in every build; only the source behind it
    // depends on the enabled root feature.
    source_roots()
}

/// The operating system store, honoring `SSL_CERT_FILE`/`SSL_CERT_DIR`.
#[cfg(feature = "tls-native-roots")]
fn source_roots() -> Result<RootCertStore, RootStoreError> {
    let mut roots = RootCertStore::empty();
    roots.add_parsable_certificates(rustls_native_certs::load_native_certs().certs);
    Ok(roots)
}

/// The bundled Mozilla set, or only the certificates `SSL_CERT_FILE` or
/// `SSL_CERT_DIR` name when either is set.
#[cfg(not(feature = "tls-native-roots"))]
fn source_roots() -> Result<RootCertStore, RootStoreError> {
    super::webpki_store::root_store(&process_env)
}

#[cfg(not(feature = "tls-native-roots"))]
fn process_env(key: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(key)
}

/// Build a shared rustls [`RustlsClientConfig`] trusting [`root_store`].
///
/// Built once via [`OnceLock`]. If the root certificates cannot be loaded the
/// config trusts nothing, so every handshake fails closed, and the cause is
/// logged. ALPN offers h2 then http/1.1; `api_bones_transport::HttpTransport`
/// narrows to h2 itself.
#[must_use]
pub fn client_tls_config() -> Arc<RustlsClientConfig> {
    static TLS_CONFIG: OnceLock<Arc<RustlsClientConfig>> = OnceLock::new();
    TLS_CONFIG.get_or_init(|| config_from(root_store())).clone()
}

/// The roots rustls trusts: the loaded ones, or an empty store when loading
/// failed so that every handshake is refused.
fn trusted_roots(roots: Result<RootCertStore, RootStoreError>) -> RootCertStore {
    match roots {
        Ok(roots) => roots,
        Err(err) => {
            tracing::error!(error = %err, "no root certificates loaded; every TLS handshake will fail");
            RootCertStore::empty()
        }
    }
}

fn config_from(roots: Result<RootCertStore, RootStoreError>) -> Arc<RustlsClientConfig> {
    let roots = trusted_roots(roots);
    // The provider is named here, so building the config never depends on a
    // process-wide default a host binary may not have installed. The aws-lc-rs
    // provider supports every protocol version rustls enables by default, so
    // the version selection cannot fail.
    let provider = Arc::new(connectrpc::rustls::crypto::aws_lc_rs::default_provider());
    let mut config = RustlsClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("the aws-lc-rs provider supports rustls' default protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Arc::new(config)
}

/// brefwiz Connect header conventions on connectrpc's `ClientConfig`.
pub trait ConnectConfigExt {
    /// Set `Authorization: Bearer <token>` as a default header, replacing any
    /// prior `authorization` default header rather than appending to it.
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
        // `ClientConfig::with_default_header` appends, so calling this twice
        // (e.g. to rotate a token) would otherwise leave both `authorization`
        // values on the config; `HeaderMap::get` returns the first, so the
        // stale credential would keep being sent. Replace instead of append.
        let mut headers = self.default_headers().clone();
        if let Ok(value) = http::HeaderValue::try_from(format!("Bearer {token}")) {
            headers.insert(http::header::AUTHORIZATION, value);
        }
        self.with_default_headers(headers)
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
    use http::Uri;

    /// Install the aws-lc-rs crypto provider once per process so rustls can
    /// build TLS configs in tests. Idempotent — later calls are no-ops.
    fn install_crypto_provider() {
        let _ = connectrpc::rustls::crypto::aws_lc_rs::default_provider().install_default();
    }

    #[test]
    fn client_tls_config_returns_same_arc() {
        install_crypto_provider();
        let a = client_tls_config();
        let b = client_tls_config();
        assert!(Arc::ptr_eq(&a, &b), "expected same Arc from OnceLock");
    }

    #[cfg(feature = "tls-native-roots")]
    #[test]
    fn root_store_loads_without_error() {
        assert!(root_store().is_ok());
    }

    #[test]
    fn unloadable_roots_yield_a_config_that_trusts_nothing() {
        install_crypto_provider();
        let err = RootStoreError {
            path: PathBuf::from("/nonexistent/ca.pem"),
            reason: "gone".to_owned(),
        };
        assert_eq!(
            err.to_string(),
            "cannot load root certificates from /nonexistent/ca.pem: gone"
        );
        let roots = trusted_roots(Err(err));
        assert_eq!(roots.roots.len(), 0);
        let cfg = config_from(Err(RootStoreError {
            path: PathBuf::from("/nonexistent/ca.pem"),
            reason: "gone".to_owned(),
        }));
        assert_eq!(cfg.alpn_protocols.len(), 2);
    }

    /// `root_store()` in a webpki-only build reads the process environment.
    /// Writing the environment in-process needs `unsafe`, which this crate
    /// denies, so each case re-runs this test binary with the variables set on
    /// the child command and the child asserts through `webpki_root_store_probe`.
    #[cfg(all(feature = "tls-webpki-roots", not(feature = "tls-native-roots")))]
    mod webpki_only {
        use super::root_store;

        const PROBE: &str = "webpki_root_store_probe";
        const EXPECT: &str = "API_BONES_ROOT_STORE_EXPECT";
        const ONE_CERT: &str = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/connect/testdata/certs/one.pem"
        );

        /// Runs the probe in a child process whose `SSL_CERT_FILE` is `file`
        /// (unset when `None`) and whose `SSL_CERT_DIR` is unset.
        fn probe_in_child(expect: &str, file: Option<&str>) {
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command
                .args([PROBE, "--nocapture", "--test-threads=1"])
                .env(EXPECT, expect)
                .env_remove("SSL_CERT_FILE")
                .env_remove("SSL_CERT_DIR");
            if let Some(file) = file {
                command.env("SSL_CERT_FILE", file);
            }
            let output = command.output().unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                stdout.contains("1 passed"),
                "probe did not run: {stdout}{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(output.status.success(), "probe failed: {stdout}");
        }

        #[test]
        fn cert_file_is_the_only_trust_source() {
            probe_in_child("one", Some(ONE_CERT));
        }

        #[test]
        fn neither_variable_trusts_the_bundled_roots() {
            probe_in_child("bundled", None);
        }

        /// Child half of the cases above; a no-op when run on its own.
        #[test]
        fn webpki_root_store_probe() {
            let Ok(expect) = std::env::var(EXPECT) else {
                return;
            };
            let store = root_store().unwrap();
            match expect.as_str() {
                "one" => assert_eq!(store.roots.len(), 1),
                _ => assert_ne!(store.roots.len(), 0),
            }
        }
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
    fn connect_config_ext_with_bearer_twice_replaces_not_appends() {
        let uri: Uri = "http://localhost:8080".parse().unwrap();
        let cfg = ClientConfig::new(uri)
            .with_bearer("first")
            .with_bearer("second");
        let headers = cfg.default_headers();
        assert_eq!(headers.get_all("authorization").iter().count(), 1);
        let auth = headers.get("authorization").and_then(|v| v.to_str().ok());
        assert_eq!(auth, Some("Bearer second"));
    }

    #[test]
    fn connect_config_ext_with_bearer_replaces_preexisting_authorization() {
        let uri: Uri = "http://localhost:8080".parse().unwrap();
        let cfg = ClientConfig::new(uri)
            .with_default_header("authorization", "Bearer stale")
            .with_bearer("fresh");
        let headers = cfg.default_headers();
        assert_eq!(headers.get_all("authorization").iter().count(), 1);
        let auth = headers.get("authorization").and_then(|v| v.to_str().ok());
        assert_eq!(auth, Some("Bearer fresh"));
    }

    #[test]
    fn connect_config_ext_with_bearer_preserves_unrelated_headers() {
        let uri: Uri = "http://localhost:8080".parse().unwrap();
        let cfg = ClientConfig::new(uri)
            .with_org("org-abc")
            .with_subject("sub-xyz")
            .with_bearer("tok123");
        let headers = cfg.default_headers();
        let auth = headers.get("authorization").and_then(|v| v.to_str().ok());
        assert_eq!(auth, Some("Bearer tok123"));
        let org = headers.get("x-org-id").and_then(|v| v.to_str().ok());
        assert_eq!(org, Some("org-abc"));
        let subject = headers.get("x-subject-id").and_then(|v| v.to_str().ok());
        assert_eq!(subject, Some("sub-xyz"));
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
