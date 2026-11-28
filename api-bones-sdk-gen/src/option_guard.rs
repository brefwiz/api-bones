// SPDX-License-Identifier: MIT
//! The refusal every wrapper applies to a client's options message: options
//! carry no transport trust and never name the server.
//!
//! A caller chooses the page size or a tenant through options; the roots,
//! pins and verification the transport trusts come from the platform and the
//! host, never from a value the application passes. The check reads the
//! declared proto, so it holds for a message however deeply it nests another.

use std::collections::BTreeSet;

use anyhow::bail;
use prost_reflect::{EnumDescriptor, FieldDescriptor, Kind, MessageDescriptor};

/// Name fragments that mark an option as transport trust.
const TRUST_FRAGMENTS: [&str; 9] = [
    "root",
    "cert",
    "pin",
    "verif",
    "tls",
    "insecure",
    "trust",
    "dangerous",
    "pem",
];

/// Whole words (split on `_`) that mark an option as transport trust; too
/// short to match as a fragment without refusing `cache` or `cancel`.
const TRUST_WORDS: [&str; 1] = ["ca"];

/// Names, with `_` removed, that stand for the server's address.
const ADDRESS_NAMES: [&str; 10] = [
    "baseurl",
    "baseuri",
    "serverurl",
    "url",
    "uri",
    "host",
    "hostname",
    "authority",
    "endpoint",
    "origin",
];

/// Refuses an options message that carries transport trust or the server's
/// address, directly or through a message-typed field, a repeated message or
/// the value message of a map.
///
/// A field is refused when its name, or the full name of the message it
/// or the enum it holds, contains `root`, `cert`, `pin`, `verif`, `tls`,
/// `insecure`, `trust`, `dangerous` or `pem` in any case, has `ca` as a
/// word, or when the field is the base URL, URL, URI, host, host name,
/// authority, endpoint or origin.
///
/// # Errors
///
/// The path of the first offending field, from the options message down.
pub(crate) fn check_options(options: &MessageDescriptor) -> anyhow::Result<()> {
    let mut visiting = BTreeSet::new();
    walk(options, options.name(), &mut visiting)
}

fn walk(
    message: &MessageDescriptor,
    path: &str,
    visiting: &mut BTreeSet<String>,
) -> anyhow::Result<()> {
    if !visiting.insert(message.full_name().to_owned()) {
        return Ok(());
    }
    for field in message.fields() {
        let here = format!("{path}.{}", field.name());
        if let Some(reason) = refused_name(field.name()) {
            bail!(
                "option `{here}` {reason}: options carry no transport trust and never name the server"
            );
        }
        if let Some(held) = held_enum(&field) {
            if let Some(reason) = refused_name(held.full_name()) {
                bail!(
                    "option `{here}` holds `{}`, which {reason}: options carry no transport trust and never name the server",
                    held.full_name()
                );
            }
        }
        if let Some(held) = held_message(&field) {
            if let Some(reason) = refused_name(held.full_name()) {
                bail!(
                    "option `{here}` holds `{}`, which {reason}: options carry no transport trust and never name the server",
                    held.full_name()
                );
            }
            walk(&held, &here, visiting)?;
        }
    }
    visiting.remove(message.full_name());
    Ok(())
}

/// The message a field holds, the value message for a map.
fn held_message(field: &FieldDescriptor) -> Option<MessageDescriptor> {
    match field.kind() {
        Kind::Message(entry) if field.is_map() => match entry.map_entry_value_field().kind() {
            Kind::Message(value) => Some(value),
            _ => None,
        },
        Kind::Message(message) => Some(message),
        _ => None,
    }
}

/// The enum a field holds, the value enum for a map.
fn held_enum(field: &FieldDescriptor) -> Option<EnumDescriptor> {
    match field.kind() {
        Kind::Enum(held) => Some(held),
        Kind::Message(entry) if field.is_map() => match entry.map_entry_value_field().kind() {
            Kind::Enum(held) => Some(held),
            _ => None,
        },
        _ => None,
    }
}

