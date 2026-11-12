// SPDX-License-Identifier: MIT
//! A provider's delegation templates survive compilation, read back through
//! descriptor reflection, and `check_delegation_templates` refuses each way a
//! template can name something its service does not declare.

use api_bones_protos::delegation_templates::{Reason, Refusal, check_delegation_templates};
use prost_reflect::DescriptorPool;

#[path = "../../tests/shared/shipped_resolver.rs"]
mod shipped_resolver;
use shipped_resolver::{CONTRACT, compiler};

/// What every service of these contracts declares: one scope per source, one
/// group, and one dimension of each match kind.
const DECLARED: &str = r#"
  option (bones.v1.provided_capabilities) = "planning";
  option (bones.v1.capability) = "legacy";
  option (bones.v1.capability_scopes) = { capability: "planning" scopes: "cap:read" scopes: "cap:write" };
  option (bones.v1.capability_scopes) = { capability: "legacy" scopes: "cap:read" };
  option (bones.v1.capability_scopes) = { capability: "foreign" scopes: "cap:read" };
  option (bones.v1.capability_scopes) = { capability: "ghostly" scopes: "cap:read" scopes: "cap:ghost" };
  option (bones.v1.capability_scopes) = { capability: "scopeless" };
  option (bones.v1.provided_broker_scopes) = { capability: "planning" scope: "broker:use" };
  option (bones.v1.attributes) = { name: "departure_group" type: ATTRIBUTE_TYPE_STRING polarity: ATTRIBUTE_POLARITY_NARROWABLE matching: ATTRIBUTE_MATCHING_SEGMENT_PREFIX separator: "/" };
  option (bones.v1.attributes) = { name: "channel" type: ATTRIBUTE_TYPE_STRING polarity: ATTRIBUTE_POLARITY_NARROWABLE matching: ATTRIBUTE_MATCHING_EXACT };
  option (bones.v1.attributes) = { name: "departure_id" type: ATTRIBUTE_TYPE_UUID polarity: ATTRIBUTE_POLARITY_NARROWABLE matching: ATTRIBUTE_MATCHING_EXACT };
"#;

const ASK: &str = "approval_mode: APPROVAL_MODE_ASK";
const UUID: &str = "0190f2a4-7b1c-7d3e-8a52-3f6c1d9e4b70";

fn compile(source: &str) -> DescriptorPool {
    let mut compiler = compiler(source.to_owned(), api_bones_protos::files());
    compiler
        .open_file(CONTRACT)
        .unwrap_or_else(|e| panic!("contract failed to compile: {e}"));
    compiler.descriptor_pool()
}

/// A template literal with a label.
fn template(name: &str, grants: &[String]) -> String {
    let grants: String = grants
        .iter()
        .map(|g| format!(" grants: {{ {g} }}"))
        .collect::<Vec<_>>()
        .concat();
    format!("name: \"{name}\" label: {{ text: \"Label\" }}{grants}")
}

/// One service named `name`, declaring `DECLARED`, one RPC requiring
/// `rpc:call`, one requiring each of `cap:read` and `cap:write`, and shipping `templates`.
fn service(name: &str, templates: &[String]) -> String {
    let options: String = templates
        .iter()
        .map(|t| format!("  option (bones.v1.delegation_templates) = {{ {t} }};\n"))
        .collect::<Vec<_>>()
        .concat();
    format!(
        "service {name} {{\n{DECLARED}{options}  rpc Call(Req) returns (Req) {{\n    \
         option (bones.v1.authz) = {{ kind: AUTHZ_KIND_AUTHENTICATED requires: \"rpc:call\" }};\n  }}\n  \
         rpc Read(Req) returns (Req) {{\n    \
         option (bones.v1.authz) = {{ kind: AUTHZ_KIND_AUTHENTICATED requires: \"cap:read\" }};\n  }}\n  \
         rpc Write(Req) returns (Req) {{\n    \
         option (bones.v1.authz) = {{ kind: AUTHZ_KIND_AUTHENTICATED requires: \"cap:write\" }};\n  }}\n  \
         rpc Open(Req) returns (Req);\n}}\n"
    )
}

