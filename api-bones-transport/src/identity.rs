// SPDX-License-Identifier: MIT
//! Workload identity policy: the one rule native and wasm share.
//!
//! Nothing here touches a socket or a host. It decides which URLs are
//! in-fleet, which SPIFFE ID a peer must present, whether a presented chain
//! names that peer, and how the SPIFFE Workload API's `FetchX509SVID` stream is
//! framed and read. The wasm transport (`node`) drives these functions over
//! the host's byte pipe; the same functions and the same tests run natively.
//!
//! The chain itself is verified by the host's TLS stack against the bundle this
//! module hands over. Only what a TLS stack cannot know is decided here: that
//! the leaf carries exactly one URI SAN and that it is the expected peer's ID.

use api_bones::connect::{ErrorInfo, with_error_info};
use connectrpc::ConnectError;
use x509_parser::extensions::GeneralName;
use x509_parser::prelude::{FromDer, X509Certificate};

/// The emitter of a refusal the transport makes itself.
const EMITTER: &str = "bones";
/// An in-fleet call whose Workload API did not answer.
pub const WORKLOAD_IDENTITY_UNAVAILABLE: &str = "WORKLOAD_IDENTITY_UNAVAILABLE";
/// The caller's SVID does not end in `/svc/<workload>`, so no peer follows.
pub const UNUSABLE_PEER_IDENTITY: &str = "UNUSABLE_PEER_IDENTITY";
/// The server did not present the peer's identity.
pub const PEER_IDENTITY_REFUSED: &str = "PEER_IDENTITY_REFUSED";
/// An in-fleet URL that is not `https:`.
pub const INSECURE_IN_FLEET: &str = "INSECURE_IN_FLEET";

/// Why a connection was refused before or while it was made. None of these
/// falls back to another lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// In-fleet, and no Workload API answered.
    WorkloadIdentityUnavailable,
    /// The SVID names no `/svc/<workload>`.
    UnusablePeerIdentity,
    /// The presented chain does not name the expected peer.
    PeerIdentityRefused,
    /// In-fleet over a scheme other than `https`.
    InsecureInFleet,
}

impl Refusal {
    /// The catalog code of the refusal.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::WorkloadIdentityUnavailable => WORKLOAD_IDENTITY_UNAVAILABLE,
            Self::UnusablePeerIdentity => UNUSABLE_PEER_IDENTITY,
            Self::PeerIdentityRefused => PEER_IDENTITY_REFUSED,
            Self::InsecureInFleet => INSECURE_IN_FLEET,
        }
    }

    /// The refusal as the Connect error a call surfaces: `unavailable` for an
    /// absent identity or a refused peer, `failed_precondition` otherwise.
    #[must_use]
    pub fn into_error(self, detail: &str) -> ConnectError {
        let message = format!("{}: {detail}", self.code());
        let error = match self {
            Self::WorkloadIdentityUnavailable | Self::PeerIdentityRefused => {
                ConnectError::unavailable(message)
            }
            Self::UnusablePeerIdentity | Self::InsecureInFleet => {
                ConnectError::failed_precondition(message)
            }
        };
        let mut info = ErrorInfo::from_code(self.code());
        EMITTER.clone_into(&mut info.emitter);
        with_error_info(error, &info)
    }
}

/// The workload a URL host names when it is in-fleet: a single DNS label,
/// `<label>.<ns>.svc` or `<label>.<ns>.svc.cluster.local`. Any other host is
/// public.
#[must_use]
pub fn in_fleet_label(host: &str) -> Option<&str> {
    let host = host.trim_end_matches('.');
    let mut parts = host.split('.');
    let label = parts.next().filter(|label| is_dns_label(label))?;
    let rest: Vec<&str> = parts.collect();
    match rest.as_slice() {
        [] => Some(label),
        [ns, "svc"] | [ns, "svc", "cluster", "local"] if is_dns_label(ns) => Some(label),
        _ => None,
    }
}

fn is_dns_label(label: &str) -> bool {
    let bytes = label.as_bytes();
    (1..=63).contains(&bytes.len())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
        && bytes[0] != b'-'
        && bytes[bytes.len() - 1] != b'-'
}

/// The peer a caller holding `own` must reach for the workload `label`: the
/// trailing `/svc/<self>` becomes `/svc/<label>`.
///
/// # Errors
///
/// [`Refusal::UnusablePeerIdentity`] when `own` is not `spiffe://<td>[/path]/svc/<name>`.
pub fn peer_spiffe_id(own: &str, label: &str) -> Result<String, Refusal> {
    let rest = own
        .strip_prefix("spiffe://")
        .ok_or(Refusal::UnusablePeerIdentity)?;
    let (head, name) = rest.rsplit_once('/').ok_or(Refusal::UnusablePeerIdentity)?;
    let (prefix, marker) = head.rsplit_once('/').ok_or(Refusal::UnusablePeerIdentity)?;
    if marker != "svc" || name.is_empty() || prefix.is_empty() || !is_dns_label(label) {
        return Err(Refusal::UnusablePeerIdentity);
    }
    Ok(format!("spiffe://{prefix}/svc/{label}"))
}

