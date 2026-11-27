// SPDX-License-Identifier: MIT
//! The one Connect client transport of the fleet.
//!
//! Every Rust Connect client is built from [`HttpTransport`], a
//! [`connectrpc::client::ClientTransport`] with three constructors and no
//! options:
//!
//! - [`HttpTransport::fleet`]: mutual TLS to a peer inside the fleet, with
//!   connection establishment bounded by [`FLEET_CONNECT_BOUND`];
//! - [`HttpTransport::tls`]: TLS to a public endpoint;
//! - [`HttpTransport::h2c`]: prior-knowledge cleartext HTTP/2, for loopback
//!   and contract tests.
//!
//! Three behaviours hold on every constructor.
//!
//! **HTTP/2 only, one connection per peer.** ALPN offers `h2` alone, so the
//! pool knows before connecting that the connection multiplexes: a burst of
//! calls to one peer waits on one connect and shares one connection as
//! streams, instead of opening one connection per call.
//!
//! **A request that never ran is replayed once.** When the peer provably did
//! not process a request (the connection refused it before opening a stream,
//! a GOAWAY with `NO_ERROR` excluded its stream, or the peer refused the
//! stream with `REFUSED_STREAM`, the cases RFC 9113 section 8.7 defines as
//! not processed), the transport sends it once more on a fresh connection,
//! inside the caller's deadline, with the timeout header rewritten to the
//! time remaining. The verdict is taken where the failure arose: a GOAWAY
//! counts only when it reached the request's own stream. A request that may
//! have run (an error-coded GOAWAY, an I/O error, a keepalive timeout, a
//! reset after the request was taken, a streaming request body) goes back to
//! the caller unchanged. See [`never_ran`] and [`reach_of`].
//!
//! **Connections are driven by one process runtime.** The transport starts a
//! runtime of its own on first construction and never drops it; connection
//! tasks, the connector (DNS, TCP, TLS) and the transport's timers run on it.
//! The connections are driven through `h2::client` directly, with one stream
//! opened at a time per connection, so a request the connection never opened
//! can move to a fresh connection.
//! A connection therefore outlives the runtime of the caller that opened it,
//! and a client shared across runtimes keeps working when one of them ends.
//!
//! Timings are fixed constants: [`POOL_IDLE_TIMEOUT`],
//! [`KEEPALIVE_INTERVAL`] and [`KEEPALIVE_TIMEOUT`] apply everywhere,
//! [`FLEET_CONNECT_BOUND`] to [`HttpTransport::fleet`].
//!
//! The transport logs `connection_opened` and `connection_closed` per
//! connection, and `request_failed` and `retry` events carrying the
//! never-ran classification, so a client shows which requests a server
//! drain caught and which it replayed.
//!
//! **The browser.** On `wasm32` the same type has one constructor,
//! `HttpTransport::fetch()`, with no arguments: it calls the host's
//! `globalThis.fetch` (a Window, a Worker or Node 18+), over connectrpc's
//! client core with its `client` features off. The credentials mode follows the
//! request's lane (`include` on the session lane, `omit` under `/public/`),
//! the BFF's CSRF token is echoed per origin and shared between wasm modules on
//! one page, an eligible read goes as a Connect GET, a failed fetch is
//! `unavailable`, and dropping a call aborts its fetch. Server streams are read
//! through the response's `ReadableStream`. A request that never ran is not
//! replayed there.
//!
//! **One deadline source.** A [`Deadline`] in the request's extensions makes
//! either transport state the time remaining in the timeout header and answer
//! `deadline_exceeded` when it passes before the response head arrives.
//!
//! This crate depends on no api-bones major, so a consumer on any major can
//! use it, and service-kit can hand one transport to every SDK.
//!
//! ```rust,no_run
//! use api_bones_transport::HttpTransport;
//! use connectrpc::client::ClientConfig;
//!
//! let transport = HttpTransport::h2c();
//! let config = ClientConfig::new("http://127.0.0.1:8080".parse().unwrap());
//! # let _ = (transport, config);
//! ```

#[cfg(not(target_arch = "wasm32"))]
mod body;
#[cfg(not(target_arch = "wasm32"))]
mod classify;
#[cfg(not(target_arch = "wasm32"))]
mod conn;
#[cfg(not(target_arch = "wasm32"))]
mod connector;
mod deadline;
#[cfg(not(target_arch = "wasm32"))]
mod driver;
#[cfg(target_arch = "wasm32")]
mod fetch;
#[cfg(not(target_arch = "wasm32"))]
mod peer;
#[cfg(not(target_arch = "wasm32"))]
mod pool;
#[cfg(not(target_arch = "wasm32"))]
mod replay;
#[cfg(not(target_arch = "wasm32"))]
mod transport;

use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
pub use body::ResponseBody;
#[cfg(not(target_arch = "wasm32"))]
pub use classify::{NeverRan, failed_to_connect, never_ran, reach_of};
pub use deadline::Deadline;
#[cfg(target_arch = "wasm32")]
pub use fetch::{FetchBody, HttpTransport};
#[cfg(not(target_arch = "wasm32"))]
pub use transport::HttpTransport;

/// Bound on establishing a connection to a peer inside the fleet (DNS, TCP and
/// TLS), applied by [`HttpTransport::fleet`].
///
/// In-cluster establishment takes single-digit milliseconds (failing attempts
/// complete in 9-51 ms on a loaded CI cluster), and a lost SYN is not
/// retransmitted before Linux's 1 s initial RTO, so a connect not done by then
/// cannot finish within a 1 s RPC budget anyway. 250 ms is about five times the
/// slowest observed establishment and leaves 750 ms of a 1 s budget for the
/// response.
pub const FLEET_CONNECT_BOUND: Duration = Duration::from_millis(250);

/// How long an unused pooled connection is kept before it is closed.
pub const POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(90);

/// Interval between HTTP/2 keepalive PINGs, sent while the connection is idle
/// too. The value the server side of the fleet uses.
pub const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30);

/// How long a keepalive PING may go unanswered before the connection is
/// closed.
pub const KEEPALIVE_TIMEOUT: Duration = Duration::from_secs(10);
