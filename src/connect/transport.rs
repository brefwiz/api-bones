// SPDX-License-Identifier: MIT
//! Client transport construction for Connect RPC services.
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use connectrpc::ErrorCode;
use connectrpc::client::{ClientConfig, HttpClient, HttpClientBuilder};
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

/// Bound on establishing a connection to a peer inside the fleet (DNS + TCP + TLS).
///
/// In-cluster establishment takes single-digit milliseconds (failing attempts
/// complete in 9-51 ms on a loaded CI cluster), and a lost SYN is not
/// retransmitted before Linux's 1 s initial RTO, so a connect not done by then
/// cannot finish within a 1 s RPC budget anyway. 250 ms is about five times the
/// slowest observed establishment and leaves 750 ms of a 1 s budget for the
/// response.
pub const FLEET_CONNECT_BOUND: Duration = Duration::from_millis(250);

/// An [`HttpClientBuilder`] whose TCP connect and connection establishment are
/// both bounded by [`FLEET_CONNECT_BOUND`].
pub fn fleet_client_builder() -> HttpClientBuilder {
    HttpClient::builder()
        .tcp_connect_timeout(FLEET_CONNECT_BOUND)
        .establishment_timeout(FLEET_CONNECT_BOUND)
}

/// Build the [`HttpClient`] for a channel the workload's own identity opens to
/// a fleet peer (mTLS, SPIFFE-pinned), with connection establishment bounded by
/// [`FLEET_CONNECT_BOUND`].
///
/// Public endpoints keep connectrpc's defaults; use [`connect_http_client`] for
/// those.
#[must_use]
pub fn fleet_http_client(tls: Arc<RustlsClientConfig>) -> HttpClient {
    fleet_client_builder().with_tls(tls)
}

/// Whether `error` is a failure to establish the connection (refused,
/// unreachable, TLS/SPIFFE peer verification failed, or establishment timeout)
/// rather than a failure after the request was on the wire.
///
/// True when the `source()` chain holds a connect-phase
/// `hyper_util::client::legacy::Error`, or a `ConnectError` with code
/// `Unavailable` caused by a `tokio::time::error::Elapsed` (the establishment
/// timeout). A true result is also returned when the TLS/SPIFFE peer check
/// fails, so it does not mean the peer is merely still booting: callers must
/// not treat it as grounds to retry without bound or to downgrade security.
#[must_use]
pub fn failed_to_connect(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(link) = current {
        if link
            .downcast_ref::<hyper_util::client::legacy::Error>()
            .is_some_and(hyper_util::client::legacy::Error::is_connect)
        {
            return true;
        }
        if link
            .downcast_ref::<connectrpc::ConnectError>()
            .is_some_and(|e| e.code == ErrorCode::Unavailable && has_elapsed_source(link))
        {
            return true;
        }
        current = link.source();
    }
    false
}

fn has_elapsed_source(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut current = error.source();
    while let Some(link) = current {
        if link.is::<tokio::time::error::Elapsed>() {
            return true;
        }
        current = link.source();
    }
    false
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

    async fn probe(client: &HttpClient, base: &str) -> Result<(), connectrpc::ConnectError> {
        use buffa_types::google::protobuf::{Empty, EmptyView};
        use connectrpc::client::call_unary;
        use connectrpc::{Spec, StreamType};
        let config = ClientConfig::new(base.parse().expect("uri"));
        call_unary::<_, Empty, EmptyView>(
            client,
            &config,
            Spec::client("/probe.v1.Probe/Ping", StreamType::Unary),
            Empty::default(),
            connectrpc::client::CallOptions::default(),
        )
        .await
        .map(|_| ())
    }

    #[tokio::test]
    async fn fleet_client_bounds_a_handshake_that_never_answers() {
        install_crypto_provider();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        // Accepts and holds the socket open without writing: the TLS handshake hangs.
        let _held = tokio::spawn(async move {
            let mut sockets = Vec::new();
            while let Ok((socket, _)) = listener.accept().await {
                sockets.push(socket);
            }
            drop(sockets);
        });
        let client = fleet_http_client(client_tls_config());

        let started = std::time::Instant::now();
        let err = probe(&client, &format!("https://127.0.0.1:{port}"))
            .await
            .expect_err("a handshake that never completes must fail");
        let elapsed = started.elapsed();

        assert!(
            elapsed >= FLEET_CONNECT_BOUND.saturating_sub(Duration::from_millis(50)),
            "failed before the bound: {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_secs(1),
            "not bounded within 1 s: {elapsed:?}"
        );
        assert!(failed_to_connect(&err), "not a connect failure: {err:?}");
    }

    #[tokio::test]
    async fn fleet_client_fails_fast_when_the_port_refuses() {
        install_crypto_provider();
        let port = {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            listener.local_addr().unwrap().port()
        };
        let client = fleet_http_client(client_tls_config());

        let started = std::time::Instant::now();
        let err = probe(&client, &format!("https://127.0.0.1:{port}"))
            .await
            .expect_err("a refused connection must fail");

        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(failed_to_connect(&err), "not a connect failure: {err:?}");
    }

    #[test]
    fn failed_to_connect_true_for_establishment_timeout_chain() {
        fn elapsed() -> tokio::time::error::Elapsed {
            tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .unwrap()
                .block_on(async {
                    tokio::time::timeout(Duration::from_millis(1), std::future::pending::<()>())
                        .await
                        .unwrap_err()
                })
        }
        let err =
            connectrpc::ConnectError::unavailable("establishment timed out").with_source(elapsed());
        assert!(failed_to_connect(&err));
        let wrong_code = connectrpc::ConnectError::internal("x").with_source(elapsed());
        assert!(!failed_to_connect(&wrong_code));
    }

    #[tokio::test]
    async fn failed_to_connect_true_for_refused_connection() {
        install_crypto_provider();
        let port = {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            listener.local_addr().unwrap().port()
        };
        let client = fleet_http_client(client_tls_config());
        let err = probe(&client, &format!("https://127.0.0.1:{port}"))
            .await
            .expect_err("refused");
        assert!(failed_to_connect(&err), "refused not recognised: {err:?}");
    }

    #[test]
    fn failed_to_connect_is_false_for_non_connect_errors() {
        let plain = connectrpc::ConnectError::unavailable("x");
        assert!(!failed_to_connect(&plain));
        let io = std::io::Error::other("boom");
        assert!(!failed_to_connect(&io));
    }

    #[test]
    fn fleet_client_builder_builds_both_modes() {
        install_crypto_provider();
        let tls = format!("{:?}", fleet_http_client(client_tls_config()));
        assert!(tls.contains("tls"), "got: {tls}");
        let plain = format!("{:?}", fleet_client_builder().plaintext_http2_only());
        assert!(plain.contains("plaintext"), "got: {plain}");
    }
}