/// Admits a presented chain (leaf first, DER) when its leaf carries exactly one
/// URI SAN, that SAN equals `expected`, and the leaf is not a CA. Chain
/// validity is the host TLS stack's, against the bundle of the caller's own
/// trust domain.
///
/// # Errors
///
/// [`Refusal::PeerIdentityRefused`] for an empty or unparsable chain, no URI SAN,
/// several URI SANs, one that differs from `expected`, or a leaf whose
/// basicConstraints are malformed or say `CA:TRUE`.
pub fn admits(chain: &[Vec<u8>], expected: &str) -> Result<(), Refusal> {
    let leaf = chain.first().ok_or(Refusal::PeerIdentityRefused)?;
    let (_, cert) = X509Certificate::from_der(leaf).map_err(|_| Refusal::PeerIdentityRefused)?;
    let san = cert
        .subject_alternative_name()
        .map_err(|_| Refusal::PeerIdentityRefused)?
        .ok_or(Refusal::PeerIdentityRefused)?;
    let mut uris = san
        .value
        .general_names
        .iter()
        .filter_map(|name| match name {
            GeneralName::URI(uri) => Some(*uri),
            _ => None,
        });
    if !matches!((uris.next(), uris.next()), (Some(uri), None) if uri == expected) {
        return Err(Refusal::PeerIdentityRefused);
    }
    // A leaf may say CA:FALSE or say nothing; CA:TRUE or a malformed
    // extension is refused.
    cert.basic_constraints()
        .is_ok_and(|constraints| constraints.is_none_or(|found| !found.value.ca))
        .then_some(())
        .ok_or(Refusal::PeerIdentityRefused)
}

/// How a URL is reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Profile {
    /// A workload of the fleet: mutual TLS with the caller's SVID, the caller's
    /// own trust bundle, and the peer `<own prefix>/svc/<label>`.
    InFleet {
        /// The workload the host names.
        label: String,
    },
    /// Any other host: TLS with the host's default roots and no client
    /// certificate, so an SVID never reaches a host outside the fleet.
    Public,
}

/// The profile of a URL with `scheme` and `host`. Host names are matched
/// without regard to case.
///
/// # Errors
///
/// [`Refusal::InsecureInFleet`] for an in-fleet host over a scheme other than
/// `https`.
pub fn profile_of(scheme: &str, host: &str) -> Result<Profile, Refusal> {
    match in_fleet_label(&host.to_ascii_lowercase()) {
        Some(_) if scheme != "https" => Err(Refusal::InsecureInFleet),
        Some(label) => Ok(Profile::InFleet {
            label: label.to_owned(),
        }),
        None => Ok(Profile::Public),
    }
}

/// The path of the unix socket a `SPIFFE_ENDPOINT_SOCKET` value names
/// (`unix:///path` or `unix:/path`), or `None` for any other form.
#[must_use]
pub fn workload_socket(endpoint: &str) -> Option<&str> {
    let path = endpoint
        .strip_prefix("unix://")
        .or_else(|| endpoint.strip_prefix("unix:"))?;
    path.starts_with('/').then_some(path)
}

/// The globals a host defines when it is not Node although it has a `process`:
/// a browser's window and worker scope, and Deno and Bun.
pub const NOT_NODE_GLOBALS: [&str; 4] = ["window", "WorkerGlobalScope", "Deno", "Bun"];

/// Whether a host is Node: it reports a Node version and defines none of
/// [`NOT_NODE_GLOBALS`]. `defined` answers whether the host has a global.
#[must_use]
pub fn is_node_host(defined: impl Fn(&str) -> bool, node_version: bool) -> bool {
    node_version && !NOT_NODE_GLOBALS.iter().any(|name| defined(name))
}

/// Whether a host is a browser Window or Worker.
#[must_use]
pub fn is_browser_host(defined: impl Fn(&str) -> bool) -> bool {
    ["window", "WorkerGlobalScope"]
        .iter()
        .any(|name| defined(name))
}

/// Whether a call on the `fetch` lane must be refused when it names a workload
/// of the fleet: on any host that reports a Node version (a Node process that
/// defines `window`, as jsdom and happy-dom do, is still Node and has no
/// workload identity on this lane) and on any host that is not a browser
/// Window or Worker. `defined` answers whether the host has a global.
#[must_use]
pub fn fetch_lane_refuses_fleet(node_version: bool, defined: impl Fn(&str) -> bool) -> bool {
    node_version || !is_browser_host(defined)
}

