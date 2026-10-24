// SPDX-License-Identifier: MIT
//! Pinned-peer client TLS for callers that hold no SVID.
//!
//! A caller outside the mesh's identity system, such as a post-deploy job with
//! no client certificate, reaches one SPIFFE peer by pinning its exact ID
//! against a committed trust bundle. Everything that decides whether a server
//! is that peer (the bundle's shape, the chain, the X509-SVID leaf rules and
//! the ID comparison) is the SPIFFE client crate's
//! [`SpiffeServerVerifier::pinned_peer`]; this module only composes it into a
//! client config that sends no client certificate.
//!
//! A refusal at connection time is read from the connection error with
//! [`tls_verdict`] and [`spiffe_refusal`], which this module re-exports so the
//! verdict is classified by the owner and never by a second reading of the
//! error here.

use std::sync::Arc;

use connectrpc::client::HttpClient;
use connectrpc::rustls::ClientConfig as RustlsClientConfig;
use quorumauth_spiffe_client::SpiffeServerVerifier;
pub use quorumauth_spiffe_client::{SpiffeError, spiffe_refusal, tls_verdict};

/// Why a pinned-peer client could not be built.
///
/// Raised before any connection is attempted; the reasons are
/// the SPIFFE client crate's own construction refusals.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PinnedPeerError {
    /// The peer to pin is not an exact SPIFFE ID.
    #[error("pinned peer is not an exact SPIFFE ID: {reason}")]
    InvalidPeerId {
        /// Why the verifier refused the peer.
        reason: String,
    },

    /// The trust bundle is not a non-empty set of CA certificates.
    #[error("pinned-peer trust bundle is unusable: {reason}")]
    InvalidTrustBundle {
        /// What was wrong with the bundle.
        reason: String,
    },

    /// rustls rejected the client configuration.
    #[error("building pinned-peer client TLS: {0}")]
    Tls(#[from] connectrpc::rustls::Error),

    /// Any other refusal from the SPIFFE verifier's construction.
    #[error("pinned-peer verifier refused: {0}")]
    Spiffe(SpiffeError),
}

impl From<SpiffeError> for PinnedPeerError {
    fn from(error: SpiffeError) -> Self {
        match error {
            SpiffeError::InvalidPeerId(reason) => Self::InvalidPeerId { reason },
            SpiffeError::InvalidTrustBundle(reason) => Self::InvalidTrustBundle { reason },
            other => Self::Spiffe(other),
        }
    }
}

/// Build a client [`RustlsClientConfig`] for a caller that holds **no SVID**.
///
/// The server is verified by [`SpiffeServerVerifier::pinned_peer`] against
/// `bundle_pem` and `peer`. No client certificate is ever sent and the
/// Workload API is never touched.
///
/// # Errors
///
/// [`PinnedPeerError::InvalidPeerId`] when `peer` is not an exact SPIFFE ID,
/// [`PinnedPeerError::InvalidTrustBundle`] when `bundle_pem` is not a non-empty
/// set of CA certificates, or [`PinnedPeerError::Tls`] when rustls rejects the
/// result.
pub fn pinned_peer_client_tls_config(
    bundle_pem: &[u8],
    peer: &str,
) -> Result<RustlsClientConfig, PinnedPeerError> {
    let bundle =
        std::str::from_utf8(bundle_pem).map_err(|e| PinnedPeerError::InvalidTrustBundle {
            reason: e.to_string(),
        })?;
    let verifier = SpiffeServerVerifier::pinned_peer(bundle, peer)?;

    let provider = Arc::new(connectrpc::rustls::crypto::aws_lc_rs::default_provider());
    Ok(RustlsClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth())
}

/// Build an [`HttpClient`] that reaches one SPIFFE peer without holding an SVID.
///
/// # Errors
///
/// [`PinnedPeerError`], as [`pinned_peer_client_tls_config`].
pub fn pinned_peer_http_client(
    bundle_pem: &[u8],
    peer: &str,
) -> Result<HttpClient, PinnedPeerError> {
    let tls = pinned_peer_client_tls_config(bundle_pem, peer)?;
    Ok(HttpClient::with_tls(Arc::new(tls)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PEER: &str = "spiffe://prod.example/svc/signer";

    fn ca_pem() -> String {
        let key = rcgen::KeyPair::generate().expect("key");
        let mut params = rcgen::CertificateParams::new(Vec::new()).expect("params");
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        params.self_signed(&key).expect("ca").pem()
    }

    #[test]
    fn a_valid_bundle_and_peer_build_a_tls_client() {
        let _ = connectrpc::rustls::crypto::aws_lc_rs::default_provider().install_default();
        let client = pinned_peer_http_client(ca_pem().as_bytes(), PEER).expect("client");
        assert!(format!("{client:?}").contains("tls"));
    }

    #[test]
    fn construction_refusals_map_from_the_verifier() {
        let bundle = ca_pem();
        assert!(matches!(
            pinned_peer_client_tls_config(bundle.as_bytes(), "spiffe://prod.example/svc/*"),
            Err(PinnedPeerError::InvalidPeerId { .. })
        ));
        assert!(matches!(
            pinned_peer_client_tls_config(b"", PEER),
            Err(PinnedPeerError::InvalidTrustBundle { .. })
        ));
        assert!(matches!(
            pinned_peer_client_tls_config(&[0xff, 0xfe], PEER),
            Err(PinnedPeerError::InvalidTrustBundle { .. })
        ));
    }
}