fn contract(services: &[String]) -> DescriptorPool {
    compile(&format!(
        "syntax = \"proto3\";\npackage p.v1;\nimport \"bones/v1/annotations.proto\";\nimport \"bones/v1/delegation.proto\";\n\
         message Req {{}}\n{}",
        services.join("\n")
    ))
}

fn refusals(pool: &DescriptorPool) -> Vec<Refusal> {
    check_delegation_templates(pool)
        .err()
        .map(|error| error.refusals)
        .unwrap_or_default()
}

/// The `(grant, reason)` pairs of the one template `S.t` made of `grants`.
fn judged(grants: &[&str]) -> Vec<(Option<usize>, Reason)> {
    let grants: Vec<String> = grants.iter().map(|g| (*g).to_owned()).collect();
    let pool = contract(&[service("S", &[template("t", &grants)])]);
    refusals(&pool)
        .into_iter()
        .map(|r| (r.grant, r.reason))
        .collect()
}

fn accepted(grants: &[&str]) {
    assert_eq!(judged(grants), Vec::new(), "{grants:?}");
}

#[test]
fn a_declared_template_reads_back_from_the_descriptor() {
    let grant = format!(
        "scope: \"rpc:call\" selectors: {{ dimension: \"departure_group\" values: \"summer/alps\" }} \
         {ASK} budget: {{ count: 20 window_seconds: 86400 }}"
    );
    let pool = contract(&[service("S", &[template("departure_assistant", &[grant])])]);
    assert_eq!(check_delegation_templates(&pool), Ok(()));

    let extension = pool
        .get_extension_by_name("bones.v1.delegation_templates")
        .expect("the extension is declared");
    let options = pool
        .get_service_by_name("p.v1.S")
        .expect("service")
        .options();
    let value = options.get_extension(&extension);
    let templates = value.as_list().expect("a repeated option");
    assert_eq!(templates.len(), 1);
    let template = templates[0].as_message().expect("a message");
    assert_eq!(
        template.get_field_by_name("name").unwrap().as_str(),
        Some("departure_assistant")
    );
    let grants = template.get_field_by_name("grants").unwrap().into_owned();
    let grant = grants.as_list().unwrap()[0].as_message().unwrap().clone();
    assert_eq!(
        grant.get_field_by_name("scope").unwrap().as_str(),
        Some("rpc:call")
    );
    let budget = grant.get_field_by_name("budget").unwrap().into_owned();
    let budget = budget.as_message().unwrap().clone();
    assert_eq!(
        budget.get_field_by_name("count").unwrap().as_u32(),
        Some(20)
    );
    assert_eq!(
        budget.get_field_by_name("window_seconds").unwrap().as_u32(),
        Some(86400)
    );
}

#[test]
fn a_grant_may_name_a_scope_from_each_source_or_a_group() {
    accepted(&[
        &format!("scope: \"rpc:call\" {ASK}"),
        &format!("scope: \"broker:use\" {ASK}"),
        &format!("scope: \"cap:read\" {ASK}"),
        &format!("scope: \"cap:write\" {ASK}"),
        &format!("group: \"planning\" {ASK}"),
        &format!("group: \"legacy\" {ASK}"),
        "scope: \"rpc:call\" approval_mode: APPROVAL_MODE_PRE_APPROVED",
    ]);
}

#[test]
fn a_selector_takes_literals_of_its_dimension() {
    accepted(&[&format!(
        "scope: \"rpc:call\" {ASK} \
         selectors: {{ dimension: \"departure_group\" values: \"summer/alps\" values: \"x\" }} \
         selectors: {{ dimension: \"channel\" values: \"a/b//c\" }} \
         selectors: {{ dimension: \"departure_id\" values: \"{UUID}\" }} \
         budget: {{ count: 1 window_seconds: 1 }}"
    )]);
}