/// Refuses an in-fleet URL on a host that can only `fetch` (Deno, Bun and any
/// other non-browser host without the Node exchange), before any request is
/// sent: a call to the fleet needs the workload identity only Node carries.
///
/// # Errors
///
/// `WORKLOAD_IDENTITY_UNAVAILABLE` for an in-fleet host, `INSECURE_IN_FLEET`
/// for an in-fleet host over a scheme other than `https`.
pub fn refuse_without_identity(scheme: &str, host: &str) -> Result<(), ConnectError> {
    match profile_of(scheme, host) {
        Ok(Profile::InFleet { .. }) => Err(Refusal::WorkloadIdentityUnavailable.into_error(
            &format!("{host} is a workload of the fleet and this host has no workload identity"),
        )),
        Ok(Profile::Public) => Ok(()),
        Err(refusal) => Err(refusal.into_error(&format!(
            "{host} is a workload of the fleet and needs https"
        ))),
    }
}

/// The owner id of a module instance: lower-case hex of its random bytes.
#[must_use]
pub fn owner_id(random: &[u8]) -> String {
    use std::fmt::Write as _;
    random.iter().fold(String::new(), |mut hex, byte| {
        let _written = write!(hex, "{byte:02x}");
        hex
    })
}

/// Whether a host TLS error `code` says the presented chain did not verify
/// against the trust the connection was given.
#[must_use]
pub fn is_verification_failure(code: &str) -> bool {
    code.starts_with("ERR_TLS_CERT")
        || matches!(
            code,
            "UNABLE_TO_VERIFY_LEAF_SIGNATURE"
                | "UNABLE_TO_GET_ISSUER_CERT"
                | "UNABLE_TO_GET_ISSUER_CERT_LOCALLY"
                | "SELF_SIGNED_CERT_IN_CHAIN"
                | "DEPTH_ZERO_SELF_SIGNED_CERT"
                | "CERT_HAS_EXPIRED"
                | "CERT_NOT_YET_VALID"
                | "CERT_UNTRUSTED"
                | "CERT_REVOKED"
                | "CERT_SIGNATURE_FAILURE"
        )
}

/// One X509-SVID of a Workload API response, as PEM for the host's TLS stack.
/// Its `Debug` output never carries the private key.
#[derive(Clone, PartialEq, Eq)]
pub struct Svid {
    /// The SPIFFE ID the certificate carries.
    pub spiffe_id: String,
    /// The certificate chain, leaf first.
    pub cert_pem: String,
    /// The private key.
    pub key_pem: String,
    /// The trust bundle of the SVID's own trust domain.
    pub bundle_pem: String,
}

impl std::fmt::Debug for Svid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Svid")
            .field("spiffe_id", &self.spiffe_id)
            .field("cert_pem", &self.cert_pem)
            .field("key_pem", &"<redacted>")
            .field("bundle_pem", &self.bundle_pem)
            .finish()
    }
}

/// The largest gRPC message the Workload API reader accepts (gRPC's default
/// receive limit); a larger frame ends the stream, so the buffer of a stream
/// that announces one stays bounded.
pub const MAX_FRAME: usize = 4 << 20;

/// Opens the gRPC frames of `buffer`: the complete messages in order and the
/// bytes of a frame not yet complete.
///
/// # Errors
///
/// A frame whose compression flag is set (the Workload API sends none), or
/// whose announced length exceeds [`MAX_FRAME`].
pub fn grpc_frames(buffer: &[u8]) -> Result<(Vec<&[u8]>, &[u8]), String> {
    let mut messages = Vec::new();
    let mut rest = buffer;
    while rest.len() >= 5 {
        if rest[0] != 0 {
            return Err("a compressed gRPC frame".to_owned());
        }
        let len = u32::from_be_bytes([rest[1], rest[2], rest[3], rest[4]]) as usize;
        if len > MAX_FRAME {
            return Err(format!("a gRPC frame of {len} bytes exceeds {MAX_FRAME}"));
        }
        let end = 5usize
            .checked_add(len)
            .ok_or("a gRPC frame length overflows")?;
        if rest.len() < end {
            break;
        }
        messages.push(&rest[5..end]);
        rest = &rest[end..];
    }
    Ok((messages, rest))
}

/// The gRPC frame of an empty `X509SVIDRequest`.
#[must_use]
pub const fn fetch_request_frame() -> [u8; 5] {
    [0, 0, 0, 0, 0]
}

