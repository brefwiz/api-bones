// SPDX-License-Identifier: MIT
//! The spans a Layer 2 call opens, reported live to a host sink.
//!
//! The core reports a span when it starts and again when it ends. Nothing is
//! buffered or replayed: a sink sees `start` as the call begins and `end` as
//! it settles. A span whose parent is missing or unreadable is a root.
//!
//! The trace context a call sends is always the core's own: the `traceparent`
//! the sink returned from `start`, or one the core mints when it has no sink
//! or the sink is not recording.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use api_bones_rt::{MaybeSend, MaybeSync, fill_random, system_now};
use serde_json::{Map, Value, json};

use crate::ctx::CallCtx;
use crate::error::Error;

/// The attributes of a span: names to JSON scalars.
pub type Attributes = BTreeMap<String, Value>;

/// The W3C `traceparent` of a span: version `00`, a trace id, a span id and
/// the trace flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Traceparent {
    trace_id: [u8; 16],
    span_id: [u8; 8],
    flags: u8,
}

/// The flags of a trace the core starts: sampled.
const SAMPLED: u8 = 0x01;

fn nibble(digit: u8) -> u8 {
    match digit {
        b'0'..=b'9' => digit - b'0',
        _ => digit - b'a' + 10,
    }
}

/// The bytes of `text`, two lower-case hex digits each.
fn hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    if text.len() != 2 * N || !text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    let mut bytes = [0_u8; N];
    for (slot, pair) in bytes.iter_mut().zip(text.as_bytes().chunks(2)) {
        *slot = (nibble(pair[0]) << 4) | nibble(pair[1]);
    }
    Some(bytes)
}

/// Eight or sixteen random bytes that are not all zero.
fn random_id<const N: usize>(fill: &mut impl FnMut(&mut [u8])) -> [u8; N] {
    let mut id = [0_u8; N];
    while id == [0_u8; N] {
        fill(&mut id);
    }
    id
}

impl Traceparent {
    /// Reads version `00` text: `00-<32 hex>-<16 hex>-<2 hex>`, lower-case,
    /// with neither id all zero.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('-');
        let (Some("00"), Some(trace), Some(span), Some(flags), None) = (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        ) else {
            return None;
        };
        let trace_id = hex::<16>(trace).filter(|id| *id != [0; 16])?;
        let span_id = hex::<8>(span).filter(|id| *id != [0; 8])?;
        let [flags] = hex::<1>(flags)?;
        Some(Self {
            trace_id,
            span_id,
            flags,
        })
    }

    /// A new span: in the trace of `parent` with its flags, or the root of a
    /// new sampled trace. Ids come from the platform's secure random source.
    #[must_use]
    pub fn mint(parent: Option<&Self>) -> Self {
        Self::mint_with(parent, fill_random)
    }

    fn mint_with(parent: Option<&Self>, mut fill: impl FnMut(&mut [u8])) -> Self {
        Self {
            trace_id: parent.map_or_else(|| random_id(&mut fill), |p| p.trace_id),
            span_id: random_id(&mut fill),
            flags: parent.map_or(SAMPLED, |p| p.flags),
        }
    }

    /// The trace id as 32 lower-case hex digits.
    #[must_use]
    pub fn trace_id(&self) -> String {
        Hex(&self.trace_id).to_string()
    }
}

/// Bytes as lower-case hex digits.
struct Hex<'a>(&'a [u8]);

impl fmt::Display for Hex<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}

impl fmt::Display for Traceparent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "00-{}-{}-{:02x}",
            Hex(&self.trace_id),
            Hex(&self.span_id),
            self.flags
        )
    }
}

/// Which side of a call a span covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanKind {
    /// One RPC the client sends.
    Client,
    /// Work inside the client, such as a walk over the pages of a list.
    Internal,
}

impl SpanKind {
    /// The `kind` of the span's JSON.
    #[must_use]
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::Client => "client",
            Self::Internal => "internal",
        }
    }
}

/// How a span ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanStatus {
    /// The work succeeded.
    Ok,
    /// The work failed.
    Error,
    /// The work was abandoned or its outcome is not known.
    Unset,
}

