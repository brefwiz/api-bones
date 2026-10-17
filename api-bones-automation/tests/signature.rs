// SPDX-License-Identifier: MIT
//! The DSSE signature, checked against a signature computed independently
//! (Python `cryptography`, Ed25519 over the DSSE pre-authentication encoding)
//! and against every way verification and admission refuse.

use api_bones_automation::signature::{
    AdmitError, PAYLOAD_TYPE, SigningKey, TrustedKey, VerifyError, VerifyingKey, admit, pae, sign,
    signed_manifest, verify,
};
use api_bones_automation::{ManifestError, read_header};

/// A manifest with only its header fields set, encoded by hand: format 1,
/// product `orders`, version `1.2.3`, a commit and a declaration digest.
const MANIFEST_HEX: &str = "080112066f72646572731a05312e322e332228303132333435363738396162636465663031323334353637383961626364656630313233343536372a477368613235363a61626162616261626162616261626162616261626162616261626162616261626162616261626162616261626162616261626162616261626162616261626162";
const DIGEST: &str = "sha256:abababababababababababababababababababababababababababababababab";
/// Base64 Ed25519 signature of the key seeded with 32 bytes of 0x07 over
/// `pae(PAYLOAD_TYPE, MANIFEST)`, computed outside this crate.
const GOLDEN_SIG: &str =
    "DU2gc3qRvTAWhI+0AABfVqfW98WTrYRu7zlWExF8BfDByn9TGHUQlHp/QS7F2Yu8NM99e7c6FUju4U7e+kTVAg==";
const GOLDEN_PUBLIC: [u8; 32] = [
    234, 74, 108, 99, 226, 156, 82, 10, 190, 245, 80, 123, 19, 46, 197, 249, 149, 71, 118, 174,
    190, 190, 123, 146, 66, 30, 234, 105, 20, 70, 210, 44,
];

fn manifest() -> Vec<u8> {
    (0..MANIFEST_HEX.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&MANIFEST_HEX[i..i + 2], 16).unwrap())
        .collect()
}

fn key() -> SigningKey {
    SigningKey::from_bytes(&[7; 32])
}

fn trusted(keyid: &str, key: &SigningKey, not_before: i64, not_after: i64) -> TrustedKey {
    TrustedKey {
        keyid: keyid.to_owned(),
        key: key.verifying_key(),
        not_before,
        not_after,
    }
}

