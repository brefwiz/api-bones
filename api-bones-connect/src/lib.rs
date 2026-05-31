//! Connect RPC adapter primitives for api-bones services (ADR-0096).
//!
//! This crate re-exports the [`api_bones::connect`] module surface for
//! consumers who want a dedicated dependency rather than enabling the
//! `connect` feature on `api-bones` directly.
//!
//! # Usage
//!
//! ```toml
//! [dependencies]
//! api-bones-connect = "0.1"
//! ```
//!
//! ```rust,ignore
//! use api_bones_connect::{
//!     DomainErrorKind, IntoDomainErrorKind, domain_to_connect,
//!     chrono_to_timestamp, parse_uuid, build_page, ConnectOptionExt as _,
//! };
//! ```

pub use api_bones::connect::*;