impl SpanStatus {
    /// The `status` of the span's JSON.
    #[must_use]
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Error => "error",
            Self::Unset => "unset",
        }
    }
}

/// What the core reports when it opens a span.
#[derive(Debug, Clone, PartialEq)]
pub struct SpanStart {
    /// `<package.Service>/<Method>` for an RPC, a descriptive name otherwise.
    pub name: String,
    /// When the span began, in milliseconds since the Unix epoch.
    pub start_time_ms: f64,
    /// The side of the call the span covers.
    pub kind: SpanKind,
    /// The parent's `traceparent`; absent for a new root.
    pub parent: Option<String>,
    /// The parent's `tracestate`.
    pub tracestate: Option<String>,
    /// The span's attributes.
    pub attributes: Attributes,
}

fn with_attributes(mut wire: Value, attributes: &Attributes) -> Value {
    if !attributes.is_empty() {
        let members: Map<String, Value> = attributes
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        wire["attributes"] = Value::Object(members);
    }
    wire
}

impl SpanStart {
    /// The JSON the host's `start` receives.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let mut wire = json!({
            "name": self.name,
            "startTime": self.start_time_ms,
            "kind": self.kind.wire_name(),
        });
        if let Some(parent) = &self.parent {
            wire["parent"] = json!(parent);
        }
        if let Some(tracestate) = &self.tracestate {
            wire["tracestate"] = json!(tracestate);
        }
        with_attributes(wire, &self.attributes)
    }
}

/// What the core reports when it ends a span the sink started.
#[derive(Debug, Clone, PartialEq)]
pub struct SpanEnd {
    /// The value `start` returned.
    pub traceparent: String,
    /// When the span ended, in milliseconds since the Unix epoch.
    pub end_time_ms: f64,
    /// How the span ended.
    pub status: SpanStatus,
    /// `<connectCode> <code>`, set when `status` is `Error`.
    pub error: Option<String>,
    /// Attributes known only at the end.
    pub attributes: Attributes,
}

impl SpanEnd {
    /// The JSON the host's `end` receives.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let mut wire = json!({
            "traceparent": self.traceparent,
            "endTime": self.end_time_ms,
            "status": self.status.wire_name(),
        });
        if let Some(error) = &self.error {
            wire["error"] = json!(error);
        }
        with_attributes(wire, &self.attributes)
    }
}

/// The host that records the core's spans, live.
pub trait SpanSink: MaybeSend + MaybeSync {
    /// A span began. Returns the new span's `traceparent`, or `None` when the
    /// span is not recording; then `end` is never called for it.
    fn start(&self, span: &SpanStart) -> Option<String>;

    /// A span the sink started ended.
    fn end(&self, end: &SpanEnd);
}

/// The wall clock in milliseconds since the Unix epoch.
fn now_ms() -> f64 {
    system_now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |elapsed| elapsed.as_secs_f64() * 1000.0)
}

/// Opens spans against an optional sink.
#[derive(Clone, Default)]
pub struct Tracer {
    sink: Option<Arc<dyn SpanSink>>,
}

impl fmt::Debug for Tracer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tracer")
            .field("sink", &self.sink.is_some())
            .finish()
    }
}

impl Tracer {
    /// A tracer that reports to `sink`.
    #[must_use]
    pub fn new(sink: Arc<dyn SpanSink>) -> Self {
        Self { sink: Some(sink) }
    }

    /// Opens a span under `parent`: the call's caller or an enclosing span.
    /// The `tracestate` follows only a readable parent.
    #[must_use]
    pub fn start(
        &self,
        name: &str,
        kind: SpanKind,
        parent: &CallCtx,
        attributes: Attributes,
    ) -> ActiveSpan {
        let parent_id = parent.traceparent.as_deref().and_then(Traceparent::parse);
        let tracestate = parent_id.and(parent.tracestate.clone());
        let recorded = self.sink.as_ref().and_then(|sink| {
            let begin = SpanStart {
                name: name.to_owned(),
                start_time_ms: now_ms(),
                kind,
                parent: parent_id.map(|id| id.to_string()),
                tracestate: tracestate.clone(),
                attributes,
            };
            sink.start(&begin).map(|host| (Arc::clone(sink), host))
        });
        let traceparent = recorded
            .as_ref()
            .and_then(|(_, host)| Traceparent::parse(host))
            .unwrap_or_else(|| Traceparent::mint(parent_id.as_ref()));
        ActiveSpan {
            traceparent,
            tracestate,
            recorded,
            ended: false,
        }
    }
}