#[test]
fn a_grant_naming_nothing_is_refused() {
    assert_eq!(judged(&[ASK]), vec![(Some(0), Reason::GrantNamesNothing)]);
}

#[test]
fn an_undeclared_scope_or_group_is_refused() {
    assert_eq!(
        judged(&[&format!("scope: \"nope:x\" {ASK}")]),
        vec![(Some(0), Reason::UndeclaredScope("nope:x".into()))]
    );
    assert_eq!(
        judged(&[&format!("group: \"ghosts\" {ASK}")]),
        vec![(Some(0), Reason::UndeclaredGroup("ghosts".into()))]
    );
}

#[test]
fn a_group_the_service_does_not_provide_or_cannot_confer_is_refused() {
    for group in ["foreign", "ghostly", "scopeless", "legacyx"] {
        assert_eq!(
            judged(&[&format!("group: \"{group}\" {ASK}")]),
            vec![(Some(0), Reason::UndeclaredGroup(group.into()))],
            "{group}"
        );
    }
}

#[test]
fn a_scope_or_group_with_an_unsafe_character_is_refused() {
    // The proto text of the name, and the name it decodes to.
    for (written, decoded) in [
        ("rpc:*", "rpc:*"),
        ("rpc call", "rpc call"),
        (r"rpc\012call", "rpc\ncall"),
        (r"cap\342\200\213:read", "cap\u{200B}:read"),
        (r#"a\"b"#, "a\"b"),
    ] {
        for key in ["scope", "group"] {
            assert_eq!(
                judged(&[&format!("{key}: \"{written}\" {ASK}")]),
                vec![(Some(0), Reason::UnsafeName(decoded.into()))],
                "{key} {written}"
            );
        }
    }
}

#[test]
fn a_refusal_names_the_grant_it_found_it_in() {
    let good = format!("scope: \"rpc:call\" {ASK}");
    let bad = format!("scope: \"nope:x\" {ASK}");
    assert_eq!(
        judged(&[&good, &bad]),
        vec![(Some(1), Reason::UndeclaredScope("nope:x".into()))]
    );
}

#[test]
fn an_undeclared_or_repeated_dimension_is_refused() {
    assert_eq!(
        judged(&[&format!(
            "scope: \"rpc:call\" {ASK} selectors: {{ dimension: \"colour\" values: \"red\" }}"
        )]),
        vec![(Some(0), Reason::UndeclaredDimension("colour".into()))]
    );
    assert_eq!(
        judged(&[&format!(
            "scope: \"rpc:call\" {ASK} \
             selectors: {{ dimension: \"channel\" values: \"a\" }} \
             selectors: {{ dimension: \"channel\" values: \"b\" }}"
        )]),
        vec![(Some(0), Reason::DuplicateDimension("channel".into()))]
    );
}

#[test]
fn a_selector_without_values_is_refused() {
    assert_eq!(
        judged(&[&format!(
            "scope: \"rpc:call\" {ASK} selectors: {{ dimension: \"channel\" }}"
        )]),
        vec![(Some(0), Reason::EmptySelector("channel".into()))]
    );
}

#[test]
fn a_wildcard_value_is_refused_on_every_dimension() {
    for (dimension, value) in [
        ("channel", "a*"),
        ("departure_group", "summer/>"),
        ("channel", "?"),
    ] {
        assert_eq!(
            judged(&[&format!(
                "scope: \"rpc:call\" {ASK} \
                 selectors: {{ dimension: \"{dimension}\" values: \"{value}\" }}"
            )]),
            vec![(
                Some(0),
                Reason::WildcardValue {
                    dimension: dimension.into(),
                    value: value.into(),
                }
            )]
        );
    }
}

#[test]
fn a_value_not_valid_on_its_dimension_is_refused() {
    for (dimension, value) in [
        ("channel", ""),
        ("departure_group", "summer//alps"),
        ("departure_group", "/summer"),
        ("departure_id", "not-a-uuid"),
    ] {
        assert_eq!(
            judged(&[&format!(
                "scope: \"rpc:call\" {ASK} \
                 selectors: {{ dimension: \"{dimension}\" values: \"{value}\" }}"
            )]),
            vec![(
                Some(0),
                Reason::InvalidValue {
                    dimension: dimension.into(),
                    value: value.into(),
                }
            )]
        );
    }
}

#[test]
fn a_value_with_an_unsafe_character_is_refused_on_every_dimension() {
    for (written, decoded) in [
        ("a b", "a b"),
        (r"a\012b", "a\nb"),
        (r#"a\"b"#, "a\"b"),
        ("a(b", "a(b"),
        ("a,b", "a,b"),
        (r"a\\b", "a\\b"),
        (r"a\342\200\256b", "a\u{202E}b"),
    ] {
        for dimension in ["channel", "departure_group"] {
            assert_eq!(
                judged(&[&format!(
                    "scope: \"rpc:call\" {ASK} \
                     selectors: {{ dimension: \"{dimension}\" values: \"{written}\" }}"
                )]),
                vec![(
                    Some(0),
                    Reason::UnsafeValue {
                        dimension: dimension.into(),
                        value: decoded.into(),
                    }
                )],
                "{dimension} {written}"
            );
        }
    }
}

#[test]
fn an_approval_mode_the_enum_does_not_define_is_refused() {
    assert_eq!(
        judged(&["scope: \"rpc:call\" approval_mode: 7"]),
        vec![(Some(0), Reason::UnknownApprovalMode(7))]
    );
}

#[test]
fn an_unspecified_approval_mode_is_refused() {
    assert_eq!(
        judged(&["scope: \"rpc:call\""]),
        vec![(Some(0), Reason::ApprovalUnspecified)]
    );
}

#[test]
fn a_zero_budget_is_refused() {
    for budget in [
        "budget: { count: 0 window_seconds: 5 }",
        "budget: { count: 5 window_seconds: 0 }",
        "budget: {}",
    ] {
        assert_eq!(
            judged(&[&format!("scope: \"rpc:call\" {ASK} {budget}")]),
            vec![(Some(0), Reason::EmptyBudget)],
            "{budget}"
        );
    }
}

#[test]
fn a_template_needs_a_snake_case_name_a_label_and_a_grant() {
    let grant = format!("scope: \"rpc:call\" {ASK}");
    for name in ["Bad-Name", ""] {
        let pool = contract(&[service(
            "S",
            &[template(name, std::slice::from_ref(&grant))],
        )]);
        let found = refusals(&pool);
        assert_eq!(found.len(), 1, "{name:?}");
        assert_eq!(found[0].reason, Reason::InvalidName);
        assert_eq!(found[0].grant, None);
        assert_eq!(found[0].service, "p.v1.S");
        assert_eq!(found[0].template, name);
    }

    for label in ["", "label: { text: \"  \" }"] {
        let source = format!("name: \"t\" {label} grants: {{ {grant} }}");
        let pool = contract(&[service("S", &[source])]);
        assert_eq!(
            refusals(&pool)
                .into_iter()
                .map(|r| r.reason)
                .collect::<Vec<_>>(),
            vec![Reason::MissingLabel],
            "{label:?}"
        );
    }

    let pool = contract(&[service("S", &[template("t", &[])])]);
    assert_eq!(
        refusals(&pool)
            .into_iter()
            .map(|r| r.reason)
            .collect::<Vec<_>>(),
        vec![Reason::NoGrants]
    );
}

#[test]
fn a_name_repeated_within_a_service_is_refused() {
    let grant = format!("scope: \"rpc:call\" {ASK}");
    let pool = contract(&[service(
        "S",
        &[
            template("t", std::slice::from_ref(&grant)),
            template("other", std::slice::from_ref(&grant)),
            template("t", &[grant]),
        ],
    )]);
    let found = refusals(&pool);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].reason, Reason::DuplicateName);
    assert_eq!(found[0].template, "t");
    assert_eq!(found[0].grant, None);
}

