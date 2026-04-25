#![cfg(feature = "nats")]
// Integration test — requires a running NATS server with JetStream enabled.
// Skipped automatically when NATS_URL is not set.

use std::time::Duration;

use api_bones_test::nats::AuditCapture;

#[tokio::test]
async fn audit_capture_assert_no_events_when_idle() {
    let Ok(url) = std::env::var("NATS_URL") else {
        eprintln!("NATS_URL not set — skipping NATS integration test");
        return;
    };

    let client = async_nats::connect(&url).await.expect("connect to NATS");
    let capture = AuditCapture::new(&client, "test-service")
        .await
        .expect("create AuditCapture");

    capture.assert_no_events(Duration::from_millis(100)).await;
}