/// Reads the first X509-SVID of an `X509SVIDResponse` message.
///
/// # Errors
///
/// A malformed message, or one with no SVID.
pub fn first_svid(message: &[u8]) -> Result<Svid, String> {
    let body = all_fields(message)?
        .into_iter()
        .find_map(|(number, value)| (number == 1).then_some(value))
        .ok_or("no X509-SVID in the response")?;
    let (mut id, mut cert, mut key, mut bundle) = (None, None, None, None);
    for (number, value) in all_fields(body)? {
        match number {
            1 => id = Some(String::from_utf8(value.to_vec()).map_err(|e| e.to_string())?),
            2 => cert = Some(value),
            3 => key = Some(value),
            4 => bundle = Some(value),
            _ => {}
        }
    }
    let (Some(id), Some(cert), Some(key), Some(bundle)) = (id, cert, key, bundle) else {
        return Err("an X509-SVID without every field".to_owned());
    };
    Ok(Svid {
        spiffe_id: id,
        cert_pem: pem_chain("CERTIFICATE", cert)?,
        key_pem: pem("PRIVATE KEY", key),
        bundle_pem: pem_chain("CERTIFICATE", bundle)?,
    })
}

/// The length-delimited fields of a protobuf message, `(number, bytes)`.
fn all_fields(mut buf: &[u8]) -> Result<Vec<(u64, &[u8])>, String> {
    let mut out = Vec::new();
    while !buf.is_empty() {
        let (tag, n) = varint(buf)?;
        buf = &buf[n..];
        match tag & 7 {
            0 => buf = &buf[varint(buf)?.1..],
            2 => {
                let (len, n) = varint(buf)?;
                let len = usize::try_from(len).map_err(|e| e.to_string())?;
                let end = n
                    .checked_add(len)
                    .filter(|end| *end <= buf.len())
                    .ok_or("a truncated field")?;
                out.push((tag >> 3, &buf[n..end]));
                buf = &buf[end..];
            }
            other => return Err(format!("unsupported wire type {other}")),
        }
    }
    Ok(out)
}

fn varint(buf: &[u8]) -> Result<(u64, usize), String> {
    let mut value = 0u64;
    for (i, byte) in buf.iter().enumerate().take(10) {
        value |= u64::from(byte & 0x7f) << (7 * i);
        if byte & 0x80 == 0 {
            return Ok((value, i + 1));
        }
    }
    Err("a bad varint".to_owned())
}

/// Splits concatenated DER certificates and writes each as a PEM block.
fn pem_chain(label: &str, mut der: &[u8]) -> Result<String, String> {
    let mut out = String::new();
    while !der.is_empty() {
        let header = *der.get(1).ok_or("a truncated certificate")?;
        let (len, skip) = if header & 0x80 == 0 {
            (usize::from(header), 2)
        } else {
            let count = usize::from(header & 0x7f);
            let bytes = der.get(2..2 + count).ok_or("a truncated certificate")?;
            (
                bytes
                    .iter()
                    .fold(0usize, |acc, b| (acc << 8) | usize::from(*b)),
                2 + count,
            )
        };
        let end = skip
            .checked_add(len)
            .filter(|end| *end <= der.len())
            .ok_or("a truncated certificate")?;
        out.push_str(&pem(label, &der[..end]));
        der = &der[end..];
    }
    if out.is_empty() {
        Err("an empty certificate set".to_owned())
    } else {
        Ok(out)
    }
}

