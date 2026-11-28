// SPDX-License-Identifier: MIT
//! The protobuf encodings of the in-process boundary.
//!
//! Every value that crosses between a host language and the core is the
//! protobuf binary encoding of a `bones.sdk.v1` message: the failure a call
//! rejects with, a page, a batch chunk, the build info, the spans the core
//! reports and the credential a host answers. Nothing crosses as text. The
//! messages are generated at build time from the protos `api-bones-protos`
//! ships; this module is the only place that names them.

use api_bones_page::{Page, Total, TotalKind};
use buffa::Message as _;
use buffa_types::google::protobuf::{Duration as ProtoDuration, Timestamp};

use crate::BOUNDARY;
use crate::error::{Arg, Error, PageKind};

/// Layer 1 of the boundary: generated at build time from the boundary protos.
#[expect(
    clippy::cast_lossless,
    clippy::elidable_lifetime_names,
    clippy::missing_fields_in_debug,
    clippy::pub_underscore_fields,
    clippy::default_trait_access,
    clippy::redundant_closure_for_method_calls,
    clippy::semicolon_if_nothing_returned,
    clippy::single_match_else,
    clippy::too_many_lines,
    clippy::unreadable_literal,
    clippy::wrong_self_convention,
    reason = "connectrpc and buffa generate this module; a fix here dies at the next codegen run"
)]
mod generated {
    connectrpc::include_generated!();
}

use generated::bones::sdk::v1 as pb;
use generated::bones::v1 as bones;

fn arg(arg: &Arg) -> bones::ErrorArg {
    use bones::error_arg::Value;
    let value = match arg {
        Arg::String(text) => Value::String(text.clone()),
        Arg::Int64(n) => Value::Int64(*n),
        Arg::Double(x) => Value::Double(*x),
        Arg::Timestamp { seconds, nanos } => Value::from(Timestamp {
            seconds: *seconds,
            nanos: *nanos,
            ..Timestamp::default()
        }),
    };
    bones::ErrorArg {
        value: Some(value),
        ..bones::ErrorArg::default()
    }
}

fn page_kind(kind: PageKind) -> pb::page_failure::Kind {
    use pb::page_failure::Kind;
    match kind {
        PageKind::CursorInvalid => Kind::KIND_CURSOR_INVALID,
        PageKind::CursorExpired => Kind::KIND_CURSOR_EXPIRED,
        PageKind::CursorQueryMismatch => Kind::KIND_CURSOR_QUERY_MISMATCH,
        PageKind::Rpc => Kind::KIND_RPC,
    }
}

fn call_error(error: &Error) -> pb::CallError {
    pb::CallError {
        code: error.code.clone(),
        emitter: error.emitter.clone(),
        platform: error.platform.clone(),
        connect_code: error.connect_code.as_wire().to_owned(),
        message: error.message.clone(),
        args: error
            .args
            .iter()
            .map(|(name, value)| (name.clone(), arg(value)))
            .collect(),
        page: error
            .page()
            .map(|detail| pb::PageFailure {
                kind: page_kind(detail.kind).into(),
                delivered_pages: detail.delivered_pages,
                delivered_items: detail.delivered_items,
                ..pb::PageFailure::default()
            })
            .into(),
        retry_after: error.retry_after().map(ProtoDuration::from).into(),
        ..pb::CallError::default()
    }
}

/// The `bones.sdk.v1.CallError` bytes of `error`: the only value an export
/// rejects with and the only failure a handle delivers.
#[must_use]
pub fn encode_call_error(error: &Error) -> Vec<u8> {
    call_error(error).encode_to_vec()
}

