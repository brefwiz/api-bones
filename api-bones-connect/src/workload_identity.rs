// SPDX-License-Identifier: MIT
//! Client transport identity acquired from the SPIFFE Workload API.
//!
//! A consumer calling an in-fleet service supplies no provider coordinates:
//! no CA path, no socket path, no trust bundle, no endpoint trust config. The
//! identity, the trust bundle and the set of servers worth talking to are all
//! derived from the SVID the Workload API already issues to this workload.
//!
//! The peer allow-list is the caller's **own trust domain**. A workload
//! holding `spiffe://prod.brefwiz/svc/bff` will talk to any
//! `spiffe://prod.brefwiz/...` server and refuse everything else. That is a
//! derivation, not a knob — nothing about it is answerable by a consumer.

use std::sync::Arc;

use brefwiz_spiffe_client::{
    SpiffeId, SpiffeIdPattern, SvidWatcher, start_watcher_safe, tls_client_config_from_watcher,
};
use connectrpc::rustls::ClientConfig as RustlsClientConfig;

/// Attempts made to reach the Workload API before reporting it absent.
const WATCHER_ATTEMPTS: u32 = 3;

/// Why a transport could not take its identity from the Workload API.
///
/// Every variant is terminal and nameable. None of them is a reason to fall
/// back to an unverified transport — the caller decides what to do, and the
/// generated transport treats absence as "serve no in-fleet traffic".
#[derive(Debug, thiserror::Error)]
pub enum WorkloadIdentityError {
    /// No Workload API answered. The workload is outside the mesh, or the
    /// agent is not running.
    #[error(
        "SPIFFE Workload API unavailable after {WATCHER_ATTEMPTS} attempts; \
         this workload has no in-fleet identity"
    )]
    Unavailable,

    /// The Workload API answered but its SVID does not name a usable trust
    /// domain, so no peer allow-list can be derived from it.
    #[error("SVID {svid_id} carries no usable trust domain")]
    UnusableTrustDomain {
        /// The SPIFFE ID that could not be reduced to a trust domain.
        svid_id: String,
    },

    /// rustls rejected the configuration built from the live SVID.
    #[error("building client TLS from the Workload API SVID: {0}")]
    Tls(#[from] connectrpc::rustls::Error),
}

/// Build a client [`RustlsClientConfig`] whose identity and trust both come
/// from the Workload API.
///
/// The returned config resolves the SVID and bundle from the running watcher on
/// every handshake, so rotation takes effect on the next connection without the
/// caller holding a second config.
///
/// # Errors
///
/// [`WorkloadIdentityError::Unavailable`] when no Workload API answers,
/// [`WorkloadIdentityError::UnusableTrustDomain`] when the SVID names no trust
/// domain, or [`WorkloadIdentityError::Tls`] when rustls rejects the result.
pub async fn workload_client_tls_config() -> Result<Arc<RustlsClientConfig>, WorkloadIdentityError>
{
    let watcher = start_watcher_safe(None, WATCHER_ATTEMPTS)
        .await
        .ok_or(WorkloadIdentityError::Unavailable)?;
    client_tls_config_for(&watcher)
}

/// Derive the peer allow-list from `watcher`'s own SVID and build the config.
///
/// Split from [`workload_client_tls_config`] so the derivation is testable
/// against a watcher built in-process, without a live agent socket.
///
/// # Errors
///
/// As [`workload_client_tls_config`], minus the unavailable case.
pub fn client_tls_config_for(
    watcher: &Arc<SvidWatcher>,
) -> Result<Arc<RustlsClientConfig>, WorkloadIdentityError> {
    let svid = watcher.current();

    // `ValidatedSvid` carries the id as a plain String, so parse it rather than
    // slicing: an id that is not a well-formed SPIFFE URI must be reported, not
    // turned into a pattern that happens to match something.
    let parsed = SpiffeId::parse(&svid.spiffe_id).map_err(|_| {
        WorkloadIdentityError::UnusableTrustDomain {
            svid_id: svid.spiffe_id.clone(),
        }
    })?;
    let trust_domain = parsed.trust_domain();

    let pattern = SpiffeIdPattern::parse(&format!("spiffe://{trust_domain}/*")).map_err(|_| {
        WorkloadIdentityError::UnusableTrustDomain {
            svid_id: svid.spiffe_id.clone(),
        }
    })?;

    let config = tls_client_config_from_watcher(watcher, &[pattern])?;
    Ok(Arc::new(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_for_unavailable_names_the_workload_api() {
        let rendered = WorkloadIdentityError::Unavailable.to_string();
        assert!(
            rendered.contains("Workload API unavailable"),
            "an absent agent must be diagnosable, got: {rendered}"
        );
        assert!(
            rendered.contains("no in-fleet identity"),
            "the message must say what the workload cannot do, got: {rendered}"
        );
    }

    #[test]
    fn error_for_unusable_trust_domain_names_the_svid() {
        let rendered = WorkloadIdentityError::UnusableTrustDomain {
            svid_id: "spiffe://example/svc".to_owned(),
        }
        .to_string();
        assert!(
            rendered.contains("spiffe://example/svc"),
            "the failing SVID must appear in the message, got: {rendered}"
        );
    }

    #[test]
    fn peer_allow_list_is_the_callers_own_trust_domain() {
        // The derivation under test: an SVID's trust domain becomes a
        // domain-wide wildcard, so a consumer never names a peer.
        let pattern = SpiffeIdPattern::parse("spiffe://prod.brefwiz/*")
            .expect("a trust-domain wildcard is a valid pattern");

        let peer = brefwiz_spiffe_client::SpiffeId::parse("spiffe://prod.brefwiz/svc/registry")
            .expect("valid peer id");
        assert!(pattern.matches(&peer), "same trust domain must be allowed");

        let foreign = brefwiz_spiffe_client::SpiffeId::parse("spiffe://other.brefwiz/svc/registry")
            .expect("valid peer id");
        assert!(
            !pattern.matches(&foreign),
            "a different trust domain must be refused"
        );
    }
}