fn pem(label: &str, der: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::new();
    for chunk in der.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                text.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                text.push('=');
            }
        }
    }
    let lines: Vec<&str> = text
        .as_bytes()
        .chunks(64)
        .map(|c| std::str::from_utf8(c).unwrap_or_default())
        .collect();
    format!(
        "-----BEGIN {label}-----\n{}\n-----END {label}-----\n",
        lines.join("\n")
    )
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    fn leaf(sans: &[&str]) -> Vec<u8> {
        leaf_of(sans, &[], rcgen::IsCa::NoCa)
    }

    fn leaf_of(uris: &[&str], dns: &[&str], ca: rcgen::IsCa) -> Vec<u8> {
        let key = rcgen::KeyPair::generate().expect("key");
        let mut params = rcgen::CertificateParams::new(Vec::new()).expect("params");
        params.subject_alt_names = dns
            .iter()
            .map(|name| rcgen::SanType::DnsName((*name).try_into().expect("dns")))
            .chain(
                uris.iter()
                    .map(|uri| rcgen::SanType::URI((*uri).try_into().expect("uri"))),
            )
            .collect();
        params.is_ca = ca;
        params.self_signed(&key).expect("cert").der().to_vec()
    }

    #[test]
    fn host_classes() {
        assert_eq!(in_fleet_label("billing"), Some("billing"));
        assert_eq!(in_fleet_label("billing.prod.svc"), Some("billing"));
        assert_eq!(
            in_fleet_label("billing.prod.svc.cluster.local"),
            Some("billing")
        );
        assert_eq!(in_fleet_label("api.example.com"), None);
        assert_eq!(in_fleet_label("billing.prod.svc.evil.com"), None);
        assert_eq!(in_fleet_label("Billing"), None);
        assert_eq!(in_fleet_label(""), None);
        assert_eq!(in_fleet_label("billing."), Some("billing"));
        assert_eq!(in_fleet_label("billing.prod.svc."), Some("billing"));
        let longest = "a".repeat(63);
        assert_eq!(in_fleet_label(&longest), Some(longest.as_str()));
        assert_eq!(in_fleet_label(&"a".repeat(64)), None);
        assert_eq!(in_fleet_label("-billing"), None);
        assert_eq!(in_fleet_label("billing-"), None);
        assert_eq!(in_fleet_label("billing.-ns.svc"), None);
        assert_eq!(in_fleet_label("bill_ing"), None);
    }

    #[test]
    fn peer_follows_own_prefix() {
        assert_eq!(
            peer_spiffe_id("spiffe://prod/org/acme/svc/bff", "billing").as_deref(),
            Ok("spiffe://prod/org/acme/svc/billing")
        );
        for own in [
            "spiffe://prod/bff",
            "https://prod/svc/bff",
            "spiffe://prod/svc/",
            "spiffe://prod",
        ] {
            assert_eq!(
                peer_spiffe_id(own, "billing"),
                Err(Refusal::UnusablePeerIdentity),
                "{own}"
            );
        }
        assert_eq!(
            peer_spiffe_id("spiffe://prod/svc/bff", "Bad_Label"),
            Err(Refusal::UnusablePeerIdentity)
        );
    }

    #[test]
    fn only_the_exact_single_uri_san_is_admitted() {
        let want = "spiffe://prod/svc/billing";
        assert_eq!(admits(&[leaf(&[want])], want), Ok(()));
        assert_eq!(
            admits(&[leaf(&["spiffe://prod/svc/other"])], want),
            Err(Refusal::PeerIdentityRefused)
        );
        assert_eq!(
            admits(&[leaf(&[want, "spiffe://prod/svc/other"])], want),
            Err(Refusal::PeerIdentityRefused)
        );
        assert_eq!(
            admits(&[leaf(&[])], want),
            Err(Refusal::PeerIdentityRefused)
        );
        assert_eq!(admits(&[], want), Err(Refusal::PeerIdentityRefused));
        assert_eq!(
            admits(&[vec![1, 2, 3]], want),
            Err(Refusal::PeerIdentityRefused)
        );
    }

    #[test]
    fn a_host_is_node_only_with_a_node_version_and_no_other_runtime_global() {
        let none = |_: &str| false;
        assert!(is_node_host(none, true));
        assert!(!is_node_host(none, false));
        for global in NOT_NODE_GLOBALS {
            assert!(!is_node_host(|name| name == global, true), "{global}");
        }
        assert!(is_node_host(|name| name == "process", true));
    }

    #[test]
    fn only_a_window_or_worker_is_a_browser_host() {
        assert!(is_browser_host(|name| name == "window"));
        assert!(is_browser_host(|name| name == "WorkerGlobalScope"));
        assert!(!is_browser_host(|name| name == "Deno" || name == "Bun"));
        assert!(!is_browser_host(|_| false));
    }

    #[test]
    fn a_fetch_only_host_is_refused_the_fleet_and_kept_the_public_net() {
        let refused = refuse_without_identity("https", "billing.prod.svc").expect_err("fleet");
        assert_eq!(refused.code, connectrpc::ErrorCode::Unavailable);
        assert_eq!(
            api_bones::connect::error_info(&refused).expect("info").code,
            WORKLOAD_IDENTITY_UNAVAILABLE
        );
        let insecure = refuse_without_identity("http", "billing").expect_err("plain");
        assert_eq!(
            api_bones::connect::error_info(&insecure)
                .expect("info")
                .code,
            INSECURE_IN_FLEET
        );
        assert!(refuse_without_identity("https", "api.example.com").is_ok());
        assert!(refuse_without_identity("http", "api.example.com").is_ok());
    }

    #[test]
    fn an_owner_id_is_the_hex_of_its_bytes_and_differs_between_instances() {
        assert_eq!(owner_id(&[0, 15, 16, 255]), "000f10ff");
        assert_eq!(owner_id(&[]), "");
        assert_ne!(owner_id(&[1; 16]), owner_id(&[2; 16]));
        assert_eq!(owner_id(&[7; 16]).len(), 32);
    }

    #[test]
    fn refusals_carry_their_code_emitter_and_connect_class() {
        use connectrpc::ErrorCode::{FailedPrecondition, Unavailable};
        for (refusal, code, class) in [
            (
                Refusal::WorkloadIdentityUnavailable,
                WORKLOAD_IDENTITY_UNAVAILABLE,
                Unavailable,
            ),
            (
                Refusal::PeerIdentityRefused,
                PEER_IDENTITY_REFUSED,
                Unavailable,
            ),
            (
                Refusal::UnusablePeerIdentity,
                UNUSABLE_PEER_IDENTITY,
                FailedPrecondition,
            ),
            (
                Refusal::InsecureInFleet,
                INSECURE_IN_FLEET,
                FailedPrecondition,
            ),
        ] {
            assert_eq!(refusal.code(), code);
            let error = refusal.into_error("detail");
            assert_eq!(error.code, class, "{code}");
            let info = api_bones::connect::error_info(&error).expect("info");
            assert_eq!(info.code, code);
            assert_eq!(info.emitter, "bones", "{code}");
            assert!(
                error
                    .message
                    .as_deref()
                    .is_some_and(|m| m.starts_with(code)),
                "{code}"
            );
        }
    }

    #[test]
    fn a_leaf_is_admitted_only_when_it_is_not_a_ca() {
        let want = "spiffe://prod/svc/billing";
        let ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        assert_eq!(
            admits(&[leaf_of(&[want], &[], ca)], want),
            Err(Refusal::PeerIdentityRefused)
        );
        let bounded = rcgen::IsCa::Ca(rcgen::BasicConstraints::Constrained(0));
        assert_eq!(
            admits(&[leaf_of(&[want], &[], bounded)], want),
            Err(Refusal::PeerIdentityRefused)
        );
        assert_eq!(
            admits(&[leaf_of(&[want], &[], rcgen::IsCa::ExplicitNoCa)], want),
            Ok(())
        );
        assert_eq!(
            admits(&[leaf_of(&[want], &[], rcgen::IsCa::NoCa)], want),
            Ok(())
        );
    }

    #[test]
    fn a_dns_san_beside_the_one_uri_san_is_admitted() {
        let want = "spiffe://prod/svc/billing";
        assert_eq!(
            admits(
                &[leaf_of(&[want], &["billing.prod.svc"], rcgen::IsCa::NoCa)],
                want
            ),
            Ok(())
        );
        assert_eq!(
            admits(
                &[leaf_of(&[], &["billing.prod.svc"], rcgen::IsCa::NoCa)],
                want
            ),
            Err(Refusal::PeerIdentityRefused)
        );
    }

    #[test]
    fn the_fetch_lane_refuses_the_fleet_on_node_and_outside_a_browser() {
        let window = |name: &str| name == "window";
        let worker = |name: &str| name == "WorkerGlobalScope";
        let none = |_: &str| false;
        // A browser has no Node version and carries no identity: nothing is refused here.
        assert!(!fetch_lane_refuses_fleet(false, window));
        assert!(!fetch_lane_refuses_fleet(false, worker));
        // jsdom and happy-dom define `window` inside a Node process.
        assert!(fetch_lane_refuses_fleet(true, window));
        assert!(fetch_lane_refuses_fleet(true, worker));
        // Deno, Bun and a host with neither.
        assert!(fetch_lane_refuses_fleet(true, |name| name == "Deno"));
        assert!(fetch_lane_refuses_fleet(false, |name| name == "Bun"));
        assert!(fetch_lane_refuses_fleet(false, none));
    }

    #[test]
    fn debug_of_an_svid_never_shows_its_key() {
        let svid = Svid {
            spiffe_id: "spiffe://prod/svc/bff".to_owned(),
            cert_pem: "CERT".to_owned(),
            key_pem: "SECRET-KEY-MATERIAL".to_owned(),
            bundle_pem: "BUNDLE".to_owned(),
        };
        let shown = format!("{svid:?} {svid:#?}");
        assert!(!shown.contains("SECRET-KEY-MATERIAL"));
        assert!(shown.contains("<redacted>"));
        assert!(shown.contains("spiffe://prod/svc/bff"));
    }

    #[test]
    fn profile_follows_host_and_scheme() {
        assert_eq!(
            profile_of("https", "billing.prod.svc"),
            Ok(Profile::InFleet {
                label: "billing".to_owned()
            })
        );
        assert_eq!(
            profile_of("https", "Billing.Prod.SVC"),
            Ok(Profile::InFleet {
                label: "billing".to_owned()
            })
        );
        assert_eq!(profile_of("http", "billing"), Err(Refusal::InsecureInFleet));
        assert_eq!(profile_of("http", "api.example.com"), Ok(Profile::Public));
        assert_eq!(profile_of("https", "10.0.0.1"), Ok(Profile::Public));
    }

    #[test]
    fn workload_socket_takes_unix_forms_only() {
        assert_eq!(
            workload_socket("unix:///run/spire/agent.sock"),
            Some("/run/spire/agent.sock")
        );
        assert_eq!(
            workload_socket("unix:/run/spire/agent.sock"),
            Some("/run/spire/agent.sock")
        );
        for other in ["tcp://127.0.0.1:8081", "unix://relative/sock", "unix:", ""] {
            assert_eq!(workload_socket(other), None, "{other}");
        }
    }

    #[test]
    fn only_chain_verification_codes_are_peer_refusals() {
        for code in [
            "UNABLE_TO_VERIFY_LEAF_SIGNATURE",
            "SELF_SIGNED_CERT_IN_CHAIN",
            "CERT_HAS_EXPIRED",
            "ERR_TLS_CERT_ALTNAME_INVALID",
        ] {
            assert!(is_verification_failure(code), "{code}");
        }
        for code in ["ECONNREFUSED", "ECONNRESET", "ERR_HTTP2_STREAM_CANCEL", ""] {
            assert!(!is_verification_failure(code), "{code}");
        }
    }

    fn field(number: u8, body: &[u8]) -> Vec<u8> {
        let mut out = vec![(number << 3) | 2, u8::try_from(body.len()).expect("short")];
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn reads_the_first_svid_of_a_response_frame() {
        let cert = leaf(&["spiffe://prod/svc/bff"]);
        assert!(
            cert.len() > 127 && cert.len() < 65536,
            "a long-form DER length"
        );
        let mut svid = field(1, b"spiffe://prod/svc/bff");
        svid.extend(field_long(2, &cert));
        svid.extend(field(3, b"key"));
        svid.extend(field_long(4, &cert));
        let message = field_long(1, &svid);
        let mut framed = vec![0];
        framed.extend((u32::try_from(message.len()).expect("len")).to_be_bytes());
        framed.extend(&message);
        framed.extend([0, 0]);
        let (messages, rest) = grpc_frames(&framed).expect("frames");
        assert_eq!((messages.len(), rest), (1, &[0u8, 0][..]));
        let parsed = first_svid(messages[0]).expect("svid");
        assert_eq!(parsed.spiffe_id, "spiffe://prod/svc/bff");
        assert!(parsed.cert_pem.starts_with("-----BEGIN CERTIFICATE-----\n"));
        assert!(parsed.key_pem.contains("PRIVATE KEY"));
        assert_eq!(parsed.cert_pem, parsed.bundle_pem);
    }

    fn field_long(number: u8, body: &[u8]) -> Vec<u8> {
        let mut out = vec![(number << 3) | 2];
        let mut len = body.len();
        while len >= 0x80 {
            out.push(u8::try_from(len & 0x7f).expect("seven bits") | 0x80);
            len >>= 7;
        }
        out.push(u8::try_from(len).expect("under 0x80"));
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn malformed_input_is_refused() {
        assert!(grpc_frames(&[1, 0, 0, 0, 0]).is_err());
        assert!(first_svid(&[]).is_err());
        assert!(first_svid(&[0x0a, 0x05, 1]).is_err());
        assert_eq!(fetch_request_frame(), [0; 5]);
    }

    #[test]
    fn pem_wraps_at_64_columns() {
        let text = pem("X", &[7u8; 100]);
        assert!(text.lines().skip(1).all(|line| line.len() <= 64));
        assert!(text.ends_with("-----END X-----\n"));
    }

    fn framed(message: &[u8]) -> Vec<u8> {
        let mut out = vec![0];
        out.extend((u32::try_from(message.len()).expect("len")).to_be_bytes());
        out.extend_from_slice(message);
        out
    }

    #[test]
    fn a_frame_split_across_chunks_waits_for_its_payload() {
        let whole = framed(b"payload");
        // A complete header and a payload that has not all arrived.
        for cut in [5, 6, whole.len() - 1] {
            let (messages, rest) = grpc_frames(&whole[..cut]).expect("frames");
            assert!(messages.is_empty(), "{cut}");
            assert_eq!(rest, &whole[..cut]);
        }
        let (messages, rest) = grpc_frames(&whole).expect("frames");
        assert_eq!((messages, rest), (vec![&b"payload"[..]], &[][..]));
        let mut two = whole.clone();
        two.extend(framed(b"next"));
        let (messages, rest) = grpc_frames(&two[..two.len() - 1]).expect("frames");
        assert_eq!((messages, rest.len()), (vec![&b"payload"[..]], 8));
    }

    #[test]
    fn a_frame_larger_than_the_cap_ends_the_stream() {
        let header = |len: u32| {
            let mut out = vec![0];
            out.extend(len.to_be_bytes());
            out
        };
        let at_cap = header(u32::try_from(MAX_FRAME).expect("cap"));
        let (messages, rest) = grpc_frames(&at_cap).expect("a frame at the cap waits");
        assert!(messages.is_empty(), "{messages:?}");
        assert_eq!(rest, &at_cap[..]);
        for len in [
            u32::try_from(MAX_FRAME).expect("cap") + 1,
            u32::MAX - 4,
            u32::MAX - 5,
            u32::MAX,
        ] {
            let error = grpc_frames(&header(len)).expect_err("oversize");
            assert!(error.contains("exceeds"), "{len}: {error}");
        }
    }

    fn svid_message(id: &[u8], cert: &[u8], key: &[u8], bundle: &[u8]) -> Vec<u8> {
        let mut svid = field_long(1, id);
        svid.extend(field_long(2, cert));
        svid.extend(field_long(3, key));
        svid.extend(field_long(4, bundle));
        field_long(1, &svid)
    }

    #[test]
    fn an_svid_missing_a_field_or_naming_bytes_that_are_not_text_is_refused() {
        let der = [0x30, 0x02, 1, 2];
        let whole = svid_message(b"spiffe://prod/svc/bff", &der, b"key", &der);
        assert!(first_svid(&whole).is_ok());
        for missing in 1..=4u8 {
            let mut svid = Vec::new();
            for (number, body) in [
                (1u8, &b"spiffe://prod/svc/bff"[..]),
                (2, &der[..]),
                (3, b"key"),
                (4, &der[..]),
            ] {
                if number != missing {
                    svid.extend(field_long(number, body));
                }
            }
            assert_eq!(
                first_svid(&field_long(1, &svid)),
                Err("an X509-SVID without every field".to_owned()),
                "without field {missing}"
            );
        }
        let error =
            first_svid(&svid_message(&[0xff, 0xfe], &der, b"key", &der)).expect_err("not UTF-8");
        assert!(error.contains("utf-8"), "{error}");
    }

    #[test]
    fn unknown_varint_fields_are_skipped_and_other_wire_types_refused() {
        let der = [0x30, 0x02, 1, 2];
        // A varint field (number 3) ahead of the SVID is skipped.
        let mut message = vec![0x18, 0x96, 0x01];
        message.extend(svid_message(b"spiffe://prod/svc/bff", &der, b"key", &der));
        assert_eq!(
            first_svid(&message).expect("svid").spiffe_id,
            "spiffe://prod/svc/bff"
        );
        // Fixed 32-bit (5) and fixed 64-bit (1) fields are not part of the message.
        for tag in [0x0d_u8, 0x09] {
            let error = first_svid(&[tag, 0, 0, 0, 0, 0, 0, 0, 0]).expect_err("wire type");
            assert!(error.starts_with("unsupported wire type"), "{error}");
        }
        let mut endless = vec![0x18];
        endless.extend([0x80_u8; 11]);
        assert_eq!(first_svid(&endless), Err("a bad varint".to_owned()));
        assert_eq!(
            first_svid(&[0x0a, 0x05, 1]),
            Err("a truncated field".to_owned())
        );
    }

    #[test]
    fn pem_chain_refuses_truncated_and_empty_sets_and_splits_every_length_form() {
        let truncated = Err("a truncated certificate".to_owned());
        // No length byte, a short length past the end, a long form whose length
        // bytes or body are missing.
        assert_eq!(pem_chain("CERTIFICATE", &[0x30]), truncated);
        assert_eq!(pem_chain("CERTIFICATE", &[0x30, 0x05, 1, 2]), truncated);
        assert_eq!(pem_chain("CERTIFICATE", &[0x30, 0x82, 0x01]), truncated);
        assert_eq!(pem_chain("CERTIFICATE", &[0x30, 0x81, 0x90, 1]), truncated);
        assert_eq!(
            pem_chain("CERTIFICATE", &[]),
            Err("an empty certificate set".to_owned())
        );
        // A short-form certificate then a long-form one, concatenated.
        let mut long = vec![0x30, 0x81, 0x80];
        long.extend([7u8; 128]);
        let mut set = vec![0x30, 0x02, 1, 2];
        set.extend(&long);
        let text = pem_chain("CERTIFICATE", &set).expect("two certificates");
        assert_eq!(text.matches("-----BEGIN CERTIFICATE-----").count(), 2);
        assert_eq!(text.matches("-----END CERTIFICATE-----").count(), 2);
    }
}
