// SPDX-License-Identifier: MIT
//! The connector: DNS, TCP and TLS under the establishment bounds, handing
//! back one erased I/O type whether the connection is cleartext or TLS.

use std::sync::Arc;
use std::time::Duration;

use connectrpc::ConnectError;
use connectrpc::rustls::ClientConfig;
use connectrpc::rustls::pki_types::ServerName;
use http::Uri;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

/// What an HTTP/2 connection runs over.
pub(crate) trait Stream: AsyncRead + AsyncWrite + Send + Unpin {}

impl<T: AsyncRead + AsyncWrite + Send + Unpin> Stream for T {}

/// A connection's byte stream, cleartext or TLS.
pub(crate) type Io = Box<dyn Stream>;

/// The one place a stream becomes an [`Io`].
pub(crate) fn erase(stream: impl Stream + 'static) -> Io {
    Box::new(stream)
}

/// Opens byte streams to peers: cleartext when it holds no TLS config,
/// ALPN-checked TLS when it does.
#[derive(Clone)]
pub(crate) struct Connector {
    tls: Option<TlsConnector>,
    tcp: Duration,
    establishment: Duration,
}

impl Connector {
    /// `tls` must offer `h2` alone in its ALPN list. `tcp` bounds each TCP
    /// connect attempt, `establishment` the whole of DNS, TCP and TLS.
    pub(crate) fn new(
        tls: Option<Arc<ClientConfig>>,
        tcp: Duration,
        establishment: Duration,
    ) -> Self {
        Self {
            tls: tls.map(TlsConnector::from),
            tcp,
            establishment,
        }
    }

    /// Opens a stream to the peer `uri` names. A bound that elapses surfaces
    /// as an `Unavailable` [`ConnectError`] caused by
    /// `tokio::time::error::Elapsed`, the shape [`crate::failed_to_connect`]
    /// recognizes.
    pub(crate) async fn connect(&self, uri: &Uri) -> Result<Io, ConnectError> {
        match tokio::time::timeout(self.establishment, self.establish(uri)).await {
            Ok(established) => established,
            Err(elapsed) => Err(ConnectError::unavailable(format!(
                "connection establishment did not complete within {:?}",
                self.establishment
            ))
            .with_source(elapsed)),
        }
    }

    async fn establish(&self, uri: &Uri) -> Result<Io, ConnectError> {
        let Some(host) = uri.host() else {
            return Err(ConnectError::invalid_argument("request URI has no host"));
        };
        // `Uri::host` keeps an IPv6 literal's brackets.
        let host = host.trim_start_matches('[').trim_end_matches(']');
        let default_port = if uri.scheme_str() == Some("https") {
            443
        } else {
            80
        };
        let tcp = self
            .tcp_connect(host, uri.port_u16().unwrap_or(default_port))
            .await?;
        let Some(tls) = &self.tls else {
            return Ok(erase(tcp));
        };
        let name = ServerName::try_from(host.to_owned()).map_err(|_| {
            ConnectError::invalid_argument(format!("{host} is not a valid TLS server name"))
        })?;
        let stream = tls
            .connect(name, tcp)
            .await
            .map_err(|e| ConnectError::unavailable_from_transport("TLS handshake failed", e))?;
        if stream.get_ref().1.alpn_protocol() != Some(&b"h2"[..]) {
            return Err(ConnectError::unavailable(
                "the peer did not negotiate h2 through ALPN",
            ));
        }
        Ok(erase(stream))
    }

    async fn tcp_connect(&self, host: &str, port: u16) -> Result<TcpStream, ConnectError> {
        let addresses = tokio::net::lookup_host((host, port))
            .await
            .map_err(|e| ConnectError::unavailable_from_transport("DNS lookup failed", e))?;
        let mut last = None;
        for address in addresses {
            match tokio::time::timeout(self.tcp, TcpStream::connect(address)).await {
                Ok(Ok(stream)) => {
                    stream.set_nodelay(true).map_err(|e| {
                        ConnectError::unavailable_from_transport("TCP_NODELAY failed", e)
                    })?;
                    return Ok(stream);
                }
                Ok(Err(e)) => {
                    last = Some(ConnectError::unavailable_from_transport(
                        format!("TCP connect to {address} failed"),
                        e,
                    ));
                }
                Err(elapsed) => {
                    last = Some(
                        ConnectError::unavailable(format!(
                            "TCP connect to {address} did not complete within {:?}",
                            self.tcp
                        ))
                        .with_source(elapsed),
                    );
                }
            }
        }
        Err(last.unwrap_or_else(|| ConnectError::unavailable("DNS lookup returned no address")))
    }
}
