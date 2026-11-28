// SPDX-License-Identifier: MIT
//! The connector: DNS, TCP and TLS under the establishment bounds, handing
//! back one erased I/O type whether the connection is cleartext or TLS.

use std::fmt::Write as _;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::time::{Duration, Instant};

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

/// The three phases of establishing a connection, in the order they run.
#[derive(Clone, Copy)]
enum Phase {
    Dns,
    Tcp,
    Tls,
}

impl Phase {
    const ALL: [Self; 3] = [Self::Dns, Self::Tcp, Self::Tls];

    const fn name(self) -> &'static str {
        match self {
            Self::Dns => "dns",
            Self::Tcp => "tcp",
            Self::Tls => "tls",
        }
    }
}

/// What each phase of one establishment cost, readable while the establishment
/// is still running so a tripped bound can say where the time went.
struct Phases {
    started: Instant,
    /// Index into [`Phase::ALL`] of the phase running or last run.
    current: AtomicU8,
    /// Microseconds each phase took; zero while it has not finished.
    finished_us: [AtomicU64; 3],
}

impl Phases {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            current: AtomicU8::new(0),
            finished_us: Default::default(),
        }
    }

    /// Runs `work` as `phase` and records how long it took.
    async fn run<T>(&self, phase: Phase, work: impl Future<Output = T>) -> T {
        self.current.store(phase as u8, Ordering::SeqCst);
        let began = Instant::now();
        let output = work.await;
        let micros = u64::try_from(began.elapsed().as_micros()).unwrap_or(u64::MAX);
        self.finished_us[phase as usize].store(micros.max(1), Ordering::SeqCst);
        output
    }

    /// `dns 0.300ms, tcp 0.100ms, tls running for 249.600ms`: each phase that
    /// started, with its cost or, for the one still running, its time so far.
    fn report(&self) -> String {
        let current = usize::from(self.current.load(Ordering::SeqCst));
        let mut finished = 0_u64;
        let mut report = String::new();
        for phase in Phase::ALL.into_iter().take(current + 1) {
            let micros = self.finished_us[phase as usize].load(Ordering::SeqCst);
            finished += micros;
            if !report.is_empty() {
                report.push_str(", ");
            }
            let _ = if micros == 0 {
                let total = u64::try_from(self.started.elapsed().as_micros()).unwrap_or(u64::MAX);
                write!(
                    report,
                    "{} running for {}",
                    phase.name(),
                    millis(total.saturating_sub(finished))
                )
            } else {
                write!(report, "{} {}", phase.name(), millis(micros))
            };
        }
        report
    }
}