fn signature_json(keyid: &str, sig: &str) -> Vec<u8> {
    format!(r#"{{"keyid":"{keyid}","sig":"{sig}"}}"#).into_bytes()
}

#[test]
fn the_pae_bytes_are_the_dsse_v1_encoding() {
    assert_eq!(
        pae("application/example", b"hello"),
        b"DSSEv1 19 application/example 5 hello".to_vec()
    );
    assert_eq!(pae("t", b""), b"DSSEv1 1 t 0 ".to_vec());
    let encoded = pae(PAYLOAD_TYPE, &manifest());
    let prefix = format!(
        "DSSEv1 {} {PAYLOAD_TYPE} {} ",
        PAYLOAD_TYPE.len(),
        manifest().len()
    );
    assert!(encoded.starts_with(prefix.as_bytes()));
    assert!(encoded.ends_with(&manifest()));
    assert_eq!(encoded.len(), prefix.len() + manifest().len());
}

#[test]
fn the_payload_type_is_the_automation_manifest_type() {
    assert_eq!(
        PAYLOAD_TYPE,
        "application/vnd.brefwiz.automation-manifest.v1+protobuf"
    );
}

#[test]
fn signing_reproduces_the_independently_computed_signature() {
    let object = sign(&manifest(), "k1", &key());
    assert_eq!(object, signature_json("k1", GOLDEN_SIG));
    assert_eq!(key().verifying_key().to_bytes(), GOLDEN_PUBLIC);
}

#[test]
fn the_golden_signature_verifies_with_the_published_key() {
    let public = VerifyingKey::from_bytes(&GOLDEN_PUBLIC).unwrap();
    let keys = [TrustedKey {
        keyid: "k1".to_owned(),
        key: public,
        not_before: 10,
        not_after: 20,
    }];
    let signed = signed_manifest(&manifest(), &signature_json("k1", GOLDEN_SIG));
    let verified = verify(&signed, &keys, 10).unwrap();
    assert_eq!(verified.manifest, manifest());
    assert_eq!(verified.keyid, "k1");
}

fn signed(keyid: &str) -> Vec<u8> {
    signed_manifest(&manifest(), &sign(&manifest(), keyid, &key()))
}

#[test]
fn the_window_is_inclusive_at_the_start_and_exclusive_at_the_end() {
    let keys = [trusted("k1", &key(), 100, 200)];
    assert!(verify(&signed("k1"), &keys, 100).is_ok());
    assert!(verify(&signed("k1"), &keys, 199).is_ok());
    for at in [99, 200, 201] {
        assert_eq!(
            verify(&signed("k1"), &keys, at).unwrap_err(),
            VerifyError::KeyNotValid {
                keyid: "k1".to_owned(),
                at
            }
        );
    }
}

#[test]
fn a_rotated_set_verifies_by_epoch() {
    let next = SigningKey::from_bytes(&[8; 32]);
    let keys = [
        trusted("k1", &key(), 0, 100),
        trusted("k2", &next, 100, 200),
    ];
    assert_eq!(verify(&signed("k1"), &keys, 50).unwrap().keyid, "k1");
    assert!(verify(&signed("k1"), &keys, 150).is_err());
    let second = signed_manifest(&manifest(), &sign(&manifest(), "k2", &next));
    assert_eq!(verify(&second, &keys, 150).unwrap().keyid, "k2");
}

#[test]
fn an_unknown_key_is_refused() {
    let keys = [trusted("other", &key(), 0, 100)];
    assert_eq!(
        verify(&signed("k1"), &keys, 1).unwrap_err(),
        VerifyError::UnknownKey("k1".to_owned())
    );
    assert_eq!(
        verify(&signed("k1"), &[], 1).unwrap_err(),
        VerifyError::UnknownKey("k1".to_owned())
    );
}

#[test]
fn a_signature_by_the_wrong_key_is_refused() {
    let impostor = SigningKey::from_bytes(&[9; 32]);
    let keys = [trusted("k1", &impostor, 0, 100)];
    assert_eq!(
        verify(&signed("k1"), &keys, 1).unwrap_err(),
        VerifyError::BadSignature("k1".to_owned())
    );
}

#[test]
fn a_changed_manifest_is_refused() {
    let keys = [trusted("k1", &key(), 0, 100)];
    let mut tampered = manifest();
    tampered[3] ^= 1;
    let envelope = signed_manifest(&tampered, &sign(&manifest(), "k1", &key()));
    assert_eq!(
        verify(&envelope, &keys, 1).unwrap_err(),
        VerifyError::BadSignature("k1".to_owned())
    );
}

#[test]
fn a_signature_over_another_payload_type_is_refused() {
    use ed25519_dalek::Signer;
    let keys = [trusted("k1", &key(), 0, 100)];
    let other = key().sign(&pae("application/other", &manifest()));
    let object = signature_json("k1", &base64_of(&other.to_bytes()));
    let envelope = signed_manifest(&manifest(), &object);
    assert_eq!(
        verify(&envelope, &keys, 1).unwrap_err(),
        VerifyError::BadSignature("k1".to_owned())
    );
}

fn base64_of(bytes: &[u8]) -> String {
    use base64::{Engine, engine::general_purpose::STANDARD};
    STANDARD.encode(bytes)
}

#[test]
fn a_malformed_envelope_is_refused() {
    let keys = [trusted("k1", &key(), 0, 100)];
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("not protobuf", vec![0xff, 0xff, 0xff]),
        ("no signature", signed_manifest(&manifest(), b"")),
        (
            "not json",
            signed_manifest(&manifest(), b"this is not json"),
        ),
        (
            "unknown member",
            signed_manifest(
                &manifest(),
                format!(r#"{{"keyid":"k1","sig":"{GOLDEN_SIG}","extra":1}}"#).as_bytes(),
            ),
        ),
        (
            "sig not base64",
            signed_manifest(&manifest(), &signature_json("k1", "***")),
        ),
        (
            "sig wrong length",
            signed_manifest(&manifest(), &signature_json("k1", "AAAA")),
        ),
    ];
    for (why, envelope) in cases {
        assert!(
            matches!(verify(&envelope, &keys, 1), Err(VerifyError::Malformed(_))),
            "{why}"
        );
    }
}

#[test]
fn a_duplicated_key_id_verifies_with_any_valid_holder() {
    let impostor = SigningKey::from_bytes(&[9; 32]);
    let keys = [
        trusted("k1", &impostor, 0, 100),
        trusted("k1", &key(), 0, 100),
    ];
    assert!(verify(&signed("k1"), &keys, 1).is_ok());
}

fn manifest_for(product: &str, digest: &str, format: u32) -> Vec<u8> {
    fn field(tag: u8, bytes: &[u8]) -> Vec<u8> {
        let mut out = vec![tag, u8::try_from(bytes.len()).unwrap()];
        out.extend_from_slice(bytes);
        out
    }
    let mut out = vec![0x08, u8::try_from(format).unwrap()];
    out.extend(field(0x12, product.as_bytes()));
    out.extend(field(0x1a, b"1.2.3"));
    out.extend(field(0x22, &[b'a'; 40]));
    out.extend(field(0x2a, digest.as_bytes()));
    out
}

fn envelope_for(manifest: &[u8]) -> Vec<u8> {
    signed_manifest(manifest, &sign(manifest, "k1", &key()))
}

#[test]
fn admission_returns_the_header_of_the_matching_manifest() {
    let keys = [trusted("k1", &key(), 0, 100)];
    let header = admit(&envelope_for(&manifest()), &keys, 1, "orders", DIGEST).unwrap();
    assert_eq!(header.format, 1);
    assert_eq!(header.product, "orders");
    assert_eq!(header.version, "1.2.3");
    assert_eq!(header.commit, "0123456789abcdef0123456789abcdef01234567");
    assert_eq!(header.declaration_digest, DIGEST);
    assert_eq!(header, read_header(&manifest()).unwrap());
}

#[test]
fn admission_refuses_each_way_the_manifest_can_be_wrong() {
    let keys = [trusted("k1", &key(), 0, 100)];
    let good = envelope_for(&manifest());

    assert!(matches!(
        admit(&good, &keys, 500, "orders", DIGEST),
        Err(AdmitError::Verify(VerifyError::KeyNotValid { .. }))
    ));
    assert_eq!(
        admit(&good, &keys, 1, "billing", DIGEST).unwrap_err(),
        AdmitError::ProductMismatch {
            expected: "billing".to_owned(),
            found: "orders".to_owned()
        }
    );
    let served = "sha256:0000";
    assert_eq!(
        admit(&good, &keys, 1, "orders", served).unwrap_err(),
        AdmitError::DigestMismatch {
            expected: served.to_owned(),
            found: DIGEST.to_owned()
        }
    );
    let unknown = envelope_for(&manifest_for("orders", DIGEST, 2));
    assert_eq!(
        admit(&unknown, &keys, 1, "orders", DIGEST).unwrap_err(),
        AdmitError::Manifest(ManifestError::UnknownFormat(2))
    );
    let zero = envelope_for(&manifest_for("orders", DIGEST, 0));
    assert_eq!(
        admit(&zero, &keys, 1, "orders", DIGEST).unwrap_err(),
        AdmitError::Manifest(ManifestError::UnknownFormat(0))
    );
    let garbage = envelope_for(&[0xff, 0xff]);
    assert!(matches!(
        admit(&garbage, &keys, 1, "orders", DIGEST),
        Err(AdmitError::Manifest(ManifestError::Undecodable(_)))
    ));
}

#[test]
fn every_error_renders_and_is_an_error() {
    fn is_error<E: std::error::Error>(e: &E) -> String {
        e.to_string()
    }
    let verify_errors = [
        VerifyError::Malformed("m".into()),
        VerifyError::UnknownKey("k".into()),
        VerifyError::KeyNotValid {
            keyid: "k".into(),
            at: 5,
        },
        VerifyError::BadSignature("k".into()),
    ];
    let rendered: Vec<String> = verify_errors.iter().map(is_error).collect();
    assert!(rendered[0].contains("malformed"));
    assert!(rendered[1].contains("not trusted"));
    assert!(rendered[2].contains("not valid at 5"));
    assert!(rendered[3].contains("does not verify"));

    let admit_errors = [
        AdmitError::Verify(VerifyError::UnknownKey("k".into())),
        AdmitError::Manifest(ManifestError::UnknownFormat(9)),
        AdmitError::Manifest(ManifestError::Undecodable("x".into())),
        AdmitError::ProductMismatch {
            expected: "a".into(),
            found: "b".into(),
        },
        AdmitError::DigestMismatch {
            expected: "a".into(),
            found: "b".into(),
        },
    ];
    let rendered: Vec<String> = admit_errors.iter().map(is_error).collect();
    assert!(rendered[0].contains("not trusted"));
    assert!(rendered[1].contains("format 9"));
    assert!(rendered[2].contains("does not decode"));
    assert!(rendered[3].contains("product b, expected a"));
    assert!(rendered[4].contains("b is not the served a"));
}
