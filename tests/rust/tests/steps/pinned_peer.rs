// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! The Rust half of `../../features/connect_pinned_peer.feature`.
//!
//! The TypeScript half answers the same file; neither keeps its own copy of
//! the rows. The server side is a real TLS acceptor on loopback, issued by a
//! test CA: nothing about the handshake is stubbed, and no verification lives
//! here. A refusal is classified by the SPIFFE client's own `tls_verdict` and
//! `spiffe_refusal`, reached through the package under test.

use std::time::Duration;

use api_bones_connect::{PinnedPeerError, SpiffeError, spiffe_refusal, tls_verdict};
use connectrpc::client::ClientTransport as _;
use connectrpc::rustls::pki_types::PrivateKeyDer;
use connectrpc::rustls::{CertificateError, Error as TlsError};
use cucumber::{given, then, when};
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, Ia5String, IsCa, KeyPair,
    KeyUsagePurpose, SanType,
};

use crate::world::{Authority, LoopbackServer, PinnedPeerWorld};

/// A handshake that has not finished by now never will.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

const SIGNER: &str = "spiffe://prod.example/svc/signer";

fn install_provider() {
    let _ = connectrpc::rustls::crypto::aws_lc_rs::default_provider().install_default();
}

/// A CA with a name of its own, so a stranger is not mistaken for the test CA.
fn authority(name: &str) -> Authority {
    let key = KeyPair::generate().expect("ca key");
    let mut params = CertificateParams::new(Vec::new()).expect("ca params");
    params.distinguished_name.push(DnType::CommonName, name);
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let cert = params.self_signed(&key).expect("ca cert");
    Authority { cert, key }
}

fn test_ca(world: &mut PinnedPeerWorld) -> &Authority {
    world.test_ca.get_or_insert_with(|| authority("test-ca"))
}

fn stranger_ca(world: &mut PinnedPeerWorld) -> &Authority {
    world
        .stranger_ca
        .get_or_insert_with(|| authority("stranger-ca"))
}

fn issuer<'a>(world: &'a mut PinnedPeerWorld, who: &str) -> &'a Authority {
    match who {
        "the test CA" => test_ca(world),
        "a stranger CA" => stranger_ca(world),
        other => panic!("the contract names an issuer this step cannot build: {other}"),
    }
}

/// A leaf issued by `ca` naming exactly `uris`. With `as_ca` the leaf is itself
/// a CA certificate, which also carries the server-auth usages so any refusal
/// is the caller's.
fn leaf(
    ca: &Authority,
    uris: &[&str],
    as_ca: bool,
) -> (rcgen::Certificate, PrivateKeyDer<'static>) {
    let key = KeyPair::generate().expect("leaf key");
    let mut params = CertificateParams::new(Vec::new()).expect("leaf params");
    params.subject_alt_names = uris
        .iter()
        .map(|u| SanType::URI(Ia5String::try_from((*u).to_owned()).expect("ia5 uri")))
        .collect();
    if as_ca {
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyCertSign,
        ];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    }
    let cert = params
        .signed_by(&key, &ca.cert, &ca.key)
        .expect("leaf cert");
    let der = PrivateKeyDer::try_from(key.serialize_der()).expect("pkcs8 key");
    (cert, der)
}

// -- the bundle --------------------------------------------------------------

#[given("a trust bundle holding the test CA")]
fn bundle_test_ca(world: &mut PinnedPeerWorld) {
    world.bundle = test_ca(world).cert.pem();
}

#[given("a trust bundle holding nothing")]
fn bundle_nothing(world: &mut PinnedPeerWorld) {
    world.bundle = String::new();
}

#[given("a trust bundle holding only a leaf certificate")]
fn bundle_only_leaf(world: &mut PinnedPeerWorld) {
    install_provider();
    let ca = test_ca(world);
    world.bundle = leaf(ca, &[SIGNER], false).0.pem();
}

#[given("a trust bundle holding the test CA and a leaf certificate")]
fn bundle_ca_and_leaf(world: &mut PinnedPeerWorld) {
    install_provider();
    let ca = test_ca(world);
    world.bundle = ca.cert.pem() + &leaf(ca, &[SIGNER], false).0.pem();
}

#[given("a trust bundle holding a certificate with no basicConstraints")]
fn bundle_no_basic_constraints(world: &mut PinnedPeerWorld) {
    let key = KeyPair::generate().expect("key");
    let mut params = CertificateParams::new(Vec::new()).expect("params");
    // `NoCa` omits the extension altogether; `ExplicitNoCa` would say CA:FALSE.
    params.is_ca = IsCa::NoCa;
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign];
    world.bundle = params.self_signed(&key).expect("cert").pem();
}

// -- the server --------------------------------------------------------------

#[given(
    regex = r#"^a server presenting an SVID for "([^"]*)" issued by (the test CA|a stranger CA)$"#
)]
fn server_svid(world: &mut PinnedPeerWorld, uri: String, who: String) {
    install_provider();
    let (cert, key) = leaf(issuer(world, &who), &[&uri], false);
    world.server = Some((cert.der().clone(), key));
}

#[given(
    regex = r#"^a server presenting a CA certificate for "([^"]*)" issued by (the test CA|a stranger CA)$"#
)]
fn server_ca_certificate(world: &mut PinnedPeerWorld, uri: String, who: String) {
    install_provider();
    let (cert, key) = leaf(issuer(world, &who), &[&uri], true);
    world.server = Some((cert.der().clone(), key));
}

#[given(
    regex = r"^a server presenting a certificate with no URI SAN issued by (the test CA|a stranger CA)$"
)]
fn server_no_uri(world: &mut PinnedPeerWorld, who: String) {
    install_provider();
    let (cert, key) = leaf(issuer(world, &who), &[], false);
    world.server = Some((cert.der().clone(), key));
}

