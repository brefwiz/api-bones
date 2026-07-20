//! OpenTelemetry trace context propagation helpers for SDK clients.
//!
//! This module provides helpers for injecting the active OpenTelemetry span context
//! into HTTP headers, enabling proper trace continuity across service boundaries.
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
use opentelemetry::{Context, global, propagation::Injector};

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
        use opentelemetry_sdk::trace::SdkTracerProvider;

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
}