#[test]
fn a_name_shared_by_two_services_is_allowed() {
    let grant = format!("scope: \"rpc:call\" {ASK}");
    let pool = contract(&[
        service("A", &[template("t", std::slice::from_ref(&grant))]),
        service("B", &[template("t", &[grant])]),
    ]);
    assert_eq!(check_delegation_templates(&pool), Ok(()));
}

#[test]
fn every_refusal_is_reported_in_one_error() {
    let pool = contract(&[service(
        "S",
        &[template(
            "t",
            &[
                format!("scope: \"nope:x\" {ASK}"),
                "scope: \"rpc:call\"".to_owned(),
            ],
        )],
    )]);
    let error = check_delegation_templates(&pool).expect_err("refused");
    assert_eq!(error.refusals.len(), 2);
    assert_eq!(
        error.to_string(),
        "p.v1.S template `t` grant 0: the scope `nope:x` is not declared by the service\n\
         p.v1.S template `t` grant 1: the approval mode is unspecified"
    );
}

#[test]
fn a_service_shipping_no_template_passes() {
    let pool = contract(&[service("S", &[])]);
    assert_eq!(check_delegation_templates(&pool), Ok(()));
}

#[test]
fn a_pool_without_the_delegation_file_passes() {
    let pool = compile(
        "syntax = \"proto3\";\npackage contract.v1;\nmessage Req {}\n\
         service S { rpc Call(Req) returns (Req); }\n",
    );
    assert!(
        pool.get_extension_by_name("bones.v1.delegation_templates")
            .is_none()
    );
    assert_eq!(check_delegation_templates(&pool), Ok(()));
}

