// SPDX-License-Identifier: MIT
//! The spans a Layer 2 call opens, reported live to a host sink.
//!
//! The core reports a span when it starts and again when it ends. Nothing is
//! buffered or replayed: a sink sees `start` as the call begins and `end` as
//! it settles. A span whose parent is missing or unreadable is a root.
//!
//! The trace context a call sends is always the core's own: the `traceparent`
//! the sink returned from `start`, or one the core mints when it has no sink
//! or the sink answered with one the core cannot read.
//!
//! A sink answers `start` with a [`SpanStarted`]. When it says the span is not
//! recording, the core still sends the span's `traceparent` and still ends the
//! span the sink started, but reports nothing for the spans nested under it.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::UNIX_EPOCH;

use api_bones_rt::{MaybeSend, MaybeSync, NoRandomSource, fill_random, system_now};
use serde_json::Value;

use crate::ctx::CallCtx;
use crate::error::Error;

/// The attributes of a span: names to JSON scalars.
pub type Attributes = BTreeMap<String, Value>;

/// The W3C `traceparent` of a span: version `00`, a trace id, a span id and
/// the trace flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
fn random_id<const N: usize>(
    fill: &mut impl FnMut(&mut [u8]) -> Result<(), NoRandomSource>,
) -> Result<[u8; N], NoRandomSource> {
    let mut id = [0_u8; N];
    while id == [0_u8; N] {
        fill(&mut id)?;
    }
    Ok(id)
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
    ///
    /// # Errors
    /// [`NoRandomSource`] when the platform has no secure random source; no id
    /// is made from anything else.
    pub fn mint(parent: Option<&Self>) -> Result<Self, NoRandomSource> {
        Self::mint_with(parent, fill_random)
    }

    fn mint_with(
        parent: Option<&Self>,
        mut fill: impl FnMut(&mut [u8]) -> Result<(), NoRandomSource>,
    ) -> Result<Self, NoRandomSource> {
        Ok(Self {
            trace_id: match parent {
                Some(parent) => parent.trace_id,
                None => random_id(&mut fill)?,
            },
            span_id: random_id(&mut fill)?,
            flags: parent.map_or(SAMPLED, |p| p.flags),
        })
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

/// What a sink answers when a span starts: the span's `traceparent`, and
/// whether the sink records it.
///
/// The default is the answer of a sink that gave nothing readable: no
/// `traceparent` and not recording.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpanStarted {
    /// The new span's `traceparent`; empty when the sink named none.
    pub traceparent: String,
    /// False when the span is not recording: the core makes no `start` or
    /// `end` crossing for the spans nested under it.
    pub recording: bool,
}

impl SpanStarted {
    /// A recorded span with `traceparent`.
    #[must_use]
    pub fn recording(traceparent: impl Into<String>) -> Self {
        Self {
            traceparent: traceparent.into(),
            recording: true,
        }
    }

    /// A span the sink does not record, with `traceparent`.
    #[must_use]
    pub fn not_recording(traceparent: impl Into<String>) -> Self {
        Self {
            traceparent: traceparent.into(),
            recording: false,
        }
    }
}

/// The host that records the core's spans, live.
pub trait SpanSink: MaybeSend + MaybeSync {
    /// A span began. Returns the new span's `traceparent` and whether it is
    /// recording. `end` is called for every span the sink started, recording
    /// or not.
    fn start(&self, span: &SpanStart) -> SpanStarted;

    /// A span the sink started ended.
    fn end(&self, end: &SpanEnd);
}

/// The wall clock in milliseconds since the Unix epoch.
fn now_ms() -> f64 {
    system_now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |elapsed| elapsed.as_secs_f64() * 1000.0)
}

/// The traceparents whose spans are quiet, each with the number of open spans
/// that hold it quiet.
type Quiet = Arc<Mutex<HashMap<Traceparent, usize>>>;

/// Keeps a traceparent quiet while it lives.
struct Hush {
    quiet: Quiet,
    traceparent: Traceparent,
}

impl Hush {
    fn new(quiet: &Quiet, traceparent: Traceparent) -> Self {
        *quiet
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(traceparent)
            .or_default() += 1;
        Self {
            quiet: Arc::clone(quiet),
            traceparent,
        }
    }
}