/// An open span. It ends once: by [`ActiveSpan::end`], [`ActiveSpan::end_with`],
/// [`ActiveSpan::fail`], or, when dropped first, with status `unset`.
pub struct ActiveSpan {
    traceparent: Traceparent,
    tracestate: Option<String>,
    recorded: Option<(Arc<dyn SpanSink>, String)>,
    ended: bool,
}

impl fmt::Debug for ActiveSpan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActiveSpan")
            .field("traceparent", &self.traceparent.to_string())
            .field("tracestate", &self.tracestate)
            .field("recorded", &self.recorded.is_some())
            .field("ended", &self.ended)
            .finish()
    }
}

impl ActiveSpan {
    /// The `traceparent` a request under this span carries.
    #[must_use]
    pub fn traceparent(&self) -> String {
        self.traceparent.to_string()
    }

    /// The `tracestate` a request under this span carries, when it has one.
    #[must_use]
    pub fn tracestate(&self) -> Option<&str> {
        self.tracestate.as_deref()
    }

    /// The context under which a child of this span opens.
    #[must_use]
    pub fn ctx(&self) -> CallCtx {
        CallCtx {
            traceparent: Some(self.traceparent()),
            tracestate: self.tracestate.clone(),
        }
    }

    /// The `traceparent` the host's sink gave this span; `None` when the span
    /// is not recording.
    #[must_use]
    pub fn recorded_traceparent(&self) -> Option<String> {
        self.recorded.as_ref().map(|(_, host)| host.clone())
    }

    /// Ends the span with `status`.
    pub fn end(self, status: SpanStatus) {
        self.end_with(status, None, Attributes::new());
    }

    /// Ends the span as failed with `error`.
    pub fn fail(self, error: &Error) {
        self.end_with(
            SpanStatus::Error,
            Some(error.span_error()),
            Attributes::new(),
        );
    }

    /// Ends the span with `status`, an optional `<connectCode> <code>` text
    /// and attributes known only now.
    pub fn end_with(mut self, status: SpanStatus, error: Option<String>, attributes: Attributes) {
        self.close(status, error, attributes);
    }

    fn close(&mut self, status: SpanStatus, error: Option<String>, attributes: Attributes) {
        self.ended = true;
        if let Some((sink, host)) = self.recorded.take() {
            sink.end(&SpanEnd {
                traceparent: host,
                end_time_ms: now_ms(),
                status,
                error,
                attributes,
            });
        }
    }
}

impl Drop for ActiveSpan {
    fn drop(&mut self) {
        if !self.ended {
            self.close(SpanStatus::Unset, None, Attributes::new());
        }
    }
}

/// The sink of a browser host: an object with `start(json)`, returning a
/// `traceparent` string or nothing, and `end(json)`.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub struct JsSink {
    host: wasm_bindgen::JsValue,
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
impl JsSink {
    /// A sink that calls `host`.
    #[must_use]
    pub fn new(host: wasm_bindgen::JsValue) -> Self {
        Self { host }
    }

    fn call(&self, method: &str, json: &Value) -> Option<wasm_bindgen::JsValue> {
        use wasm_bindgen::JsCast as _;
        let function = js_sys::Reflect::get(&self.host, &wasm_bindgen::JsValue::from_str(method))
            .ok()?
            .dyn_into::<js_sys::Function>()
            .ok()?;
        function
            .call1(
                &self.host,
                &wasm_bindgen::JsValue::from_str(&json.to_string()),
            )
            .ok()
    }
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
impl SpanSink for JsSink {
    fn start(&self, span: &SpanStart) -> Option<String> {
        self.call("start", &span.to_value())?.as_string()
    }

