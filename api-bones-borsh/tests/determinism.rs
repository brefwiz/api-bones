//! Determinism snapshot tests for `api-bones-borsh` (ADR platform/0047).
//!
//! These tests verify:
//! 1. `to_canonical_bytes()` is stable across repeated calls.
//! 2. Output matches `borsh::to_vec()` directly.
//! 3. A golden byte snapshot catches any Borsh-spec drift.

use api_bones_borsh::prelude::*;

/// Representative test payload covering the primitive types used by
/// platform chain-bound types.
#[derive(Debug, PartialEq, BorshSerialize, BorshDeserialize)]
struct TestPayload {
    schema_id: u32,
    schema_version: u16,
    value: u64,
    label: String,
    flags: Vec<bool>,
}

impl BorshCanonical for TestPayload {
    const CODEC_VERSION: u8 = 1;
}

fn sample() -> TestPayload {
    TestPayload {
        schema_id: 42,
        schema_version: 3,
        value: 0xDEAD_BEEF_CAFE_1234,
        label: "brefwiz".to_string(),
        flags: vec![true, false, true],
    }
}

#[test]
fn canonical_bytes_are_stable() {
    let p = sample();
    let a = p.to_canonical_bytes().expect("first encode");
    let b = p.to_canonical_bytes().expect("second encode");
    assert_eq!(a, b, "to_canonical_bytes() must be deterministic");
}

#[test]
fn canonical_bytes_match_borsh_to_vec() {
    let p = sample();
    let canonical = p.to_canonical_bytes().expect("canonical encode");
    let direct = to_vec(&p).expect("borsh::to_vec");
    assert_eq!(
        canonical, direct,
        "to_canonical_bytes() must equal borsh::to_vec() output"
    );
}

#[test]
fn golden_snapshot() {
    let p = sample();
    let bytes = p.to_canonical_bytes().expect("encode");

    // Golden bytes for sample():
    //   schema_id u32 LE=42, schema_version u16 LE=3,
    //   value u64 LE=0xDEADBEEFCAFE1234,
    //   label String (u32 len + utf8), flags Vec<bool> (u32 len + u8 each)
    #[rustfmt::skip]
    let expected: &[u8] = &[
        0x2a, 0x00, 0x00, 0x00,                               // schema_id = 42
        0x03, 0x00,                                            // schema_version = 3
        0x34, 0x12, 0xfe, 0xca, 0xef, 0xbe, 0xad, 0xde,      // value
        0x07, 0x00, 0x00, 0x00,                               // label len = 7
        b'b', b'r', b'e', b'f', b'w', b'i', b'z',            // "brefwiz"
        0x03, 0x00, 0x00, 0x00,                               // flags len = 3
        0x01, 0x00, 0x01,                                     // [true, false, true]
    ];

    assert_eq!(
        bytes.as_slice(),
        expected,
        "golden snapshot mismatch — Borsh spec may have drifted"
    );
}

#[test]
fn versioned_envelope_round_trips() {
    let p = sample();
    let versioned = p.to_versioned().expect("versioned encode");
    assert_eq!(versioned.codec_version, TestPayload::CODEC_VERSION);

    // VersionedBytes itself is Borsh-serialisable.
    let outer = to_vec(&versioned).expect("outer encode");
    let decoded: api_bones_borsh::VersionedBytes = from_slice(&outer).expect("outer decode");
    assert_eq!(decoded, versioned);
}

#[test]
fn round_trip_decode() {
    let original = sample();
    let bytes = original.to_canonical_bytes().expect("encode");
    let decoded: TestPayload = from_slice(&bytes).expect("decode");
    assert_eq!(original, decoded);
}