#[given(
    regex = r#"^a server presenting a certificate with the URI SANs "([^"]*)" issued by (the test CA|a stranger CA)$"#
)]
fn server_uris(world: &mut PinnedPeerWorld, uris: String, who: String) {
    install_provider();
    let uris: Vec<&str> = uris.split(',').collect();
    let (cert, key) = leaf(issuer(world, &who), &uris, false);
    world.server = Some((cert.der().clone(), key));
}

// -- the caller --------------------------------------------------------------

#[when(expr = "a caller pinning {string} connects")]
async fn connects(world: &mut PinnedPeerWorld, peer: String) {
    install_provider();
    let (cert, key) = world.server.take().expect("no server was given");
    let ca = test_ca(world).cert.der().clone();
    let server = LoopbackServer::start(cert, key, ca).await;
    let url = format!("https://localhost:{}/pkg.v1.Svc/Method", server.addr.port());
    world.loopback = Some(server);
    call(world, &peer, url).await;
}

/// One request to `url` from a caller pinning `peer`, recorded as the outcome.
async fn call(world: &mut PinnedPeerWorld, peer: &str, url: String) {
    let request = http::Request::builder()
        .method("POST")
        .uri(url)
        .body(connectrpc::client::full_body(bytes::Bytes::new()))
        .expect("request");
    let client = api_bones_connect::pinned_peer_http_client(world.bundle.as_bytes(), peer)
        .expect("a valid peer and bundle build a client");
    let attempt = tokio::time::timeout(HANDSHAKE_TIMEOUT, client.send(request))
        .await
        .expect("the request neither finished nor failed");
    world.outcome = Some(attempt.map(|response| response.status().as_u16()));
}

#[when(expr = "a caller pinning {string} is built from that bundle")]
fn built(world: &mut PinnedPeerWorld, peer: String) {
    install_provider();
    world.built = Some(
        api_bones_connect::pinned_peer_client_tls_config(world.bundle.as_bytes(), &peer)
            .map(|_| ()),
    );
}

#[when(expr = "a caller pinning {string} is asked to call the cleartext address {string}")]
async fn asked_cleartext(world: &mut PinnedPeerWorld, peer: String, url: String) {
    install_provider();
    call(world, &peer, url).await;
}

// -- the verdicts ------------------------------------------------------------

type Outcome = Option<Result<u16, connectrpc::ConnectError>>;

/// What the one request came to, named as the contract names it. A SPIFFE
/// refusal is read by the SPIFFE client's own classifiers; the chain refusal is
/// the TLS stack's own, with no SPIFFE refusal and an issuer it could not find.
fn outcome_kind(outcome: &Outcome) -> &'static str {
    match outcome {
        None => "no request",
        Some(Ok(200)) => "established",
        Some(Ok(_)) => "answered otherwise",
        Some(Err(error)) => match (
            tls_verdict(error),
            tls_verdict(error).and_then(spiffe_refusal),
        ) {
            (_, Some(SpiffeError::PeerNotAllowed(_))) => "a wrong identity",
            (_, Some(SpiffeError::InvalidSvid(_))) => "a non-SVID leaf",
            (_, Some(_)) => "another SPIFFE refusal",
            (Some(TlsError::InvalidCertificate(CertificateError::UnknownIssuer)), None) => {
                "an untrusted chain"
            }
            _ => "an unclassified failure",
        },
    }
}

#[then("the connection is established")]
fn established(world: &mut PinnedPeerWorld) {
    assert_eq!(
        outcome_kind(&world.outcome),
        "established",
        "the server must answer the request: {:?}",
        world.outcome
    );
}

#[then("the server completed its handshake having seen no client certificate")]
fn no_client_cert(world: &mut PinnedPeerWorld) {
    // `None` is a server that never served a request: not "saw nothing".
    assert_eq!(
        world
            .loopback
            .as_ref()
            .and_then(LoopbackServer::saw_client_cert),
        Some(false),
        "the server must serve the request and see no client certificate"
    );
}

#[then(
    regex = r"^the connection is refused for (a wrong identity|a non-SVID leaf|an untrusted chain)$"
)]
fn refused_for(world: &mut PinnedPeerWorld, reason: String) {
    assert_eq!(
        outcome_kind(&world.outcome),
        reason,
        "refused for the wrong reason: {:?}",
        world.outcome
    );
}

/// How building a caller ended, named as the contract names it.
fn built_kind(built: &Option<Result<(), PinnedPeerError>>) -> &'static str {
    match built {
        None => "not built",
        Some(Ok(())) => "built",
        Some(Err(PinnedPeerError::InvalidTrustBundle { .. })) => "an invalid trust bundle",
        Some(Err(PinnedPeerError::InvalidPeerId { .. })) => "an invalid peer id",
        Some(Err(_)) => "another refusal",
    }
}

#[then(regex = r"^it is refused as (an invalid trust bundle|an invalid peer id)$")]
fn refused_as(world: &mut PinnedPeerWorld, reason: String) {
    assert_eq!(
        built_kind(&world.built),
        reason,
        "refused for the wrong reason: {:?}",
        world.built
    );
}

// The refusal is the client's own invalid-argument answer, given before any
// connection; a connection attempt would end as `unavailable` instead.
#[then("it is refused as a cleartext address")]
fn refused_cleartext(world: &mut PinnedPeerWorld) {
    match &world.outcome {
        Some(Err(error)) => assert_eq!(
            error.code,
            connectrpc::ErrorCode::InvalidArgument,
            "a cleartext address must be refused up front: {error:?}"
        ),
        other => panic!("a cleartext address must be refused, got {other:?}"),
    }
}