    fn end(&self, end: &SpanEnd) {
        self.call("end", &end.to_value());
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const PARENT: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";

    #[derive(Default)]
    struct Recorder {
        starts: Mutex<Vec<SpanStart>>,
        ends: Mutex<Vec<SpanEnd>>,
        answer: Option<String>,
    }

    impl SpanSink for Recorder {
        fn start(&self, span: &SpanStart) -> Option<String> {
            self.starts.lock().unwrap().push(span.clone());
            self.answer.clone()
        }

        fn end(&self, end: &SpanEnd) {
            self.ends.lock().unwrap().push(end.clone());
        }
    }

    fn recorder(answer: Option<&str>) -> Arc<Recorder> {
        Arc::new(Recorder {
            answer: answer.map(str::to_owned),
            ..Recorder::default()
        })
    }

    fn call_ctx(traceparent: Option<&str>, tracestate: Option<&str>) -> CallCtx {
        CallCtx {
            traceparent: traceparent.map(str::to_owned),
            tracestate: tracestate.map(str::to_owned),
        }
    }

    #[test]
    fn a_traceparent_reads_and_writes_the_same_text() {
        let id = Traceparent::parse(PARENT).unwrap();
        assert_eq!(id.to_string(), PARENT);
        assert_eq!(id.trace_id(), "4bf92f3577b34da6a3ce929d0e0e4736");
        let unsampled = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-00";
        assert_eq!(
            Traceparent::parse(unsampled).unwrap().to_string(),
            unsampled
        );
    }

    #[test]
    fn a_malformed_traceparent_is_refused() {
        for text in [
            "",
            "00",
            "01-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-00",
            "00-4bf92f3577b34da6a3ce929d0e0e47-00f067aa0ba902b7-01",
            "00-4BF92F3577B34DA6A3CE929D0E0E4736-00f067aa0ba902b7-01",
            "00-4bf92f3577b34da6a3ce929d0e0e473g-00f067aa0ba902b7-01",
            "00-00000000000000000000000000000000-00f067aa0ba902b7-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-1",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-+1",
        ] {
            assert_eq!(Traceparent::parse(text), None, "{text}");
        }
    }

    #[test]
    fn a_minted_root_is_a_fresh_sampled_trace() {
        let first = Traceparent::mint(None);
        let second = Traceparent::mint(None);
        assert_eq!(Traceparent::parse(&first.to_string()), Some(first));
        assert!(first.to_string().ends_with("-01"));
        assert_ne!(first.trace_id(), second.trace_id());
        assert_ne!(first.span_id, second.span_id);
    }

    #[test]
    fn a_minted_child_keeps_the_trace_and_flags_of_its_parent() {
        let parent =
            Traceparent::parse("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-00").unwrap();
        let child = Traceparent::mint(Some(&parent));
        assert_eq!(child.trace_id(), parent.trace_id());
        assert_ne!(child.span_id, parent.span_id);
        assert_eq!(child.flags, 0);
    }

    #[test]
    fn an_all_zero_draw_is_redrawn() {
        let mut calls = 0;
        let id = Traceparent::mint_with(None, |bytes| {
            calls += 1;
            if calls > 1 {
                bytes.fill(7);
            }
        });
        assert_eq!(
            calls, 3,
            "the zero trace id was drawn again, then the span id once"
        );
        assert_eq!(id.trace_id(), "07".repeat(16));
        assert_eq!(id.span_id, [7; 8]);
    }

    #[test]
    fn a_span_serialises_to_the_host_json() {
        let mut attributes = Attributes::new();
        attributes.insert("rpc.system".to_owned(), json!("connect_rpc"));
        let begin = SpanStart {
            name: "pkg.Svc/Get".to_owned(),
            start_time_ms: 1500.5,
            kind: SpanKind::Client,
            parent: Some(PARENT.to_owned()),
            tracestate: Some("k=v".to_owned()),
            attributes,
        };
        assert_eq!(
            begin.to_value(),
            json!({
                "name": "pkg.Svc/Get",
                "startTime": 1500.5,
                "kind": "client",
                "parent": PARENT,
                "tracestate": "k=v",
                "attributes": {"rpc.system": "connect_rpc"},
            })
        );
        let root = SpanStart {
            name: "walk".to_owned(),
            start_time_ms: 1.0,
            kind: SpanKind::Internal,
            parent: None,
            tracestate: None,
            attributes: Attributes::new(),
        };
        assert_eq!(
            root.to_value(),
            json!({"name": "walk", "startTime": 1.0, "kind": "internal"})
        );
    }

    #[test]
    fn a_span_end_serialises_to_the_host_json() {
        let mut attributes = Attributes::new();
        attributes.insert("pages".to_owned(), json!(3));
        let failed = SpanEnd {
            traceparent: PARENT.to_owned(),
            end_time_ms: 2000.0,
            status: SpanStatus::Error,
            error: Some("unavailable SERVICE_UNAVAILABLE".to_owned()),
            attributes,
        };
        assert_eq!(
            failed.to_value(),
            json!({
                "traceparent": PARENT,
                "endTime": 2000.0,
                "status": "error",
                "error": "unavailable SERVICE_UNAVAILABLE",
                "attributes": {"pages": 3},
            })
        );
        let unset = SpanEnd {
            traceparent: PARENT.to_owned(),
            end_time_ms: 1.0,
            status: SpanStatus::Unset,
            error: None,
            attributes: Attributes::new(),
        };
        assert_eq!(
            unset.to_value(),
            json!({"traceparent": PARENT, "endTime": 1.0, "status": "unset"})
        );
        assert_eq!(SpanStatus::Ok.wire_name(), "ok");
    }

    #[test]
    fn without_a_sink_the_core_mints_ids_and_reports_nothing() {
        let span = Tracer::default().start(
            "pkg.Svc/Get",
            SpanKind::Client,
            &call_ctx(None, Some("k=v")),
            Attributes::new(),
        );
        assert!(Traceparent::parse(&span.traceparent()).is_some());
        assert_eq!(
            span.tracestate(),
            None,
            "a tracestate needs a readable parent"
        );
        assert_eq!(span.recorded_traceparent(), None);
        span.end(SpanStatus::Ok);
        assert_eq!(format!("{:?}", Tracer::default()), "Tracer { sink: false }");
    }

    #[test]
    fn without_a_sink_a_child_continues_the_callers_trace() {
        let span = Tracer::default().start(
            "x",
            SpanKind::Client,
            &call_ctx(Some(PARENT), Some("k=v")),
            Attributes::new(),
        );
        let sent = Traceparent::parse(&span.traceparent()).unwrap();
        assert_eq!(sent.trace_id(), "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_ne!(span.traceparent(), PARENT);
        assert_eq!(span.tracestate(), Some("k=v"));
        assert_eq!(span.ctx(), call_ctx(Some(&span.traceparent()), Some("k=v")));
    }

    #[test]
    fn a_recording_sink_sees_start_and_end_and_names_the_traceparent() {
        let sink = recorder(Some(
            "00-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-bbbbbbbbbbbbbbbb-01",
        ));
        let tracer = Tracer::new(sink.clone());
        assert_eq!(format!("{tracer:?}"), "Tracer { sink: true }");
        let mut attributes = Attributes::new();
        attributes.insert("rpc.service".to_owned(), json!("pkg.Svc"));
        let span = tracer.start(
            "pkg.Svc/Get",
            SpanKind::Client,
            &call_ctx(Some(PARENT), Some("k=v")),
            attributes,
        );
        assert_eq!(
            span.traceparent(),
            "00-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-bbbbbbbbbbbbbbbb-01"
        );
        assert_eq!(
            span.recorded_traceparent().as_deref(),
            Some("00-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-bbbbbbbbbbbbbbbb-01")
        );
        assert!(
            sink.ends.lock().unwrap().is_empty(),
            "end waits for the settle"
        );
        span.fail(&Error::handle_busy());
        let starts = sink.starts.lock().unwrap();
        assert_eq!(starts.len(), 1);
        assert_eq!(starts[0].name, "pkg.Svc/Get");
        assert_eq!(starts[0].kind, SpanKind::Client);
        assert_eq!(starts[0].parent.as_deref(), Some(PARENT));
        assert_eq!(starts[0].tracestate.as_deref(), Some("k=v"));
        assert_eq!(starts[0].attributes["rpc.service"], json!("pkg.Svc"));
        assert!(starts[0].start_time_ms > 1.7e12);
        let ends = sink.ends.lock().unwrap();
        assert_eq!(ends.len(), 1);
        assert_eq!(
            ends[0].traceparent,
            "00-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-bbbbbbbbbbbbbbbb-01"
        );
        assert_eq!(ends[0].status, SpanStatus::Error);
        assert_eq!(
            ends[0].error.as_deref(),
            Some("failed_precondition HANDLE_BUSY")
        );
        assert!(ends[0].end_time_ms >= starts[0].start_time_ms);
    }

    #[test]
    fn an_unreadable_parent_makes_a_root_span() {
        let sink = recorder(None);
        let tracer = Tracer::new(sink.clone());
        let span = tracer.start(
            "x",
            SpanKind::Client,
            &call_ctx(Some("garbage"), Some("k=v")),
            Attributes::new(),
        );
        let starts = sink.starts.lock().unwrap();
        assert_eq!(starts[0].parent, None);
        assert_eq!(starts[0].tracestate, None);
        drop(starts);
        drop(span);
    }

    #[test]
    fn a_sink_that_is_not_recording_never_gets_an_end() {
        let sink = recorder(None);
        let span = Tracer::new(sink.clone()).start(
            "x",
            SpanKind::Internal,
            &CallCtx::default(),
            Attributes::new(),
        );
        assert_eq!(span.recorded_traceparent(), None);
        assert!(Traceparent::parse(&span.traceparent()).is_some());
        span.end(SpanStatus::Ok);
        assert!(sink.ends.lock().unwrap().is_empty());
    }

    #[test]
    fn a_host_traceparent_the_core_cannot_read_is_ended_as_given_and_replaced_for_sending() {
        let sink = recorder(Some("opaque-host-id"));
        let span = Tracer::new(sink.clone()).start(
            "x",
            SpanKind::Client,
            &call_ctx(Some(PARENT), None),
            Attributes::new(),
        );
        let sent = Traceparent::parse(&span.traceparent()).unwrap();
        assert_eq!(sent.trace_id(), "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(
            span.recorded_traceparent().as_deref(),
            Some("opaque-host-id")
        );
        span.end(SpanStatus::Ok);
        assert_eq!(sink.ends.lock().unwrap()[0].traceparent, "opaque-host-id");
    }

    #[test]
    fn a_span_dropped_before_it_ends_ends_with_status_unset() {
        let sink = recorder(Some(
            "00-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-bbbbbbbbbbbbbbbb-01",
        ));
        let span = Tracer::new(sink.clone()).start(
            "walk",
            SpanKind::Internal,
            &CallCtx::default(),
            Attributes::new(),
        );
        assert!(format!("{span:?}").contains("ended: false"));
        drop(span);
        let ends = sink.ends.lock().unwrap();
        assert_eq!(ends.len(), 1);
        assert_eq!(ends[0].status, SpanStatus::Unset);
        assert_eq!(ends[0].error, None);
    }

    #[test]
    fn a_span_ends_once() {
        let sink = recorder(Some(
            "00-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-bbbbbbbbbbbbbbbb-01",
        ));
        let span = Tracer::new(sink.clone()).start(
            "x",
            SpanKind::Client,
            &CallCtx::default(),
            Attributes::new(),
        );
        let mut attributes = Attributes::new();
        attributes.insert("pages".to_owned(), json!(2));
        span.end_with(SpanStatus::Ok, None, attributes);
        let ends = sink.ends.lock().unwrap();
        assert_eq!(ends.len(), 1, "the drop that follows adds nothing");
        assert_eq!(ends[0].status, SpanStatus::Ok);
        assert_eq!(ends[0].attributes["pages"], json!(2));
    }
}