#[test]
fn a_service_without_an_authz_rule_declares_no_scope_from_it() {
    let source = format!(
        "service S {{\n{DECLARED}  option (bones.v1.delegation_templates) = {{ {} }};\n  \
         rpc Call(Req) returns (Req);\n}}\n",
        template("t", &[format!("scope: \"rpc:call\" {ASK}")])
    );
    let pool = contract(&[source]);
    assert_eq!(
        refusals(&pool)
            .into_iter()
            .map(|r| r.reason)
            .collect::<Vec<_>>(),
        vec![Reason::UndeclaredScope("rpc:call".into())]
    );
}

/// A service declaring `DECLARED` plus `extra` options and `rpcs`, shipping one
/// template `t` of `grants`.
fn judged_on(extra: &str, rpcs: &str, grants: &[&str]) -> Vec<Reason> {
    let grants: Vec<String> = grants.iter().map(|g| (*g).to_owned()).collect();
    let source = format!(
        "service S {{\n{DECLARED}{extra}  option (bones.v1.delegation_templates) = {{ {} }};\n{rpcs}}}\n",
        template("t", &grants)
    );
    refusals(&contract(&[source]))
        .into_iter()
        .map(|r| r.reason)
        .collect()
}

#[test]
fn a_scope_only_a_public_or_unspecified_rpc_lists_is_not_declared() {
    let rpcs = "  rpc Pub(Req) returns (Req) {\n    \
                option (bones.v1.authz) = { kind: AUTHZ_KIND_PUBLIC reason: \"open\" requires: \"pub:x\" };\n  }\n  \
                rpc Unset(Req) returns (Req) {\n    \
                option (bones.v1.authz) = { requires: \"unset:x\" };\n  }\n  \
                rpc Call(Req) returns (Req) {\n    \
                option (bones.v1.authz) = { kind: AUTHZ_KIND_AUTHENTICATED requires: \"rpc:call\" };\n  }\n";
    for scope in ["pub:x", "unset:x"] {
        assert_eq!(
            judged_on("", rpcs, &[&format!("scope: \"{scope}\" {ASK}")]),
            vec![Reason::UndeclaredScope(scope.into())],
            "{scope}"
        );
    }
    assert_eq!(
        judged_on("", rpcs, &[&format!("scope: \"rpc:call\" {ASK}")]),
        Vec::new()
    );
}