impl Drop for Hush {
    fn drop(&mut self) {
        let mut quiet = self.quiet.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(holders) = quiet.get_mut(&self.traceparent) {
            *holders -= 1;
            if *holders == 0 {
                quiet.remove(&self.traceparent);
            }
        }
    }
}

/// Opens spans against an optional sink.
#[derive(Clone, Default)]
pub struct Tracer {
    sink: Option<Arc<dyn SpanSink>>,
    quiet: Quiet,
}

impl fmt::Debug for Tracer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tracer")
            .field("sink", &self.sink.is_some())
            .finish_non_exhaustive()
    }
}

impl Tracer {
    /// A tracer that reports to `sink`.
    #[must_use]
    pub fn new(sink: Arc<dyn SpanSink>) -> Self {
        Self {
            sink: Some(sink),
            quiet: Quiet::default(),
        }
    }

    fn is_quiet(&self, traceparent: &Traceparent) -> bool {
        self.quiet
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(traceparent)
    }

    /// Opens a span under `parent`: the call's caller or an enclosing span.
    /// The `tracestate` follows only a readable parent.
    ///
    /// With a sink, the span is reported unless its parent is quiet. A span the
    /// sink answers as not recording, and every span opened under a quiet
    /// span, is quiet: it opens no span at the sink. A span the sink started is
    /// ended at the sink even when it is not recording.
    ///
    /// # Errors
    /// [`NoRandomSource`] when the core must mint the span's ids and the
    /// platform has no secure random source. A span the sink started is ended
    /// at the sink as failed.
    pub fn start(
        &self,
        name: &str,
        kind: SpanKind,
        parent: &CallCtx,
        attributes: Attributes,
    ) -> Result<ActiveSpan, NoRandomSource> {
        self.start_minting(name, kind, parent, attributes, fill_random)
    }

    fn start_minting(
        &self,
        name: &str,
        kind: SpanKind,
        parent: &CallCtx,
        attributes: Attributes,
        fill: impl FnMut(&mut [u8]) -> Result<(), NoRandomSource>,
    ) -> Result<ActiveSpan, NoRandomSource> {
        let parent_id = parent.traceparent.as_deref().and_then(Traceparent::parse);
        let tracestate = parent_id.and(parent.tracestate.clone());
        let started = self.sink.as_ref().and_then(|sink| {
            if parent_id.as_ref().is_some_and(|id| self.is_quiet(id)) {
                return None;
            }
            let begin = SpanStart {
                name: name.to_owned(),
                start_time_ms: now_ms(),
                kind,
                parent: parent_id.map(|id| id.to_string()),
                tracestate: tracestate.clone(),
                attributes,
            };
            Some((Arc::clone(sink), sink.start(&begin)))
        });
        let named = started
            .as_ref()
            .and_then(|(_, answer)| Traceparent::parse(&answer.traceparent));
        let traceparent = match named {
            Some(traceparent) => traceparent,
            None => match Traceparent::mint_with(parent_id.as_ref(), fill) {
                Ok(traceparent) => traceparent,
                Err(refused) => {
                    if let Some((sink, answer)) = started {
                        sink.end(&SpanEnd {
                            traceparent: answer.traceparent,
                            end_time_ms: now_ms(),
                            status: SpanStatus::Error,
                            error: Some(Error::from(crate::error::no_random_source()).span_error()),
                            attributes: Attributes::new(),
                        });
                    }
                    return Err(refused);
                }
            },
        };
        let recording = started.as_ref().is_some_and(|(_, answer)| answer.recording);
        let hush = (self.sink.is_some() && !recording).then(|| Hush::new(&self.quiet, traceparent));
        Ok(ActiveSpan {
            traceparent,
            tracestate,
            recorded: started.map(|(sink, answer)| (sink, answer.traceparent)),
            recording,
            _hush: hush,
            ended: false,
        })
    }
}

/// An open span. It ends once: by [`ActiveSpan::end`], [`ActiveSpan::end_with`],
/// [`ActiveSpan::fail`], or, when dropped first, with status `unset`.
pub struct ActiveSpan {
    traceparent: Traceparent,
    tracestate: Option<String>,
    recorded: Option<(Arc<dyn SpanSink>, String)>,
    recording: bool,
    _hush: Option<Hush>,
    ended: bool,
}