/// The error a `bones.sdk.v1.CallError` carries: the identity and arguments a
/// message is rendered from. The page detail and the wait it names are not
/// read back.
///
/// # Errors
/// `invalid_argument` when `bytes` is not the encoding of a `CallError`.
pub fn decode_call_error(bytes: &[u8]) -> Result<Error, Error> {
    use bones::error_arg::Value;
    let wire = pb::CallError::decode_from_slice(bytes)
        .map_err(|error| Error::invalid_argument(format!("the error does not decode: {error}")))?;
    let args = wire
        .args
        .iter()
        .filter_map(|(name, arg)| {
            let value = match arg.value.as_ref()? {
                Value::String(text) => Arg::String(text.clone()),
                Value::Int64(n) => Arg::Int64(*n),
                Value::Double(x) => Arg::Double(*x),
                Value::Timestamp(at) => Arg::Timestamp {
                    seconds: at.seconds,
                    nanos: at.nanos,
                },
            };
            Some((name.clone(), value))
        })
        .collect();
    let mut error = Error::emitted(
        &wire.emitter,
        &wire.code,
        &wire.platform,
        wire.message,
        args,
    );
    if let Ok(code) = wire.connect_code.parse() {
        error.connect_code = code;
    }
    Ok(error)
}

fn total(total: Total) -> bones::Total {
    use bones::total::Kind;
    bones::Total {
        value: total.value,
        kind: match total.kind {
            TotalKind::Estimate => Kind::KIND_ESTIMATE,
            TotalKind::Exact => Kind::KIND_EXACT,
        }
        .into(),
        ..bones::Total::default()
    }
}

/// The `bones.sdk.v1.Page` bytes of `page`; `encode_item` writes one item as
/// its message.
#[must_use]
pub fn encode_page<T>(page: &Page<T>, encode_item: impl Fn(&T) -> Vec<u8>) -> Vec<u8> {
    pb::Page {
        items: page.items.iter().map(encode_item).collect(),
        revision: page.revision.clone(),
        from_cache: page.from_cache,
        has_next: page.has_next,
        total: page.total.map(total).into(),
        ..pb::Page::default()
    }
    .encode_to_vec()
}

/// The `bones.sdk.v1.BatchChunk` bytes of `results`, one outcome per
/// requested id in request order; `encode_item` writes one item as its
/// message.
#[must_use]
pub fn encode_batch_chunk<T>(
    results: &[Result<T, Error>],
    encode_item: impl Fn(&T) -> Vec<u8>,
) -> Vec<u8> {
    use pb::batch_result::Outcome;
    pb::BatchChunk {
        results: results
            .iter()
            .map(|result| pb::BatchResult {
                outcome: Some(match result {
                    Ok(item) => Outcome::Item(encode_item(item)),
                    Err(error) => Outcome::Error(Box::new(call_error(error))),
                }),
                ..pb::BatchResult::default()
            })
            .collect(),
        ..pb::BatchChunk::default()
    }
    .encode_to_vec()
}

/// The `bones.sdk.v1.BuildInfo` bytes of a core named `name` at `version`,
/// stamped with the boundary version it speaks.
#[must_use]
pub fn encode_build_info(name: &str, version: &str) -> Vec<u8> {
    pb::BuildInfo {
        name: name.to_owned(),
        version: version.to_owned(),
        boundary: BOUNDARY,
        ..pb::BuildInfo::default()
    }
    .encode_to_vec()
}

pub use host::{credential_read, span_end, span_start, span_started};

/// The `bones.sdk.v1.FormatResult` bytes of an error's message in a locale:
/// `text` with an optional `hint`.
#[must_use]
pub fn encode_format_text(text: &str, hint: Option<&str>) -> Vec<u8> {
    use pb::format_result::{Formatted, Outcome};
    pb::FormatResult {
        outcome: Some(Outcome::Formatted(Box::new(Formatted {
            text: text.to_owned(),
            hint: hint.map(str::to_owned),
            ..Formatted::default()
        }))),
        ..pb::FormatResult::default()
    }
    .encode_to_vec()
}

/// The `bones.sdk.v1.FormatResult` bytes of a message that was refused by
/// `rule`.
#[must_use]
pub fn encode_format_refused(rule: &str) -> Vec<u8> {
    use pb::format_result::Outcome;
    pb::FormatResult {
        outcome: Some(Outcome::RefusedRule(rule.to_owned())),
        ..pb::FormatResult::default()
    }
    .encode_to_vec()
}

/// What the core says to the host and what the host answers: spans and the
/// credential.
mod host {
    use std::time::{Duration, UNIX_EPOCH};

