// SPDX-License-Identifier: MIT
//! [`HttpTransport`]: the h2 client behind every fleet Connect client.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use connectrpc::ConnectError;
use connectrpc::client::{
    BoxFuture, ClientBody, ClientTransport, DEFAULT_ESTABLISHMENT_TIMEOUT,
    DEFAULT_TCP_CONNECT_TIMEOUT,
};
use connectrpc::rustls::ClientConfig;
use http::{Request, Response, Uri};

use crate::body::ResponseBody;
use crate::connector::Connector;
use crate::pool::Pool;
use crate::{FLEET_CONNECT_BOUND, driver, replay};

/// The fleet's Connect client transport: HTTP/2 only, one connection per
/// peer, a request that never ran replayed once, connections driven by the
/// process driver runtime. See the [crate documentation](crate).
///
/// Clones share one connection pool.
#[derive(Clone)]
pub struct HttpTransport {
    mode: Mode,
    pool: Arc<Pool>,
}

impl HttpTransport {
    /// Mutual TLS to a peer inside the fleet. Connection establishment (DNS,
    /// TCP, TLS) is bounded by [`FLEET_CONNECT_BOUND`].
    ///
    /// `tls` is cloned and its ALPN list replaced by `h2` alone; its
    /// certificate resolver and verifier are shared with the clone, so
    /// rotation through them is unaffected. Only `https://` URIs are served.
    ///
    /// # Panics
    ///
    /// Panics when the process driver runtime, started on the first
    /// construction, cannot start its threads.
    #[must_use]
    pub fn fleet(tls: Arc<ClientConfig>) -> Self {
        Self::build(Mode::Fleet, Some(tls))
    }

    /// TLS to a public endpoint, HTTP/2 only, with connectrpc's default
    /// establishment bounds ([`DEFAULT_TCP_CONNECT_TIMEOUT`] per address,
    /// [`DEFAULT_ESTABLISHMENT_TIMEOUT`] overall). Only `https://` URIs are
    /// served.
    ///
    /// # Panics
    ///
    /// Panics when the process driver runtime, started on the first
    /// construction, cannot start its threads.
    #[must_use]
    pub fn tls(tls: Arc<ClientConfig>) -> Self {
        Self::build(Mode::Tls, Some(tls))
    }

    /// Prior-knowledge cleartext HTTP/2, for loopback and contract tests,
    /// with connectrpc's default establishment bounds. Only `http://` URIs
    /// are served.
    ///
    /// # Panics
    ///
    /// Panics when the process driver runtime, started on the first
    /// construction, cannot start its threads.
    #[must_use]
    pub fn h2c() -> Self {
        Self::build(Mode::H2c, None)
    }

    /// The one constructor every public one funnels into.
    fn build(mode: Mode, tls: Option<Arc<ClientConfig>>) -> Self {
        driver::handle();
        let tls = tls.map(|config| {
            // ALPN offers `h2` alone; the clone shares the config's resolver
            // and verifier.
            let mut config = (*config).clone();
            config.alpn_protocols = vec![b"h2".to_vec()];
            Arc::new(config)
        });
        let (tcp, establishment) = mode.bounds();
        Self {
            mode,
            pool: Arc::new(Pool::new(Connector::new(tls, tcp, establishment))),
        }
    }
}

impl fmt::Debug for HttpTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpTransport")
            .field("mode", &self.mode.as_str())
            .finish_non_exhaustive()
    }
}

impl ClientTransport for HttpTransport {
    type ResponseBody = ResponseBody;
    type Error = ConnectError;

    fn send(
        &self,
        request: Request<ClientBody>,
    ) -> BoxFuture<'static, Result<Response<ResponseBody>, ConnectError>> {
        if let Some(refusal) = self.mode.refuse(request.uri()) {
            return Box::pin(std::future::ready(Err(refusal)));
        }
        Box::pin(replay::send(Arc::clone(&self.pool), request))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Fleet,
    Tls,
    H2c,
}

impl Mode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Fleet => "fleet",
            Self::Tls => "tls",
            Self::H2c => "h2c",
        }
    }

    /// The `(TCP connect, whole establishment)` bounds of the mode.
    const fn bounds(self) -> (Duration, Duration) {
        match self {
            Self::Fleet => (FLEET_CONNECT_BOUND, FLEET_CONNECT_BOUND),
            Self::Tls | Self::H2c => (DEFAULT_TCP_CONNECT_TIMEOUT, DEFAULT_ESTABLISHMENT_TIMEOUT),
        }
    }

    /// Refuses a URI whose scheme the mode does not serve, so a TLS transport
    /// never falls back to cleartext and a cleartext one never meets a TLS
    /// endpoint.
    fn refuse(self, uri: &Uri) -> Option<ConnectError> {
        let scheme = uri.scheme_str();
        match self {
            Self::H2c if scheme == Some("https") => Some(ConnectError::invalid_argument(
                "HttpTransport::h2c() received an https:// URI; use HttpTransport::tls or fleet",
            )),
            Self::Fleet | Self::Tls if scheme == Some("http") => {
                Some(ConnectError::invalid_argument(format!(
                    "HttpTransport::{}() received an http:// URI; use HttpTransport::h2c for cleartext",
                    self.as_str()
                )))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_names_the_mode_only() {
        assert_eq!(
            format!("{:?}", HttpTransport::h2c()),
            r#"HttpTransport { mode: "h2c", .. }"#
        );
    }

    #[test]
    fn each_mode_refuses_the_other_scheme() {
        let https: Uri = "https://peer.example:443".parse().expect("uri");
        let http: Uri = "http://127.0.0.1:8080".parse().expect("uri");
        assert!(Mode::H2c.refuse(&https).is_some());
        assert!(Mode::H2c.refuse(&http).is_none());
        assert!(Mode::Fleet.refuse(&http).is_some());
        assert!(Mode::Tls.refuse(&http).is_some());
        assert!(Mode::Tls.refuse(&https).is_none());
    }

    #[test]
    fn only_fleet_carries_the_fleet_bound() {
        assert_eq!(
            Mode::Fleet.bounds(),
            (FLEET_CONNECT_BOUND, FLEET_CONNECT_BOUND)
        );
        assert_eq!(
            Mode::Tls.bounds(),
            (DEFAULT_TCP_CONNECT_TIMEOUT, DEFAULT_ESTABLISHMENT_TIMEOUT)
        );
    }
}
