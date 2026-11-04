// SPDX-License-Identifier: MIT
//! Document checks use independently compiled manifests, never merged pools.

use api_bones_automation::{DocumentError, DocumentInput, DocumentPhase, Rule, check_document};

#[path = "document.rs"]
mod support;

#[test]
fn admission_resolves_cross_product_bindings_and_keeps_parameter_slots_open() {
    for case in [
        "open required parameter",
        "local reference",
        "cross-product trigger",
        "matching trigger",
        "matching output",
        "matching repeated",
        "matching enum",
        "matching message",
        "matching map",
        "matching resolver",
    ] {
        support::fixture(case)
            .check(DocumentPhase::Admission)
            .unwrap_or_else(|e| panic!("{case}: {e}"));
    }
}

#[test]
fn publish_requires_an_argument_or_default_only_for_required_parameters() {
    for case in ["argument value", "default value", "optional parameter"] {
        support::fixture(case)
            .check(DocumentPhase::Publish)
            .unwrap_or_else(|e| panic!("{case}: {e}"));
    }
    assert_rule("open required parameter", DocumentPhase::Publish, Rule::D6);
}

#[test]
fn document_rules_refuse_invalid_cross_product_references_and_bindings() {
    for (case, rule) in [
        ("wrong binding type", Rule::D4),
        ("unknown action", Rule::D1),
        ("unknown product", Rule::D1),
        ("unknown trigger", Rule::D1),
        ("person-reserved action", Rule::D2),
        ("resolver without resolves", Rule::D5),
        ("unknown parameter", Rule::D5),
        ("unknown target field", Rule::D4),
        ("unknown source field", Rule::D4),
        ("repeated mismatch", Rule::D4),
        ("different enum", Rule::D4),
        ("different message", Rule::D4),
        ("changed message", Rule::D4),
        ("map mismatch", Rule::D4),
        ("output mismatch", Rule::D4),
        ("later output", Rule::D3),
        ("literal mismatch", Rule::D4),
        ("missing manifest", Rule::D1),
    ] {
        assert_rule(case, DocumentPhase::Admission, rule);
    }
}

#[test]
fn arguments_are_checked_during_both_admission_and_publish() {
    for phase in [DocumentPhase::Admission, DocumentPhase::Publish] {
        for case in ["wrong argument", "unknown argument", "duplicate argument"] {
            assert_rule(case, phase, Rule::D6);
        }
    }
}

#[test]
fn manifest_and_document_input_errors_preserve_the_cause() {
    for case in ["invalid manifest", "unsupported manifest"] {
        let error = support::fixture(case)
            .check(DocumentPhase::Admission)
            .unwrap_err();
        assert!(
            matches!(error, DocumentError::Manifest { index: 1, .. }),
            "{error}"
        );
        assert!(error.to_string().contains("manifest 1"));
    }
    let duplicate = support::fixture("duplicate manifest")
        .check(DocumentPhase::Admission)
        .unwrap_err();
    assert!(matches!(duplicate, DocumentError::Input(_)));
    assert!(duplicate.to_string().contains("warehouse"));
    let undecodable = support::fixture("invalid document")
        .check(DocumentPhase::Admission)
        .unwrap_err();
    assert!(matches!(undecodable, DocumentError::Undecodable(_)));
    assert!(undecodable.to_string().contains("document does not decode"));
}

#[test]
fn input_must_name_a_declaring_product_with_one_admitted_manifest() {
    let fixture = support::fixture("argument value");
    let manifests: Vec<&[u8]> = fixture.manifests.iter().map(Vec::as_slice).collect();
    for product in ["", "absent"] {
        let error = check_document(&DocumentInput {
            document: &fixture.document,
            product,
            manifests: &manifests,
            phase: DocumentPhase::Publish,
        })
        .unwrap_err();
        assert!(matches!(error, DocumentError::Input(_)));
    }
}

#[test]
fn refusals_are_deterministic_and_identify_the_document_site() {
    let fixture = support::fixture("wrong binding type");
    let first = fixture.check(DocumentPhase::Admission).unwrap_err();
    assert_eq!(first, fixture.check(DocumentPhase::Admission).unwrap_err());
    let DocumentError::Refused(refusals) = &first else {
        panic!("{first}");
    };
    assert_eq!(refusals[0].site, "document step send");
    assert!(first.to_string().contains("D4 at document step send"));
}

fn assert_rule(case: &str, phase: DocumentPhase, rule: Rule) {
    let error = support::fixture(case).check(phase).unwrap_err();
    let DocumentError::Refused(refusals) = error else {
        panic!("{case}: {error}");
    };
    assert!(
        refusals.iter().any(|r| r.rule == rule),
        "{case}: {refusals:?}"
    );
}
