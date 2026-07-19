//! OpenTelemetry trace context propagation helpers for SDK clients and services.
//!
//! This module provides the outbound half — injecting the active OpenTelemetry
//! span context into HTTP headers — and the inbound half — extracting a parent
//! context from inbound headers — so trace continuity survives a service
//! boundary in either direction.
//!
//! # Example
//!
//! ```rust,no_run
//! use api_bones::propagation::inject_current;
//! use http::HeaderMap;
//!
//! let mut headers = HeaderMap::new();
//! inject_current(&mut headers);
//! // headers now contains traceparent / tracestate if an active span exists
//! ```

use http::HeaderMap;
use opentelemetry::{
    Context, global,
    propagation::{Extractor, Injector},
};

/// Injects the current OpenTelemetry span context into an [`http::HeaderMap`]
/// (`traceparent` / `tracestate`) via the globally-installed propagator.
///
/// This struct implements the [`opentelemetry::propagation::Injector`] trait to
/// enable injecting trace context headers into HTTP requests. It is the internal
/// adapter between the OpenTelemetry propagator and the HTTP header map.
struct HeaderMapInjector<'a>(&'a mut HeaderMap);

impl Injector for HeaderMapInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        if let (Ok(name), Ok(val)) = (
            http::header::HeaderName::try_from(key),
            http::header::HeaderValue::try_from(value),
        ) {
            self.0.insert(name, val);
        }
    }
}

/// Injects the current OpenTelemetry span context into HTTP headers.
///
/// This function retrieves the active span context from OpenTelemetry's global
/// context and injects it into the provided [`http::HeaderMap`] using the
/// globally-installed text map propagator. This enables proper trace continuity
/// by ensuring the callee's span links to the caller's span instead of starting
/// a new (orphan-root) trace.
///
/// If there is no active span context, no headers are added.
///
/// # Example
///
/// ```rust,no_run
/// use api_bones::propagation::inject_current;
/// use http::HeaderMap;
///
/// let mut headers = HeaderMap::new();
/// inject_current(&mut headers);
/// // headers may now contain "traceparent" and "tracestate" headers
/// ```
pub fn inject_current(headers: &mut HeaderMap) {
    global::get_text_map_propagator(|propagator| {
        propagator.inject_context(&Context::current(), &mut HeaderMapInjector(headers));
    });
}

/// Reads an [`http::HeaderMap`] (`traceparent` / `tracestate`) for the
/// [`opentelemetry::propagation::Extractor`] trait — the internal adapter
/// between the HTTP header map and the OpenTelemetry propagator, mirroring
/// [`HeaderMapInjector`] for the inbound direction.
struct HeaderMapExtractor<'a>(&'a HeaderMap);

impl Extractor for HeaderMapExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|v| v.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(http::HeaderName::as_str).collect()
    }
}

/// Extracts an inbound OpenTelemetry context from HTTP headers.
///
/// Reads `traceparent`/`tracestate` (or whatever the globally-installed text
/// map propagator understands) via the globally-installed propagator and
/// returns the resulting [`Context`], the inbound counterpart to
/// [`inject_current`]. Every service boundary needs both halves: an outbound
/// client injects the active context so the callee can extract it here and
/// parent its own root span on it — without this half, an inbound request
/// always starts a fresh, disconnected trace, regardless of what the caller
/// sent (platform/0229).
///
/// Returns the current (background) context, unmodified, if the headers carry
/// no recognizable trace context — callers can attach it exactly as they
/// would any other `Context` (e.g. via `tracing_opentelemetry`'s
/// `Span::set_parent`, or `Context::attach` directly), with the same
/// resulting no-op behavior as if no context had been extracted at all.
///
/// # Example
///
/// ```rust,no_run
/// use api_bones::propagation::extract_context;
/// use http::HeaderMap;
///
/// let headers = HeaderMap::new();
/// let parent_cx = extract_context(&headers);
/// // parent_cx.attach(), or hand it to your tracing integration's
/// // set_parent(parent_cx).
/// ```
#[must_use]
pub fn extract_context(headers: &HeaderMap) -> Context {
    global::get_text_map_propagator(|propagator| propagator.extract(&HeaderMapExtractor(headers)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_traceparent_without_active_span() {
        let mut headers = HeaderMap::new();
        inject_current(&mut headers);
        assert!(
            !headers.contains_key("traceparent"),
            "expected no traceparent header when no span is active"
        );
    }

    #[test]
    fn traceparent_injected_with_propagator_and_active_span() {
        use opentelemetry::trace::{TraceContextExt, Tracer, TracerProvider};
        use opentelemetry_sdk::propagation::TraceContextPropagator;
        use opentelemetry_sdk::trace::TracerProvider as SdkTracerProvider;

        global::set_text_map_propagator(TraceContextPropagator::new());
        let provider = SdkTracerProvider::builder().build();
        let tracer = provider.tracer("test");

        let span = tracer.start("test-span");
        let cx = Context::current_with_span(span);
        let _guard = cx.attach();

        let mut headers = HeaderMap::new();
        inject_current(&mut headers);

        assert!(
            headers.contains_key("traceparent"),
            "expected traceparent header when span is active, got: {headers:?}"
        );
    }

    #[test]
    fn extract_round_trips_an_injected_context() {
        use opentelemetry::trace::{TraceContextExt, Tracer, TracerProvider};
        use opentelemetry_sdk::propagation::TraceContextPropagator;
        use opentelemetry_sdk::trace::TracerProvider as SdkTracerProvider;

        global::set_text_map_propagator(TraceContextPropagator::new());
        let provider = SdkTracerProvider::builder().build();
        let tracer = provider.tracer("test");

        let span = tracer.start("test-span");
        let cx = Context::current_with_span(span);
        let sent_span_context = cx.span().span_context().clone();
        let _guard = cx.attach();

        let mut headers = HeaderMap::new();
        inject_current(&mut headers);

        let extracted = extract_context(&headers);
        let extracted_span = extracted.span();
        let extracted_span_context = extracted_span.span_context();
        // is_remote correctly differs (true once a context has crossed the
        // wire); trace_id/span_id/trace_flags are the part that must round-trip.
        assert_eq!(
            extracted_span_context.trace_id(),
            sent_span_context.trace_id()
        );
        assert_eq!(
            extracted_span_context.span_id(),
            sent_span_context.span_id()
        );
        assert_eq!(
            extracted_span_context.trace_flags(),
            sent_span_context.trace_flags()
        );
        assert!(extracted_span_context.is_remote());
    }

    #[test]
    fn extract_is_a_no_op_context_without_traceparent() {
        use opentelemetry::trace::TraceContextExt;

        let headers = HeaderMap::new();
        let cx = extract_context(&headers);
        assert!(
            !cx.span().span_context().is_valid(),
            "expected an invalid/empty span context when no traceparent header is present"
        );
    }
}
