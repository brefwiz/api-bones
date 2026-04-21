//! W3C Trace Context, `CorrelationId`, and `RequestId` propagation.
//!
//! Demonstrates parsing a `traceparent` header, creating child spans,
//! propagating `CorrelationId` across services, and generating `RequestId`s
//! at the edge.
//!
//! Run: `cargo run --example trace_context`

use api_bones::{CorrelationId, RequestId, SamplingFlags, TraceContext};

fn main() {
    // -- Parse a traceparent header from an incoming request --
    let incoming = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
    let tc: TraceContext = incoming.parse().expect("valid traceparent");
    println!("Parsed traceparent:");
    println!("  trace-id: {}", tc.trace_id);
    println!("  span-id:  {}", tc.span_id);
    println!("  sampled:  {}", tc.flags.is_sampled());
    assert_eq!(tc.to_string(), incoming);

    // -- Generate a fresh trace context for a new request --
    let fresh = TraceContext::new();
    println!("\nFresh TraceContext: {fresh}");
    println!("  header value: {}", fresh.header_value());

    // -- Create a child span (same trace-id, new span-id) --
    let child = tc.child_span();
    println!("\nChild span:");
    println!(
        "  trace-id same as parent: {}",
        child.trace_id == tc.trace_id
    );
    println!("  span-id differs:         {}", child.span_id != tc.span_id);
    println!("  child traceparent: {child}");

    // -- Sampling flags --
    println!("\n--- Sampling flags ---");
    let sampled = SamplingFlags::sampled();
    let not_sampled = SamplingFlags::not_sampled();
    println!("sampled():     is_sampled={}", sampled.is_sampled());
    println!("not_sampled(): is_sampled={}", not_sampled.is_sampled());

    // -- CorrelationId: groups related requests across services --
    println!("\n--- CorrelationId propagation ---");
    let correlation_id = CorrelationId::new_uuid();
    println!("Header name:  {}", correlation_id.header_name());
    println!("Header value: {}", correlation_id.as_str());
    // Parse a correlation ID from an incoming header
    let parsed_corr: CorrelationId = "user-action-checkout-42"
        .parse()
        .expect("valid correlation ID");
    println!("Parsed corr:  {}", parsed_corr.as_str());

    // Forward it downstream
    println!(
        "Forwarding {} = {}",
        parsed_corr.header_name(),
        parsed_corr.as_str()
    );

    // -- RequestId: unique per HTTP request at the edge --
    println!("\n--- RequestId per-request tracing ---");
    let request_id = RequestId::new();
    println!("Header name:  {}", request_id.header_name());
    println!("Header value: {}", request_id.as_str());

    // Parse from an incoming X-Request-Id header
    let parsed_req: RequestId = "550e8400-e29b-41d4-a716-446655440000"
        .parse()
        .expect("valid UUID");
    println!("Parsed req:   {}", parsed_req.as_str());

    println!("\nAll trace_context examples passed.");
}
