// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! The Rust half of `../../../features/connect_fleet_connect_bound.feature`.

use std::sync::{LazyLock, Mutex, MutexGuard, Once, PoisonError};
use std::time::{Duration, Instant};

use api_bones_transport::{FLEET_CONNECT_BOUND, HttpTransport, failed_to_connect};
use buffa_types::google::protobuf::{Empty, EmptyView};
use connectrpc::client::{CallOptions, ClientConfig, call_unary};
use connectrpc::{ConnectRpcService, Router, Server, Spec, StreamType};
use cucumber::{given, then, when};
use serde_json::Value;

use crate::world::FleetConnectBoundWorld;

use super::tracing_fields::{Fields, TelemetryFields as EventFields};

/// Every event the transport emits, with its name and fields.
static EVENTS: LazyLock<Mutex<Vec<EventFields>>> = LazyLock::new(|| Mutex::new(Vec::new()));

fn events() -> MutexGuard<'static, Vec<EventFields>> {
    EVENTS.lock().unwrap_or_else(PoisonError::into_inner)
}

struct EventLayer;

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for EventLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        if event.metadata().target() == "api_bones_transport" {
            let mut fields = Fields(EventFields::new());
            event.record(&mut fields);
            fields
                .0
                .insert("level".into(), event.metadata().level().to_string().into());
            events().push(fields.0);
        }
    }
}

fn capture_events() {
    use tracing_subscriber::prelude::*;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        tracing::subscriber::set_global_default(tracing_subscriber::registry().with(EventLayer))
            .expect("transport tracing subscriber");
    });
}

/// The events naming `event` for the peer on `port`.
fn events_named(event: &str, port: u16) -> Vec<EventFields> {
    let peer = format!("127.0.0.1:{port}");
    events()
        .iter()
        .filter(|e| e.get("event").and_then(Value::as_str) == Some(event))
        .filter(|e| e.get("peer").and_then(Value::as_str) == Some(peer.as_str()))
        .cloned()
        .collect()
}

#[given("a fleet peer that accepts connections and never answers")]
async fn given_silent_peer(world: &mut FleetConnectBoundWorld) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    world.port = listener.local_addr().expect("addr").port();
    // Sockets are held open and never written to, so the TLS handshake hangs.
    world.peer = Some(tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            held.push(socket);
        }
        drop(held);
    }));
}

#[given("a fleet peer that refuses connections")]
async fn given_refusing_peer(world: &mut FleetConnectBoundWorld) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    world.port = listener.local_addr().expect("addr").port();
    drop(listener);
}

#[given("a fleet peer that answers every call with a failure")]
async fn given_failing_peer(world: &mut FleetConnectBoundWorld) {
    let bound = Server::bind("127.0.0.1:0").await.expect("bind");
    world.port = bound.local_addr().expect("addr").port();
    world.plaintext = true;
    // No service is registered, so the peer answers the request with an error
    // after the connection is established.
    world.peer = Some(tokio::spawn(async move {
        let _ = bound
            .serve_with_service(ConnectRpcService::new(Router::new()))
            .await;
    }));
}

#[given("the peer is addressed by a URL carrying credentials and a path")]
fn given_decorated_url(world: &mut FleetConnectBoundWorld) {
    world.decorated = true;
}

#[when("the workload calls it over a fleet client")]
async fn when_call(world: &mut FleetConnectBoundWorld) {
    capture_events();
    let _ = connectrpc::rustls::crypto::aws_lc_rs::default_provider().install_default();
    let (client, scheme) = if world.plaintext {
        (HttpTransport::h2c(), "http")
    } else {
        (
            HttpTransport::fleet(api_bones::connect::client_tls_config()),
            "https",
        )
    };
    let authority = if world.decorated {
        format!("alice:hunter2@127.0.0.1:{}/private/path", world.port)
    } else {
        format!("127.0.0.1:{}", world.port)
    };
    let config = ClientConfig::new(format!("{scheme}://{authority}").parse().expect("uri"));
    let started = Instant::now();
    let result = call_unary::<_, Empty, EmptyView>(
        &client,
        &config,
        Spec::client("/probe.v1.Probe/Ping", StreamType::Unary),
        Empty::default(),
        CallOptions::default(),
    )
    .await;
    world.elapsed = Some(started.elapsed());
    world.failure = Some(result.expect_err("the call must fail"));
}

