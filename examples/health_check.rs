//! Health check examples.
//!
//! Demonstrates building `ReadinessResponse` with multiple component checks
//! using the typestate builder pattern.
//!
//! Run: `cargo run --example health_check`

use api_bones::health::{HealthCheck, HealthStatus, ReadinessResponse};

fn main() {
    // -- All healthy --
    let healthy = ReadinessResponse::builder()
        .version("1.2.0")
        .service_id("booking-service")
        .add_check("postgres:connection", HealthCheck::pass("datastore"))
        .add_check("redis:ping", HealthCheck::pass("datastore"))
        .build();

    println!("=== All healthy ===");
    println!("Status:      {}", healthy.status);
    println!("HTTP status: {}", healthy.http_status());

    #[cfg(feature = "serde")]
    println!(
        "JSON:\n{}",
        serde_json::to_string_pretty(&healthy).expect("serialize")
    );

    // -- Degraded (warn) --
    let degraded = ReadinessResponse::builder()
        .version("1.2.0")
        .service_id("booking-service")
        .add_check("postgres:connection", HealthCheck::pass("datastore"))
        .add_check(
            "redis:latency",
            HealthCheck::warn("datastore", "p99 > 200ms"),
        )
        .build();

    println!("\n=== Degraded ===");
    println!("Status:      {}", degraded.status);
    println!("HTTP status: {}", degraded.http_status());

    // -- Unhealthy (fail) --
    let unhealthy = ReadinessResponse::builder()
        .version("1.2.0")
        .service_id("booking-service")
        .add_check(
            "postgres:connection",
            HealthCheck::fail("datastore", "connection refused"),
        )
        .add_check("redis:ping", HealthCheck::pass("datastore"))
        .build();

    println!("\n=== Unhealthy ===");
    println!("Status:      {}", unhealthy.status);
    println!("HTTP status: {}", unhealthy.http_status());

    // -- Using the HealthCheck builder --
    let custom_check = HealthCheck::builder()
        .component_type("system")
        .status(HealthStatus::Warn)
        .output("disk usage at 85%")
        .time("2026-04-06T12:00:00Z")
        .build();

    println!("\n=== Custom check via builder ===");
    println!("Component: {}", custom_check.component_type);
    println!("Status:    {}", custom_check.status);
    println!("Output:    {:?}", custom_check.output);
    println!("Time:      {:?}", custom_check.time);
}
