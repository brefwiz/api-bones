//! Test helpers for `api-bones` consumers.
//!
//! Provides builder ergonomics for api-bones types and assertion helpers for
//! axum and reqwest responses so services share a single test vocabulary.
//!
//! # Feature flags
//!
//! | Feature | Adds |
//! |---------|------|
//! | `builders` (default) | [`builders`] — pure-Rust builders, no IO |
//! | `axum` | [`axum`] — `axum-test` assertion helpers and `TestServer` |
//! | `reqwest` | [`reqwest`] — reqwest assertion helpers |
//! | `nats` | [`nats`] — `JetStream` `AuditCapture` fixture |
//!
//! # Quick start — builders
//!
//! ```rust,ignore
//! use api_bones::error::ErrorCode;
//! use api_bones_test::builders::{FakeApiResponse, FakePaginated, FakeProblem};
//!
//! let resp = FakeApiResponse::new(42u32).build();
//! assert_eq!(resp.data, 42);
//!
//! let page = FakePaginated::new(vec![1u32, 2, 3]).build();
//! assert_eq!(page.total_count, 3);
//!
//! let err = FakeProblem::new(ErrorCode::ValidationFailed)
//!     .field("/email", "must be a valid email")
//!     .build();
//! assert!(!err.errors.is_empty());
//! ```
//!
//! # Quick start — axum assertions
//!
//! ```rust,no_run
//! # #[cfg(feature = "axum")]
//! # async fn run() {
//! use axum::Router;
//! use api_bones_test::axum::TestServer;
//!
//! let app = Router::new(); // your router
//! let server = TestServer::new(app);
//! let payload: u32 = server.get_envelope("/items/1").await;
//! # }
//! ```
//!
//! # Quick start — reqwest assertions
//!
//! ```rust,no_run
//! # #[cfg(feature = "reqwest")]
//! # async fn run() {
//! use api_bones_test::reqwest::assert_envelope_reqwest;
//!
//! let client = ::reqwest::Client::new();
//! let resp = client.get("http://localhost:3000/items/1").send().await.unwrap();
//! let payload: u32 = assert_envelope_reqwest(resp).await;
//! # }
//! ```
//!
//! # Quick start — NATS audit capture
//!
//! ```rust,no_run
//! # #[cfg(feature = "nats")]
//! # async fn run() {
//! use std::time::Duration;
//! use api_bones_test::nats::AuditCapture;
//!
//! // Requires a running NATS server (use testcontainers in CI)
//! let client = async_nats::connect("nats://localhost:4222").await.unwrap();
//! let capture = AuditCapture::new(&client, "my-service").await.unwrap();
//! capture.assert_no_events(Duration::from_millis(100)).await;
//! # }
//! ```

#[cfg(feature = "builders")]
pub mod builders;

#[cfg(feature = "axum")]
pub mod axum;

#[cfg(feature = "reqwest")]
pub mod reqwest;

#[cfg(feature = "nats")]
pub mod nats;