#[then("the call fails within one second")]
fn then_within_budget(world: &mut FleetConnectBoundWorld) {
    let elapsed = world.elapsed.expect("no call made");
    assert!(
        elapsed < Duration::from_secs(1),
        "call took {elapsed:?}, over the one second budget"
    );
}

#[then("the failure is reported as a failure to connect")]
fn then_connect_failure(world: &mut FleetConnectBoundWorld) {
    let failure = world.failure.as_ref().expect("no failure");
    let connecting = failed_to_connect(failure);
    assert!(connecting, "not a connect failure: {failure:?}");
}

#[then("the failure is not reported as a failure to connect")]
fn then_not_connect_failure(world: &mut FleetConnectBoundWorld) {
    let failure = world.failure.as_ref().expect("no failure");
    let connecting = failed_to_connect(failure);
    assert!(!connecting, "wrongly a connect failure: {failure:?}");
}

/// Every link of the failure's `source()` chain, outermost first: the
/// transport names the phases on the connect error it wraps.
fn failure_message(world: &FleetConnectBoundWorld) -> String {
    let mut links = Vec::new();
    let mut current: Option<&(dyn std::error::Error + 'static)> =
        Some(world.failure.as_ref().expect("no failure"));
    while let Some(link) = current {
        links.push(link.to_string());
        current = link.source();
    }
    links.join(": ")
}

#[then("the failure names the dns and tcp phases by their cost")]
fn then_names_finished_phases(world: &mut FleetConnectBoundWorld) {
    let message = failure_message(world);
    assert!(
        message.contains("dns ") && message.contains("tcp ") && message.contains("ms, "),
        "finished phases not named: {message}"
    );
    assert!(
        !message.contains("dns running") && !message.contains("tcp running"),
        "a finished phase reported as running: {message}"
    );
}

#[then("the failure names the tls phase as still running")]
fn then_names_running_phase(world: &mut FleetConnectBoundWorld) {
    let message = failure_message(world);
    assert!(
        message.contains("tls running for "),
        "running phase not named: {message}"
    );
}

#[then("the bound-exceeded event is a warning naming the peer, the bound and the phases")]
fn then_bound_event(world: &mut FleetConnectBoundWorld) {
    let found = events_named("connection_establishment_bound_exceeded", world.port);
    assert_eq!(found.len(), 1, "expected one bound event: {found:?}");
    let event = &found[0];
    assert_eq!(event["level"], "WARN", "{event:?}");
    assert_eq!(
        u128::from(event["bound_ms"].as_u64().expect("bound_ms")),
        FLEET_CONNECT_BOUND.as_millis(),
        "{event:?}"
    );
    let phases = event["phases"].as_str().expect("phases");
    assert!(phases.contains("tls running for "), "{event:?}");
    let failure = failure_message(world);
    assert!(failure.contains(phases), "{failure} vs {event:?}");
}

#[then("the established event is a debug event naming the peer and the phases")]
fn then_established_event(world: &mut FleetConnectBoundWorld) {
    let found = events_named("connection_established", world.port);
    assert_eq!(found.len(), 1, "expected one established event: {found:?}");
    let event = &found[0];
    assert_eq!(event["level"], "DEBUG", "{event:?}");
    let phases = event["phases"].as_str().expect("phases");
    assert!(
        phases.contains("dns ") && phases.contains("tcp "),
        "{event:?}"
    );
}

#[then("no event carries the credentials or the path")]
fn then_events_redacted(world: &mut FleetConnectBoundWorld) {
    let peer = format!("127.0.0.1:{}", world.port);
    let seen: Vec<_> = events()
        .iter()
        .filter(|e| e.get("peer").and_then(Value::as_str) == Some(peer.as_str()))
        .cloned()
        .collect();
    assert!(!seen.is_empty(), "no event named the peer {peer}");
    for event in seen {
        for (name, value) in &event {
            let text = value.to_string();
            for leaked in ["alice", "hunter2", "private"] {
                assert!(
                    !text.contains(leaked),
                    "{leaked} leaked in {name}: {event:?}"
                );
            }
        }
    }
}