#[test]
fn a_group_counts_only_when_every_entry_naming_it_is_declared() {
    let rpcs = "  rpc Read(Req) returns (Req) {\n    \
                option (bones.v1.authz) = { kind: AUTHZ_KIND_AUTHENTICATED requires: \"cap:read\" };\n  }\n  \
                rpc Write(Req) returns (Req) {\n    \
                option (bones.v1.authz) = { kind: AUTHZ_KIND_AUTHENTICATED requires: \"cap:write\" };\n  }\n";
    let bad = "  option (bones.v1.capability_scopes) = { capability: \"planning\" scopes: \"cap:ghost\" };\n";
    for extra in [bad.to_owned(), format!("{bad}{bad}")] {
        assert_eq!(
            judged_on(&extra, rpcs, &[&format!("group: \"planning\" {ASK}")]),
            vec![Reason::UndeclaredGroup("planning".into())]
        );
    }
    assert_eq!(
        judged_on("", rpcs, &[&format!("group: \"planning\" {ASK}")]),
        Vec::new()
    );
}

#[test]
fn a_format_character_is_refused_in_a_name_and_a_value_and_a_letter_is_not() {
    // UTF-8 octal escapes of U+061C, U+00AD, U+180E and U+E0041.
    for (written, decoded) in [
        (r"a\330\234b", "a\u{061C}b"),
        (r"a\302\255b", "a\u{00AD}b"),
        (r"a\341\240\216b", "a\u{180E}b"),
        (r"a\363\240\201\201b", "a\u{E0041}b"),
    ] {
        for key in ["scope", "group"] {
            assert_eq!(
                judged(&[&format!("{key}: \"{written}\" {ASK}")]),
                vec![(Some(0), Reason::UnsafeName(decoded.into()))],
                "{key} {written}"
            );
        }
        assert_eq!(
            judged(&[&format!(
                "scope: \"rpc:call\" {ASK} \
                 selectors: {{ dimension: \"channel\" values: \"{written}\" }}"
            )]),
            vec![(
                Some(0),
                Reason::UnsafeValue {
                    dimension: "channel".into(),
                    value: decoded.into(),
                }
            )],
            "{written}"
        );
    }
    accepted(&[&format!(
        "scope: \"rpc:call\" {ASK} \
         selectors: {{ dimension: \"channel\" values: \"caf\\303\\251\" }}"
    )]);
}

#[test]
fn a_value_with_a_wildcard_and_a_hostile_character_is_refused_without_echo() {
    for written in [r"*\033[2J", r"a*\342\200\256b"] {
        let pool = contract(&[service(
            "S",
            &[template(
                "t",
                &[format!(
                    "scope: \"rpc:call\" {ASK} \
                     selectors: {{ dimension: \"channel\" values: \"{written}\" }}"
                )],
            )],
        )]);
        let found = refusals(&pool);
        assert_eq!(found.len(), 1, "{written}");
        assert!(matches!(found[0].reason, Reason::UnsafeValue { .. }));
        let rendered = found[0].to_string();
        assert!(!rendered.chars().any(|c| c.is_control() || c == '\u{202E}'));
    }
}

#[test]
fn a_dimension_name_with_an_unsafe_character_is_refused_as_a_name() {
    for (written, decoded) in [
        (r"chan\033nel", "chan\u{1b}nel"),
        (r"chan\342\200\256nel", "chan\u{202E}nel"),
        ("chan*", "chan*"),
    ] {
        assert_eq!(
            judged(&[&format!(
                "scope: \"rpc:call\" {ASK} selectors: {{ dimension: \"{written}\" values: \"a\" }}"
            )]),
            vec![(Some(0), Reason::UnsafeName(decoded.into()))],
            "{written}"
        );
    }
}