    use api_bones_batch::{Credential, Identity};
    use buffa::Message as _;
    use buffa_types::google::protobuf::Timestamp;
    use connectrpc::ConnectError;
    use serde_json::Value;

    use super::pb;
    use crate::span::{Attributes, SpanEnd, SpanKind, SpanStart, SpanStarted, SpanStatus};

    /// A Unix-epoch millisecond count as a timestamp; a count that is not a
    /// finite, non-negative instant is the epoch.
    ///
    /// Generic over the field's container: the generated messages hold the
    /// timestamp inline, a plain `MessageField<Timestamp>` boxes it.
    pub(super) fn timestamp<F: From<Option<Timestamp>>>(ms: f64) -> F {
        Some(
            Duration::try_from_secs_f64(ms / 1000.0)
                .ok()
                .and_then(|elapsed| UNIX_EPOCH.checked_add(elapsed))
                .map(Timestamp::from)
                .unwrap_or_default(),
        )
        .into()
    }

    /// A span attribute as the boundary carries it: a number keeps its kind;
    /// a value that is not a scalar crosses as its JSON text.
    pub(super) fn attribute(value: &Value) -> pb::AttributeValue {
        use pb::attribute_value::Value as Wire;
        let wire = match value {
            Value::String(text) => Wire::String(text.clone()),
            Value::Bool(flag) => Wire::Bool(*flag),
            Value::Number(number) => number.as_i64().map_or_else(
                || Wire::Double(number.as_f64().unwrap_or_default()),
                Wire::Int64,
            ),
            other => Wire::String(other.to_string()),
        };
        pb::AttributeValue {
            value: Some(wire),
            ..pb::AttributeValue::default()
        }
    }

    /// The attributes as the map the generated message holds, whatever hasher
    /// that map uses.
    fn attributes<M: FromIterator<(String, pb::AttributeValue)>>(attributes: &Attributes) -> M {
        attributes
            .iter()
            .map(|(name, value)| (name.clone(), attribute(value)))
            .collect()
    }

    /// The `bones.sdk.v1.SpanStart` bytes of `span`.
    #[must_use]
    pub fn span_start(span: &SpanStart) -> Vec<u8> {
        use pb::span_start::Kind;
        pb::SpanStart {
            name: span.name.clone(),
            start_time: timestamp(span.start_time_ms),
            kind: match span.kind {
                SpanKind::Client => Kind::KIND_CLIENT,
                SpanKind::Internal => Kind::KIND_INTERNAL,
            }
            .into(),
            parent: span.parent.clone(),
            tracestate: span.tracestate.clone(),
            attributes: attributes(&span.attributes),
            ..pb::SpanStart::default()
        }
        .encode_to_vec()
    }

    /// The `bones.sdk.v1.SpanEnd` bytes of `end`.
    #[must_use]
    pub fn span_end(end: &SpanEnd) -> Vec<u8> {
        use pb::span_end::Status;
        pb::SpanEnd {
            traceparent: end.traceparent.clone(),
            end_time: timestamp(end.end_time_ms),
            status: match end.status {
                SpanStatus::Ok => Status::STATUS_OK,
                SpanStatus::Error => Status::STATUS_ERROR,
                SpanStatus::Unset => Status::STATUS_UNSET,
            }
            .into(),
            error: end.error.clone(),
            attributes: attributes(&end.attributes),
            ..pb::SpanEnd::default()
        }
        .encode_to_vec()
    }

    /// What a host's sink answered when a span started; `None` when the bytes
    /// are not a `bones.sdk.v1.SpanStarted`.
    #[must_use]
    pub fn span_started(bytes: &[u8]) -> Option<SpanStarted> {
        let answer = pb::SpanStarted::decode_from_slice(bytes).ok()?;
        Some(SpanStarted {
            traceparent: answer.traceparent,
            recording: answer.recording,
        })
    }

