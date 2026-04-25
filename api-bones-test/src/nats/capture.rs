use std::time::Duration;

use async_nats::Client;
use async_nats::jetstream::{self, consumer::PullConsumer};
use futures_util::TryStreamExt as _;

/// `JetStream` consumer fixture for asserting audit event emissions.
///
/// Subscribes to `audit.{service}.>` and exposes helpers to consume events
/// in tests.
pub struct AuditCapture {
    consumer: PullConsumer,
}

impl AuditCapture {
    /// Subscribe to `audit.{service}.>` on the given NATS client.
    ///
    /// # Errors
    ///
    /// Returns an error if the stream or consumer cannot be created.
    pub async fn new(client: &Client, service: &str) -> Result<Self, async_nats::Error> {
        let js = jetstream::new(client.clone());
        let stream_name = format!("AUDIT_{}", service.to_uppercase());
        let subject = format!("audit.{service}.>");

        let stream = js
            .get_or_create_stream(jetstream::stream::Config {
                name: stream_name,
                subjects: vec![subject],
                ..Default::default()
            })
            .await?;

        let consumer = stream
            .get_or_create_consumer(
                "test-capture",
                jetstream::consumer::pull::Config {
                    durable_name: Some("test-capture".to_owned()),
                    ..Default::default()
                },
            )
            .await?;

        Ok(Self { consumer })
    }

    /// Fetch the next raw message within `timeout`.
    ///
    /// Returns the raw bytes of the first message, or `None` on timeout.
    ///
    /// # Errors
    ///
    /// Returns an error on `JetStream` failures.
    pub async fn next_raw(&self, timeout: Duration) -> Result<Option<Vec<u8>>, async_nats::Error> {
        let mut messages = self.consumer.fetch().max_messages(1).messages().await?;

        match tokio::time::timeout(timeout, messages.try_next()).await {
            Ok(Ok(Some(msg))) => {
                let payload = msg.payload.to_vec();
                msg.ack().await?;
                Ok(Some(payload))
            }
            Ok(Ok(None)) => Ok(None),
            Ok(Err(e)) => Err(e),
            Err(_elapsed) => Ok(None),
        }
    }

    /// Assert that no messages arrive within `during`.
    ///
    /// # Panics
    ///
    /// Panics if a message arrives during `during`.
    pub async fn assert_no_events(&self, during: Duration) {
        let result = self.next_raw(during).await;
        match result {
            Ok(Some(payload)) => panic!(
                "expected no audit events but received {} bytes",
                payload.len()
            ),
            Ok(None) => {}
            Err(e) => panic!("AuditCapture error while asserting no events: {e}"),
        }
    }
}