/// Why `name` is refused, when it is. A full name names the server's
/// address when its last segment does.
fn refused_name(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    let last = lower.rsplit('.').next().unwrap_or(&lower);
    let squeezed: String = last.chars().filter(|c| *c != '_').collect();
    if ADDRESS_NAMES.contains(&squeezed.as_str()) {
        return Some("names the server's address");
    }
    if TRUST_FRAGMENTS
        .iter()
        .any(|fragment| lower.contains(fragment))
        || lower
            .split(['_', '.'])
            .any(|word| TRUST_WORDS.contains(&word))
    {
        return Some("names transport trust");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wrapper_ts::testkit::compile;

    fn options(body: &str) -> MessageDescriptor {
        let proto = format!("syntax = \"proto3\";\npackage t.v1;\n{body}\n");
        compile(&proto).message("t.v1.Options").expect("a message")
    }

    fn refusal(body: &str) -> String {
        check_options(&options(body))
            .expect_err("refused")
            .to_string()
    }

    #[test]
    fn a_page_size_is_allowed() {
        check_options(&options(
            "message Options { optional uint32 default_page_size = 1; string tenant = 2; }",
        ))
        .expect("allowed");
    }

    #[test]
    fn a_plain_ca_bundle_is_refused() {
        assert!(
            refusal("message Options { bytes ca_pem = 1; }").contains("`Options.ca_pem`"),
            "named by its path"
        );
    }

    #[test]
    fn every_trust_fragment_is_refused_in_any_case() {
        for name in [
            "root_store",
            "server_cert",
            "pinned_key",
            "verify_hostname",
            "use_tls",
            "insecure_mode",
            "trusted",
            "dangerous_flag",
            "ca",
            "CertChain",
        ] {
            let body = format!("message Options {{ string {name} = 1; }}");
            assert!(
                refusal(&body).contains("transport trust"),
                "`{name}` is refused"
            );
        }
    }

    #[test]
    fn the_base_url_and_the_host_are_refused() {
        for name in [
            "base_url",
            "baseUrl",
            "host",
            "authority",
            "url",
            "uri",
            "base_uri",
            "endpoint",
            "hostname",
            "origin",
            "server_url",
        ] {
            let body = format!("message Options {{ string {name} = 1; }}");
            assert!(
                refusal(&body).contains("server's address"),
                "`{name}` is refused"
            );
        }
    }

    #[test]
    fn words_that_only_contain_ca_are_allowed() {
        check_options(&options(
            "message Options { bool cache = 1; bool cancel_on_drop = 2; }",
        ))
        .expect("allowed");
    }

    #[test]
    fn a_nested_message_is_searched_and_the_path_names_it() {
        let text = refusal(
            "message Options { Settings transport = 1; }\nmessage Settings { bytes ca_pem = 1; }",
        );
        assert!(text.contains("`Options.transport.ca_pem`"), "{text}");
    }

    #[test]
    fn a_message_type_named_for_tls_is_refused_whatever_its_fields_are() {
        let text = refusal(
            "message Options { TlsSettings transport = 1; }\nmessage TlsSettings { uint32 level = 1; }",
        );
        assert!(text.contains("t.v1.TlsSettings"), "{text}");
    }

    #[test]
    fn the_value_message_of_a_map_and_a_repeated_message_are_searched() {
        let text = refusal(
            "message Options { map<string, Peer> peers = 1; }\nmessage Peer { bytes ca_pem = 1; }",
        );
        assert!(text.contains("`Options.peers.ca_pem`"), "{text}");
        let text = refusal(
            "message Options { repeated Peer peers = 1; }\nmessage Peer { bytes ca_pem = 1; }",
        );
        assert!(text.contains("`Options.peers.ca_pem`"), "{text}");
    }

    #[test]
    fn a_map_of_scalars_and_a_recursive_message_are_allowed() {
        check_options(&options(
            "message Options { map<string, string> labels = 1; Node tree = 2; }\nmessage Node { string name = 1; Node next = 2; }",
        ))
        .expect("allowed");
    }

    #[test]
    fn a_recursive_message_still_reveals_a_later_offender() {
        let text = refusal(
            "message Options { Node tree = 1; }\nmessage Node { Node next = 1; bytes ca_pem = 2; }",
        );
        assert!(text.contains("ca_pem"), "{text}");
    }

    #[test]
    fn an_enum_typed_field_is_refused_by_its_enum_name() {
        let text = refusal(
            "message Options { TlsVersion min = 1; }\nenum TlsVersion { TLS_VERSION_UNSPECIFIED = 0; }",
        );
        assert!(text.contains("t.v1.TlsVersion"), "{text}");
        assert!(text.contains("transport trust"), "{text}");
    }

    #[test]
    fn the_value_enum_of_a_map_is_refused_by_its_enum_name() {
        let text = refusal(
            "message Options { map<string, VerifyMode> modes = 1; }\nenum VerifyMode { VERIFY_MODE_UNSPECIFIED = 0; }",
        );
        assert!(text.contains("t.v1.VerifyMode"), "{text}");
    }

    #[test]
    fn an_enum_typed_field_named_for_an_address_is_refused() {
        let text = refusal(
            "message Options { Endpoint site = 1; }\nenum Endpoint { ENDPOINT_UNSPECIFIED = 0; }",
        );
        assert!(text.contains("server's address"), "{text}");
    }

    #[test]
    fn an_enum_with_a_plain_name_is_allowed() {
        check_options(&options(
            "message Options { Mode mode = 1; map<string, Mode> modes = 2; }\nenum Mode { MODE_UNSPECIFIED = 0; }",
        ))
        .expect("allowed");
    }
}