impl fmt::Debug for ActiveSpan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActiveSpan")
            .field("traceparent", &self.traceparent.to_string())
            .field("tracestate", &self.tracestate)
            .field("recorded", &self.recorded.is_some())
            .field("recording", &self.recording)
            .field("ended", &self.ended)
            .finish_non_exhaustive()
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
            prefetch: None,
            first_only: false,
        }
    }

    /// The `traceparent` the host's sink gave this span; `None` when the sink
    /// did not start the span or named none.
    #[must_use]
    pub fn recorded_traceparent(&self) -> Option<String> {
        self.recorded
            .as_ref()
            .map(|(_, host)| host)
            .filter(|host| !host.is_empty())
            .cloned()
    }

    /// Whether the host's sink records this span. False without a sink, when
    /// the sink answered not recording, and under a quiet span.
    #[must_use]
    pub fn is_recording(&self) -> bool {
        self.recording
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

/// The sink of a browser host: an object with `start(bytes)`, returning the
/// `bones.sdk.v1.SpanStarted` bytes, and `end(bytes)`. Both take the
/// `bones.sdk.v1.SpanStart` and `bones.sdk.v1.SpanEnd` bytes as a
/// `Uint8Array`.
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

    fn call(&self, method: &str, message: &[u8]) -> Option<wasm_bindgen::JsValue> {
        use wasm_bindgen::JsCast as _;
        let function = js_sys::Reflect::get(&self.host, &wasm_bindgen::JsValue::from_str(method))
            .ok()?
            .dyn_into::<js_sys::Function>()
            .ok()?;
        function
            .call1(&self.host, &js_sys::Uint8Array::from(message))
            .ok()
    }
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
impl SpanSink for JsSink {
    fn start(&self, span: &SpanStart) -> SpanStarted {
        use wasm_bindgen::JsCast as _;
        self.call("start", &crate::boundary::span_start(span))
            .and_then(|answer| answer.dyn_into::<js_sys::Uint8Array>().ok())
            .and_then(|answer| crate::boundary::span_started(&answer.to_vec()))
            .unwrap_or_default()
    }

    fn end(&self, end: &SpanEnd) {
        self.call("end", &crate::boundary::span_end(end));
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use serde_json::json;

    const PARENT: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";

    #[derive(Default)]
    struct Recorder {
        starts: Mutex<Vec<SpanStart>>,
        ends: Mutex<Vec<SpanEnd>>,
        answer: SpanStarted,
    }

    impl SpanSink for Recorder {
        fn start(&self, span: &SpanStart) -> SpanStarted {
            self.starts.lock().unwrap().push(span.clone());
            self.answer.clone()
        }

        fn end(&self, end: &SpanEnd) {
            self.ends.lock().unwrap().push(end.clone());
        }
    }

    const HOST: &str = "00-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-bbbbbbbbbbbbbbbb-01";

    fn recorder(answer: SpanStarted) -> Arc<Recorder> {
        Arc::new(Recorder {
            answer,
            ..Recorder::default()
        })
    }

    fn start_in(tracer: &Tracer, name: &str, parent: &CallCtx) -> ActiveSpan {
        tracer
            .start(name, SpanKind::Client, parent, Attributes::new())
            .unwrap()
    }

    fn start_names(sink: &Recorder) -> Vec<String> {
        sink.starts
            .lock()
            .unwrap()
            .iter()
            .map(|start| start.name.clone())
            .collect()
    }

    fn call_ctx(traceparent: Option<&str>, tracestate: Option<&str>) -> CallCtx {
        CallCtx {
            traceparent: traceparent.map(str::to_owned),
            tracestate: tracestate.map(str::to_owned),
            prefetch: None,
            first_only: false,
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
        let first = Traceparent::mint(None).unwrap();
        let second = Traceparent::mint(None).unwrap();
        assert_eq!(Traceparent::parse(&first.to_string()), Some(first));
        assert!(first.to_string().ends_with("-01"));
        assert_ne!(first.trace_id(), second.trace_id());
        assert_ne!(first.span_id, second.span_id);
    }

    #[test]
    fn a_minted_child_keeps_the_trace_and_flags_of_its_parent() {
        let parent =
            Traceparent::parse("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-00").unwrap();
        let child = Traceparent::mint(Some(&parent)).unwrap();
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
            Ok(())
        })
        .unwrap();
        assert_eq!(
            calls, 3,
            "the zero trace id was drawn again, then the span id once"
        );
        assert_eq!(id.trace_id(), "07".repeat(16));
        assert_eq!(id.span_id, [7; 8]);
    }

    #[test]
    fn a_host_without_randomness_mints_no_id() {
        let none = |_: &mut [u8]| Err(NoRandomSource);
        assert_eq!(Traceparent::mint_with(None, none), Err(NoRandomSource));
        let parent = Traceparent::parse(PARENT).unwrap();
        assert_eq!(
            Traceparent::mint_with(Some(&parent), none),
            Err(NoRandomSource),
            "a child needs a fresh span id"
        );
        let mut draws = 0;
        let after_trace_id = Traceparent::mint_with(None, |bytes| {
            draws += 1;
            if draws > 1 {
                return Err(NoRandomSource);
            }
            bytes.fill(7);
            Ok(())
        });
        assert_eq!(after_trace_id, Err(NoRandomSource));
    }

    #[test]
    fn a_span_that_needs_ids_the_host_cannot_draw_is_refused() {
        let refused = Tracer::default().start_minting(
            "x",
            SpanKind::Client,
            &CallCtx::default(),
            Attributes::new(),
            |_: &mut [u8]| Err(NoRandomSource),
        );
        assert_eq!(refused.unwrap_err(), NoRandomSource);
    }

    #[test]
    fn a_span_the_sink_started_is_ended_failed_when_its_ids_cannot_be_drawn() {
        let sink = recorder(SpanStarted::recording("not a traceparent"));
        let refused = Tracer::new(sink.clone()).start_minting(
            "x",
            SpanKind::Client,
            &CallCtx::default(),
            Attributes::new(),
            |_: &mut [u8]| Err(NoRandomSource),
        );
        assert_eq!(refused.unwrap_err(), NoRandomSource);
        let ends = sink.ends.lock().unwrap();
        assert_eq!(ends.len(), 1, "the sink's span is not left open");
        assert_eq!(ends[0].traceparent, "not a traceparent");
        assert_eq!(ends[0].status, SpanStatus::Error);
        assert_eq!(
            ends[0].error.as_deref(),
            Some("failed_precondition PRECONDITION_FAILED")
        );
    }

    #[test]
    fn without_a_sink_the_core_mints_ids_and_reports_nothing() {
        let span = Tracer::default()
            .start(
                "pkg.Svc/Get",
                SpanKind::Client,
                &call_ctx(None, Some("k=v")),
                Attributes::new(),
            )
            .unwrap();
        assert!(Traceparent::parse(&span.traceparent()).is_some());
        assert_eq!(
            span.tracestate(),
            None,
            "a tracestate needs a readable parent"
        );
        assert_eq!(span.recorded_traceparent(), None);
        span.end(SpanStatus::Ok);
        assert_eq!(
            format!("{:?}", Tracer::default()),
            "Tracer { sink: false, .. }"
        );
    }

    #[test]
    fn without_a_sink_a_child_continues_the_callers_trace() {
        let span = Tracer::default()
            .start(
                "x",
                SpanKind::Client,
                &call_ctx(Some(PARENT), Some("k=v")),
                Attributes::new(),
            )
            .unwrap();
        let sent = Traceparent::parse(&span.traceparent()).unwrap();
        assert_eq!(sent.trace_id(), "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_ne!(span.traceparent(), PARENT);
        assert_eq!(span.tracestate(), Some("k=v"));
        assert_eq!(span.ctx(), call_ctx(Some(&span.traceparent()), Some("k=v")));
    }

    #[test]
    fn a_recording_sink_sees_start_and_end_and_names_the_traceparent() {
        let sink = recorder(SpanStarted::recording(HOST));
        let tracer = Tracer::new(sink.clone());
        assert_eq!(format!("{tracer:?}"), "Tracer { sink: true, .. }");
        let mut attributes = Attributes::new();
        attributes.insert("rpc.service".to_owned(), json!("pkg.Svc"));
        let span = tracer
            .start(
                "pkg.Svc/Get",
                SpanKind::Client,
                &call_ctx(Some(PARENT), Some("k=v")),
                attributes,
            )
            .unwrap();
        assert_eq!(span.traceparent(), HOST);
        assert_eq!(span.recorded_traceparent().as_deref(), Some(HOST));
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
        assert_eq!(ends[0].traceparent, HOST);
        assert_eq!(ends[0].status, SpanStatus::Error);
        assert_eq!(
            ends[0].error.as_deref(),
            Some("failed_precondition HANDLE_BUSY")
        );
        assert!(ends[0].end_time_ms >= starts[0].start_time_ms);
    }

    #[test]
    fn an_unreadable_parent_makes_a_root_span() {
        let sink = recorder(SpanStarted::default());
        let tracer = Tracer::new(sink.clone());
        let span = tracer
            .start(
                "x",
                SpanKind::Client,
                &call_ctx(Some("garbage"), Some("k=v")),
                Attributes::new(),
            )
            .unwrap();
        let starts = sink.starts.lock().unwrap();
        assert_eq!(starts[0].parent, None);
        assert_eq!(starts[0].tracestate, None);
        drop(starts);
        drop(span);
    }

    #[test]
    fn a_span_the_sink_does_not_record_is_still_ended_and_sends_a_root_traceparent() {
        let sink = recorder(SpanStarted::not_recording(HOST));
        let span = start_in(&Tracer::new(sink.clone()), "x", &CallCtx::default());
        assert!(!span.is_recording());
        assert_eq!(span.traceparent(), HOST);
        assert_eq!(span.recorded_traceparent().as_deref(), Some(HOST));
        span.end(SpanStatus::Ok);
        let ends = sink.ends.lock().unwrap();
        assert_eq!(ends.len(), 1);
        assert_eq!(ends[0].traceparent, HOST);
    }

    #[test]
    fn a_sink_that_answers_nothing_readable_starts_a_quiet_span_with_a_minted_traceparent() {
        let sink = recorder(SpanStarted::default());
        let span = start_in(&Tracer::new(sink.clone()), "x", &CallCtx::default());
        assert!(!span.is_recording());
        assert_eq!(span.recorded_traceparent(), None);
        assert!(Traceparent::parse(&span.traceparent()).is_some());
        span.end(SpanStatus::Ok);
        assert_eq!(sink.ends.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_recording_span_is_recording() {
        let sink = recorder(SpanStarted::recording(HOST));
        let span = start_in(&Tracer::new(sink), "x", &CallCtx::default());
        assert!(span.is_recording());
        assert!(!start_in(&Tracer::default(), "x", &CallCtx::default()).is_recording());
    }

    #[test]
    fn a_recording_span_with_no_traceparent_names_none() {
        let sink = recorder(SpanStarted::recording(""));
        let span = start_in(&Tracer::new(sink.clone()), "x", &CallCtx::default());
        assert!(span.is_recording());
        assert_eq!(span.recorded_traceparent(), None);
        assert!(Traceparent::parse(&span.traceparent()).is_some());
        span.end(SpanStatus::Ok);
        assert_eq!(sink.ends.lock().unwrap()[0].traceparent, "");
    }

    #[test]
    fn nothing_nests_under_a_span_the_sink_does_not_record() {
        let sink = recorder(SpanStarted::not_recording(HOST));
        let tracer = Tracer::new(sink.clone());
        let root = start_in(&tracer, "root", &CallCtx::default());
        let child = start_in(&tracer, "child", &root.ctx());
        let grandchild = start_in(&tracer, "grandchild", &child.ctx());
        assert!(!child.is_recording());
        assert!(!grandchild.is_recording());
        assert_eq!(child.recorded_traceparent(), None);
        assert_eq!(
            Traceparent::parse(&child.traceparent()).unwrap().trace_id(),
            Traceparent::parse(&root.traceparent()).unwrap().trace_id(),
            "a quiet child still continues the trace it was sent in"
        );
        grandchild.end(SpanStatus::Ok);
        child.end(SpanStatus::Ok);
        root.end(SpanStatus::Ok);
        assert_eq!(start_names(&sink), ["root"]);
        assert_eq!(sink.ends.lock().unwrap().len(), 1, "only the root ends");
    }

    #[test]
    fn a_span_stays_quiet_until_the_last_span_holding_it_quiet_closes() {
        let sink = recorder(SpanStarted::not_recording(HOST));
        let tracer = Tracer::new(sink);
        let host = Traceparent::parse(HOST).unwrap();
        let first = start_in(&tracer, "first", &CallCtx::default());
        let second = start_in(&tracer, "second", &CallCtx::default());
        assert_eq!(first.traceparent(), second.traceparent());
        assert!(tracer.is_quiet(&host));
        drop(first);
        assert!(tracer.is_quiet(&host));
        drop(second);
        assert!(!tracer.is_quiet(&host));
    }

    #[test]
    fn a_span_in_another_trace_is_not_quiet() {
        let sink = recorder(SpanStarted::not_recording(HOST));
        let tracer = Tracer::new(sink.clone());
        let root = start_in(&tracer, "root", &CallCtx::default());
        let elsewhere = start_in(&tracer, "elsewhere", &call_ctx(Some(PARENT), None));
        assert_eq!(start_names(&sink), ["root", "elsewhere"]);
        drop((root, elsewhere));
    }

    #[test]
    fn the_children_of_a_recording_span_are_reported() {
        let sink = recorder(SpanStarted::recording(HOST));
        let tracer = Tracer::new(sink.clone());
        let root = start_in(&tracer, "root", &CallCtx::default());
        let child = start_in(&tracer, "child", &root.ctx());
        assert!(child.is_recording());
        drop((child, root));
        assert_eq!(start_names(&sink), ["root", "child"]);
        assert_eq!(sink.ends.lock().unwrap().len(), 2);
    }

    #[test]
    fn a_cloned_tracer_shares_the_quiet_set() {
        let sink = recorder(SpanStarted::not_recording(HOST));
        let tracer = Tracer::new(sink.clone());
        let root = start_in(&tracer, "root", &CallCtx::default());
        let child = start_in(&tracer.clone(), "child", &root.ctx());
        assert!(!child.is_recording());
        assert_eq!(start_names(&sink), ["root"]);
    }

    #[test]
    fn a_span_started_is_made_from_its_traceparent() {
        assert_eq!(
            SpanStarted::recording("tp"),
            SpanStarted {
                traceparent: "tp".to_owned(),
                recording: true
            }
        );
        assert_eq!(
            SpanStarted::not_recording("tp"),
            SpanStarted {
                traceparent: "tp".to_owned(),
                recording: false
            }
        );
        assert_eq!(SpanStarted::default(), SpanStarted::not_recording(""));
    }

    #[test]
    fn a_host_traceparent_the_core_cannot_read_is_ended_as_given_and_replaced_for_sending() {
        let sink = recorder(SpanStarted::recording("opaque-host-id"));
        let span = Tracer::new(sink.clone())
            .start(
                "x",
                SpanKind::Client,
                &call_ctx(Some(PARENT), None),
                Attributes::new(),
            )
            .unwrap();
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
        let sink = recorder(SpanStarted::recording(HOST));
        let span = Tracer::new(sink.clone())
            .start(
                "walk",
                SpanKind::Internal,
                &CallCtx::default(),
                Attributes::new(),
            )
            .unwrap();
        assert!(format!("{span:?}").contains("ended: false"));
        drop(span);
        let ends = sink.ends.lock().unwrap();
        assert_eq!(ends.len(), 1);
        assert_eq!(ends[0].status, SpanStatus::Unset);
        assert_eq!(ends[0].error, None);
    }

    #[test]
    fn a_span_ends_once() {
        let sink = recorder(SpanStarted::recording(HOST));
        let span = Tracer::new(sink.clone())
            .start(
                "x",
                SpanKind::Client,
                &CallCtx::default(),
                Attributes::new(),
            )
            .unwrap();
        let mut attributes = Attributes::new();
        attributes.insert("pages".to_owned(), json!(2));
        span.end_with(SpanStatus::Ok, None, attributes);
        let ends = sink.ends.lock().unwrap();
        assert_eq!(ends.len(), 1, "the drop that follows adds nothing");
        assert_eq!(ends[0].status, SpanStatus::Ok);
        assert_eq!(ends[0].attributes["pages"], json!(2));
    }
}