#[test]
fn a_template_named_with_a_control_or_format_character_renders_escaped() {
    for written in [r"a\033b", r"a\342\200\256b"] {
        let grant = format!("scope: \"rpc:call\" {ASK}");
        let source =
            format!("name: \"{written}\" label: {{ text: \"Label\" }} grants: {{ {grant} }}");
        let found = refusals(&contract(&[service("S", &[source])]));
        assert_eq!(found.len(), 1, "{written}");
        assert_eq!(found[0].reason, Reason::InvalidName);
        let rendered = found[0].to_string();
        assert!(
            !rendered.chars().any(|c| c.is_control() || c == '\u{202E}'),
            "{written}"
        );
    }
}

/// The `implied_by` of each `provided_broker_scopes` entry of `p.v1.B`, in
/// declaration order, read back through descriptor reflection.
fn implied_by_of_entries(pool: &DescriptorPool) -> Vec<String> {
    let extension = pool
        .get_extension_by_name("bones.v1.provided_broker_scopes")
        .expect("the extension is declared");
    let options = pool
        .get_service_by_name("p.v1.B")
        .expect("service")
        .options();
    let value = options.get_extension(&extension);
    value
        .as_list()
        .expect("a repeated option")
        .iter()
        .map(|entry| {
            let entry = entry.as_message().expect("a message");
            entry
                .get_field_by_name("implied_by")
                .expect("the field is declared")
                .as_str()
                .expect("a string")
                .to_owned()
        })
        .collect()
}

#[test]
fn a_broker_scope_reads_back_the_capability_that_implies_it() {
    let pool = contract(&[String::from(
        "service B {\n  option (bones.v1.provided_capabilities) = \"events\";\n  \
         option (bones.v1.provided_capabilities) = \"reader\";\n  \
         option (bones.v1.provided_broker_scopes) = { capability: \"events\" scope: \"b:events:t:subscribe:own\" implied_by: \"reader\" };\n  \
         option (bones.v1.provided_broker_scopes) = { capability: \"events\" scope: \"b:events:t:subscribe\" implied_by: \"\" };\n  \
         option (bones.v1.provided_broker_scopes) = { capability: \"events\" scope: \"b:right\" };\n  \
         rpc Open(Req) returns (Req);\n}\n",
    )]);
    assert_eq!(implied_by_of_entries(&pool), ["reader", "", ""]);
}

#[test]
fn an_implied_by_survives_a_wire_round_trip() {
    use prost_reflect::prost::Message as _;
    use prost_reflect::{DynamicMessage, Value};

    let pool = contract(&[String::from(
        "service B {\n  option (bones.v1.provided_capabilities) = \"events\";\n  \
         option (bones.v1.provided_capabilities) = \"reader\";\n  \
         option (bones.v1.provided_broker_scopes) = { capability: \"events\" scope: \"b:events:t:subscribe:own\" implied_by: \"reader\" };\n  \
         rpc Open(Req) returns (Req);\n}\n",
    )]);
    let descriptor = pool
        .get_message_by_name("bones.v1.BrokerScope")
        .expect("BrokerScope is declared");
    let field = descriptor
        .get_field_by_name("implied_by")
        .expect("implied_by is declared");
    assert_eq!(field.number(), 3);

    let mut message = DynamicMessage::new(descriptor.clone());
    message.set_field_by_name("capability", Value::String("events".into()));
    message.set_field_by_name("scope", Value::String("b:events:t:subscribe:own".into()));
    message.set_field_by_name("implied_by", Value::String("reader".into()));
    let decoded = DynamicMessage::decode(descriptor.clone(), message.encode_to_vec().as_slice())
        .expect("decodes");
    assert_eq!(decoded, message);
    assert_eq!(
        decoded.get_field_by_name("implied_by").unwrap().as_str(),
        Some("reader")
    );

    let mut bare = DynamicMessage::new(descriptor.clone());
    bare.set_field_by_name("scope", Value::String("b:right".into()));
    let decoded =
        DynamicMessage::decode(descriptor, bare.encode_to_vec().as_slice()).expect("decodes");
    assert_eq!(
        decoded.get_field_by_name("implied_by").unwrap().as_str(),
        Some("")
    );
}