    /// What a host answered when the core asked for its credential.
    ///
    /// # Errors
    /// `failed_precondition` when the bytes are not a
    /// `bones.sdk.v1.CredentialAnswer` or the answer names no
    /// credential: nothing is sent as an unknown caller.
    pub fn credential_read(bytes: &[u8]) -> Result<Credential, ConnectError> {
        use pb::credential_answer::Answer;
        let answer = pb::CredentialAnswer::decode_from_slice(bytes).map_err(|error| {
            ConnectError::failed_precondition(format!(
                "the host's credential answer is not a CredentialAnswer: {error}"
            ))
        })?;
        match answer.answer {
            Some(Answer::Credential(credential)) => {
                let mut headers: Vec<_> = credential
                    .headers
                    .iter()
                    .map(|(name, value)| (name.to_ascii_lowercase(), value.as_bytes().to_vec()))
                    .collect();
                headers.sort();
                Ok(Credential {
                    identity: Identity {
                        subject: credential.subject,
                        org: credential.org,
                    },
                    headers,
                })
            }
            None => Err(ConnectError::failed_precondition(
                "the host's credential answer names no credential",
            )),
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, UNIX_EPOCH};

    use api_bones::connect::ConnectCode;
    use buffa::MessageField;
    use connectrpc::{ConnectError, ErrorCode};
    use serde_json::json;

    use super::host::{attribute, timestamp};
    use super::*;
    use crate::span::{Attributes, SpanEnd, SpanKind, SpanStart, SpanStarted, SpanStatus};

    fn decode<M: buffa::Message>(bytes: &[u8]) -> M {
        M::decode_from_slice(bytes).expect("the bytes decode")
    }

    fn page(items: Vec<u32>, total: Option<Total>) -> Page<u32> {
        Page {
            from_cache: true,
            has_next: true,
            items: Arc::from(items),
            revision: "rev-1".to_owned(),
            total,
        }
    }

    /// `error` carrying one argument of each kind a call error names.
    fn with_every_kind_of_argument(mut error: Error) -> Error {
        error.args.insert("name".to_owned(), Arg::from("a"));
        error.args.insert("n".to_owned(), Arg::from(7_i64));
        error.args.insert("x".to_owned(), Arg::from(1.5));
        error.args.insert(
            "at".to_owned(),
            Arg::from(UNIX_EPOCH + Duration::new(10, 5)),
        );
        error
    }

    #[test]
    fn a_call_error_decodes_to_the_identity_and_arguments_it_was_encoded_from() {
        let error = with_every_kind_of_argument(Error::emitted(
            "pay",
            "GRANT_MISSING",
            "FORBIDDEN",
            "no grant",
            std::collections::BTreeMap::new(),
        ));
        let back = decode_call_error(&encode_call_error(&error)).expect("decodes");
        assert_eq!(back, error);
    }

    #[test]
    fn a_call_error_with_an_unknown_connect_code_keeps_the_platform_mapping_and_an_empty_argument_is_dropped()
     {
        let wire = pb::CallError {
            platform: "RESOURCE_NOT_FOUND".to_owned(),
            connect_code: "bogus".to_owned(),
            args: [("empty".to_owned(), bones::ErrorArg::default())]
                .into_iter()
                .collect(),
            ..pb::CallError::default()
        };
        let back = decode_call_error(&wire.encode_to_vec()).expect("decodes");
        assert_eq!(back.connect_code, ConnectCode::NotFound);
        assert!(back.args.is_empty());
    }

    #[test]
    fn bytes_that_are_no_call_error_are_refused() {
        let error = decode_call_error(&[0x0a]).expect_err("truncated");
        assert!(error.message.contains("does not decode"), "{error}");
    }

    #[test]
    fn a_call_error_crosses_with_every_member_of_the_error() {
        let error = with_every_kind_of_argument(
            Error::local(ConnectCode::NotFound, "WIDGET_MISSING", "NOT_FOUND", "gone").on_page(
                PageKind::CursorExpired,
                2,
                40,
            ),
        );
        let wire: pb::CallError = decode(&encode_call_error(&error));
        assert_eq!(wire.code, "WIDGET_MISSING");
        assert_eq!(wire.emitter, error.emitter);
        assert_eq!(wire.platform, "NOT_FOUND");
        assert_eq!(wire.connect_code, "not_found");
        assert_eq!(wire.message, "gone");
        assert_eq!(wire.args.len(), 4);
        assert_eq!(
            wire.args["name"].value,
            Some(bones::error_arg::Value::String("a".to_owned()))
        );
        assert_eq!(
            wire.args["n"].value,
            Some(bones::error_arg::Value::Int64(7))
        );
        assert_eq!(
            wire.args["x"].value,
            Some(bones::error_arg::Value::Double(1.5))
        );
        let Some(bones::error_arg::Value::Timestamp(at)) = &wire.args["at"].value else {
            panic!("the timestamp argument keeps its kind");
        };
        assert_eq!((at.seconds, at.nanos), (10, 5));
        let page = wire.page.as_option().expect("a failed list names its page");
        assert_eq!(page.kind, pb::page_failure::Kind::KIND_CURSOR_EXPIRED);
        assert_eq!((page.delivered_pages, page.delivered_items), (2, 40));
        assert!(wire.retry_after.is_unset());
    }

    #[test]
    fn a_call_error_carries_the_wait_the_server_named_and_no_page_otherwise() {
        let mut error = Error::handle_busy();
        assert!(
            decode::<pb::CallError>(&encode_call_error(&error))
                .page
                .is_unset()
        );
        error = Error::from(
            ConnectError::resource_exhausted("slow down").with_headers(retry_after_headers("3")),
        );
        let wire: pb::CallError = decode(&encode_call_error(&error));
        let wait = wire.retry_after.as_option().expect("the wait crosses");
        assert_eq!((wait.seconds, wait.nanos), (3, 0));
        assert_eq!(wire.connect_code, "resource_exhausted");
    }

    fn retry_after_headers(seconds: &str) -> http::HeaderMap {
        let mut headers = http::HeaderMap::new();
        headers.insert("retry-after", seconds.parse().unwrap());
        headers
    }

    #[test]
    fn every_page_kind_has_its_own_wire_kind() {
        use pb::page_failure::Kind;
        for (kind, wire) in [
            (PageKind::CursorInvalid, Kind::KIND_CURSOR_INVALID),
            (PageKind::CursorExpired, Kind::KIND_CURSOR_EXPIRED),
            (
                PageKind::CursorQueryMismatch,
                Kind::KIND_CURSOR_QUERY_MISMATCH,
            ),
            (PageKind::Rpc, Kind::KIND_RPC),
        ] {
            assert_eq!(page_kind(kind), wire);
        }
    }

    #[test]
    fn a_page_crosses_as_its_encoded_items_and_its_cache_facts() {
        let wire: pb::Page = decode(&encode_page(&page(vec![1, 2], None), |n| {
            n.to_be_bytes().to_vec()
        }));
        assert_eq!(wire.items, [vec![0, 0, 0, 1], vec![0, 0, 0, 2]]);
        assert_eq!(wire.revision, "rev-1");
        assert!(wire.from_cache);
        assert!(wire.has_next);
        assert!(wire.total.is_unset());
    }

    #[test]
    fn a_page_total_keeps_its_value_and_how_it_was_obtained() {
        use bones::total::Kind;
        for (kind, wire) in [
            (TotalKind::Estimate, Kind::KIND_ESTIMATE),
            (TotalKind::Exact, Kind::KIND_EXACT),
        ] {
            let total = Total { value: 9, kind };
            let encoded = decode::<pb::Page>(&encode_page(&page(vec![], Some(total)), |n| {
                n.to_be_bytes().to_vec()
            }));
            let got = encoded.total.as_option().expect("the total crosses");
            assert_eq!(got.value, 9);
            assert_eq!(got.kind, wire);
        }
    }

    #[test]
    fn a_batch_chunk_holds_one_outcome_per_id_in_order() {
        use pb::batch_result::Outcome;
        let results = [Ok(5_u32), Err(Error::canceled()), Ok(6)];
        let wire: pb::BatchChunk =
            decode(&encode_batch_chunk(&results, |n| n.to_be_bytes().to_vec()));
        assert_eq!(wire.results.len(), 3);
        assert_eq!(
            wire.results[0].outcome,
            Some(Outcome::Item(vec![0, 0, 0, 5]))
        );
        let Some(Outcome::Error(error)) = &wire.results[1].outcome else {
            panic!("a failed id crosses as its error");
        };
        assert_eq!(error.code, crate::CANCELED);
        assert_eq!(
            wire.results[2].outcome,
            Some(Outcome::Item(vec![0, 0, 0, 6]))
        );
    }

    #[test]
    fn the_build_info_stamps_the_boundary_version() {
        let wire: pb::BuildInfo = decode(&encode_build_info("core", "1.2.3"));
        assert_eq!(wire.name, "core");
        assert_eq!(wire.version, "1.2.3");
        assert_eq!(wire.boundary, BOUNDARY);
    }

    #[test]
    fn a_formatted_message_carries_its_text_and_hint() {
        let wire: pb::FormatResult = decode(&encode_format_text("bonjour", Some("essayez")));
        let Some(pb::format_result::Outcome::Formatted(formatted)) = wire.outcome else {
            panic!("the outcome is a formatted message");
        };
        assert_eq!(formatted.text, "bonjour");
        assert_eq!(formatted.hint.as_deref(), Some("essayez"));
        let wire: pb::FormatResult = decode(&encode_format_text("hello", None));
        let Some(pb::format_result::Outcome::Formatted(formatted)) = wire.outcome else {
            panic!("the outcome is a formatted message");
        };
        assert_eq!(formatted.hint, None);
    }

    #[test]
    fn a_refused_format_names_its_rule() {
        let wire: pb::FormatResult = decode(&encode_format_refused("locale"));
        assert!(matches!(
            wire.outcome,
            Some(pb::format_result::Outcome::RefusedRule(rule)) if rule == "locale"
        ));
    }

    #[test]
    fn a_span_start_carries_its_parent_its_kind_and_its_attributes() {
        let mut attributes = Attributes::new();
        attributes.insert("rpc.system".to_owned(), json!("connect_rpc"));
        let start = SpanStart {
            name: "pkg.Svc/Get".to_owned(),
            start_time_ms: 1500.0,
            kind: SpanKind::Client,
            parent: Some("00-parent".to_owned()),
            tracestate: Some("k=v".to_owned()),
            attributes,
        };
        let wire: pb::SpanStart = decode(&span_start(&start));
        assert_eq!(wire.name, "pkg.Svc/Get");
        let at = wire.start_time.as_option().expect("the start time crosses");
        assert_eq!((at.seconds, at.nanos), (1, 500_000_000));
        assert_eq!(wire.kind, pb::span_start::Kind::KIND_CLIENT);
        assert_eq!(wire.parent.as_deref(), Some("00-parent"));
        assert_eq!(wire.tracestate.as_deref(), Some("k=v"));
        assert_eq!(
            wire.attributes["rpc.system"].value,
            Some(pb::attribute_value::Value::String("connect_rpc".to_owned()))
        );
    }

    #[test]
    fn a_root_internal_span_start_names_no_parent() {
        let start = SpanStart {
            name: "walk".to_owned(),
            start_time_ms: 0.0,
            kind: SpanKind::Internal,
            parent: None,
            tracestate: None,
            attributes: Attributes::new(),
        };
        let wire: pb::SpanStart = decode(&span_start(&start));
        assert_eq!(wire.kind, pb::span_start::Kind::KIND_INTERNAL);
        assert_eq!(wire.parent, None);
        assert_eq!(wire.tracestate, None);
        assert!(wire.attributes.is_empty());
    }

    #[test]
    fn an_instant_that_is_not_a_finite_count_is_the_epoch() {
        for ms in [f64::NAN, f64::INFINITY, -1.0, f64::MAX] {
            let at: MessageField<Timestamp> = timestamp(ms);
            let at = at.as_option().expect("a timestamp is always set");
            assert_eq!((at.seconds, at.nanos), (0, 0), "{ms}");
        }
    }

    #[test]
    fn an_attribute_keeps_its_kind_and_a_non_scalar_crosses_as_text() {
        use pb::attribute_value::Value as Wire;
        for (value, wire) in [
            (json!("text"), Wire::String("text".to_owned())),
            (json!(true), Wire::Bool(true)),
            (json!(-3), Wire::Int64(-3)),
            (json!(u64::MAX), Wire::Double(1.844_674_407_370_955_2e19)),
            (json!(2.5), Wire::Double(2.5)),
            (json!(null), Wire::String("null".to_owned())),
            (json!([1, 2]), Wire::String("[1,2]".to_owned())),
        ] {
            assert_eq!(attribute(&value).value, Some(wire), "{value}");
        }
    }

    #[test]
    fn a_span_end_carries_its_status_its_error_and_its_attributes() {
        let mut attributes = Attributes::new();
        attributes.insert("pages".to_owned(), json!(3));
        for (status, wire) in [
            (SpanStatus::Ok, pb::span_end::Status::STATUS_OK),
            (SpanStatus::Error, pb::span_end::Status::STATUS_ERROR),
            (SpanStatus::Unset, pb::span_end::Status::STATUS_UNSET),
        ] {
            let end = SpanEnd {
                traceparent: "00-span".to_owned(),
                end_time_ms: 2000.0,
                status,
                error: Some("unavailable SERVICE_UNAVAILABLE".to_owned()),
                attributes: attributes.clone(),
            };
            let decoded: pb::SpanEnd = decode(&span_end(&end));
            assert_eq!(decoded.traceparent, "00-span");
            assert_eq!(decoded.status, wire);
            assert_eq!(
                decoded.error.as_deref(),
                Some("unavailable SERVICE_UNAVAILABLE")
            );
            let at = decoded.end_time.as_option().expect("the end time crosses");
            assert_eq!(at.seconds, 2);
            assert_eq!(
                decoded.attributes["pages"].value,
                Some(pb::attribute_value::Value::Int64(3))
            );
        }
        let ok = SpanEnd {
            traceparent: String::new(),
            end_time_ms: 1.0,
            status: SpanStatus::Ok,
            error: None,
            attributes: Attributes::new(),
        };
        let decoded: pb::SpanEnd = decode(&span_end(&ok));
        assert_eq!(decoded.error, None);
        assert!(decoded.attributes.is_empty());
    }

    #[test]
    fn a_sinks_answer_is_read_and_unreadable_bytes_are_none() {
        let recording = pb::SpanStarted {
            traceparent: "00-tp".to_owned(),
            recording: true,
            ..pb::SpanStarted::default()
        };
        assert_eq!(
            span_started(&recording.encode_to_vec()),
            Some(SpanStarted::recording("00-tp"))
        );
        assert_eq!(span_started(&[]), Some(SpanStarted::default()));
        assert_eq!(span_started(&[0xff, 0xff]), None);
    }

    fn answer(answer: Option<pb::credential_answer::Answer>) -> Vec<u8> {
        pb::CredentialAnswer {
            answer,
            ..pb::CredentialAnswer::default()
        }
        .encode_to_vec()
    }

    #[test]
    fn a_credential_answer_gives_the_identity_and_the_headers_in_name_order() {
        use pb::credential_answer::Answer;
        let credential = pb::Credential {
            subject: "user-1".to_owned(),
            org: "org-1".to_owned(),
            headers: [
                ("Brefwiz-Relayed-Credential".to_owned(), "relay".to_owned()),
                ("authorization".to_owned(), "Bearer t".to_owned()),
            ]
            .into_iter()
            .collect(),
            ..pb::Credential::default()
        };
        let credential = credential_read(&answer(Some(Answer::Credential(Box::new(credential)))))
            .expect("a credential reads");
        assert_eq!(credential.identity.subject, "user-1");
        assert_eq!(credential.identity.org, "org-1");
        assert_eq!(
            credential.headers,
            [
                ("authorization".to_owned(), b"Bearer t".to_vec()),
                ("brefwiz-relayed-credential".to_owned(), b"relay".to_vec()),
            ]
        );
    }

    #[test]
    fn an_answer_without_a_credential_or_that_is_not_an_answer_fails_closed() {
        for bytes in [answer(None), vec![0xff, 0xff]] {
            let error = credential_read(&bytes).expect_err("nothing is sent as an unknown caller");
            assert_eq!(error.code, ErrorCode::FailedPrecondition);
        }
    }
}