fn millis(micros: u64) -> String {
    format!("{}.{:03}ms", micros / 1000, micros % 1000)
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
        let phases = Phases::new();
        match tokio::time::timeout(self.establishment, self.establish(uri, &phases)).await {
            Ok(established) => {
                tracing::debug!(
                    target: "api_bones_transport",
                    event = "connection_established",
                    peer = %crate::peer::name(uri),
                    phases = %phases.report(),
                );
                established
            }
            Err(elapsed) => {
                let report = phases.report();
                tracing::warn!(
                    target: "api_bones_transport",
                    event = "connection_establishment_bound_exceeded",
                    peer = %crate::peer::name(uri),
                    bound_ms = u64::try_from(self.establishment.as_millis()).unwrap_or(u64::MAX),
                    phases = %report,
                );
                Err(ConnectError::unavailable(format!(
                    "connection establishment did not complete within {:?} ({report})",
                    self.establishment
                ))
                .with_source(elapsed))
            }
        }
    }

    async fn establish(&self, uri: &Uri, phases: &Phases) -> Result<Io, ConnectError> {
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
            .tcp_connect(host, uri.port_u16().unwrap_or(default_port), phases)
            .await?;
        let Some(tls) = &self.tls else {
            return Ok(erase(tcp));
        };
        let name = ServerName::try_from(host.to_owned()).map_err(|_| {
            ConnectError::invalid_argument(format!("{host} is not a valid TLS server name"))
        })?;
        let stream = phases
            .run(Phase::Tls, tls.connect(name, tcp))
            .await
            .map_err(|e| ConnectError::unavailable_from_transport("TLS handshake failed", e))?;
        if stream.get_ref().1.alpn_protocol() != Some(&b"h2"[..]) {
            return Err(ConnectError::unavailable(
                "the peer did not negotiate h2 through ALPN",
            ));
        }
        Ok(erase(stream))
    }

    async fn tcp_connect(
        &self,
        host: &str,
        port: u16,
        phases: &Phases,
    ) -> Result<TcpStream, ConnectError> {
        let addresses = phases
            .run(Phase::Dns, tokio::net::lookup_host((host, port)))
            .await
            .map_err(|e| ConnectError::unavailable_from_transport("DNS lookup failed", e))?;
        phases.run(Phase::Tcp, self.dial_any(addresses)).await
    }

    async fn dial_any(
        &self,
        addresses: impl Iterator<Item = std::net::SocketAddr>,
    ) -> Result<TcpStream, ConnectError> {
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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::AtomicU64;

    use connectrpc::rustls::RootCertStore;
    use connectrpc::rustls::crypto::aws_lc_rs::default_provider;
    use tokio::net::TcpListener;

    use super::*;

    #[tokio::test]
    async fn the_report_lists_each_started_phase_with_its_cost() {
        let phases = Phases::new();
        phases.run(Phase::Dns, std::future::ready(())).await;
        phases
            .run(Phase::Tcp, tokio::time::sleep(Duration::from_millis(2)))
            .await;
        let report = phases.report();
        assert!(
            report.starts_with("dns ") && report.contains(", tcp "),
            "{report}"
        );
        assert!(report.ends_with("ms"), "{report}");
        assert!(
            !report.contains("running") && !report.contains("tls"),
            "{report}"
        );
    }

    #[tokio::test]
    async fn the_report_shows_the_running_phase_with_its_time_so_far() {
        let phases = Phases::new();
        phases.run(Phase::Dns, std::future::ready(())).await;
        let outcome = tokio::time::timeout(
            Duration::from_millis(5),
            phases.run(Phase::Tcp, std::future::pending::<()>()),
        )
        .await;
        assert!(outcome.is_err());
        let report = phases.report();
        assert!(
            report.starts_with("dns ") && report.contains(", tcp running for "),
            "{report}"
        );
    }

    #[test]
    fn durations_render_in_milliseconds_with_microsecond_precision() {
        assert_eq!(millis(5), "0.005ms");
        assert_eq!(millis(1_500), "1.500ms");
        assert_eq!(millis(249_600), "249.600ms");
    }

    #[tokio::test]
    async fn a_tripped_bound_names_the_phase_still_running() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        // Accepts and then says nothing, so the TLS handshake never finishes.
        let silent = tokio::spawn(async move {
            let held = listener.accept().await.expect("accept");
            std::future::pending::<()>().await;
            drop(held);
        });
        let config = ClientConfig::builder_with_provider(Arc::new(default_provider()))
            .with_safe_default_protocol_versions()
            .expect("versions")
            .with_root_certificates(RootCertStore::empty())
            .with_no_client_auth();
        let connector = Connector::new(
            Some(Arc::new(config)),
            Duration::from_secs(1),
            Duration::from_millis(100),
        );
        let uri: Uri = format!("https://127.0.0.1:{port}/").parse().expect("uri");

        let error = connector
            .connect(&uri)
            .await
            .err()
            .expect("the bound trips");
        let message = error.to_string();
        assert!(
            message.contains("did not complete within 100ms (dns ")
                && message.contains(", tls running for "),
            "{message}"
        );
        silent.abort();
    }

    /// Records every event's fields as `name=value` text.
    struct Capture(Arc<Mutex<Vec<String>>>);

    struct Fields(String);

    impl tracing::field::Visit for Fields {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            let _ = write!(self.0, "{}={value:?} ", field.name());
        }
    }

    impl tracing::Subscriber for Capture {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }

        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            static NEXT: AtomicU64 = AtomicU64::new(1);
            tracing::span::Id::from_u64(NEXT.fetch_add(1, Ordering::Relaxed))
        }

        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

        fn event(&self, event: &tracing::Event<'_>) {
            let mut fields = Fields(String::new());
            event.record(&mut fields);
            self.0.lock().expect("events").push(fields.0);
        }

        fn enter(&self, _: &tracing::span::Id) {}

        fn exit(&self, _: &tracing::span::Id) {}
    }

    #[tokio::test]
    async fn events_name_the_peer_without_its_userinfo_path_or_query() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let _guard = tracing::subscriber::set_default(Capture(Arc::clone(&events)));
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let secret = format!("http://alice:hunter2@127.0.0.1:{port}/private/path?token=abc");
        let connector = Connector::new(None, Duration::from_secs(1), Duration::from_secs(5));
        let uri: Uri = secret.parse().expect("uri");

        connector.connect(&uri).await.expect("cleartext connect");
        let rejected = {
            let config = ClientConfig::builder_with_provider(Arc::new(default_provider()))
                .with_safe_default_protocol_versions()
                .expect("versions")
                .with_root_certificates(RootCertStore::empty())
                .with_no_client_auth();
            let tripping = Connector::new(
                Some(Arc::new(config)),
                Duration::from_secs(1),
                Duration::from_millis(50),
            );
            let uri: Uri = secret.replace("http:", "https:").parse().expect("uri");
            tripping.connect(&uri).await.expect_err("the bound trips")
        };
        drop(rejected);

        let events = events.lock().expect("events");
        let established = events
            .iter()
            .find(|e| e.contains("connection_established"))
            .expect("the success event");
        let exceeded = events
            .iter()
            .find(|e| e.contains("connection_establishment_bound_exceeded"))
            .expect("the bound event");
        for event in [established, exceeded] {
            assert!(
                event.contains(&format!("peer=127.0.0.1:{port} ")),
                "{event}"
            );
            for leaked in ["alice", "hunter2", "private", "token"] {
                assert!(!event.contains(leaked), "{leaked} in {event}");
            }
        }
    }
}
