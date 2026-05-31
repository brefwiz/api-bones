// SPDX-License-Identifier: MIT
//! Network-level request primitives.
//!
//! Gated by the `axum` feature. Provides utilities for inspecting connection
//! metadata attached to incoming [`http::Request`]s.

pub mod client_ip;
