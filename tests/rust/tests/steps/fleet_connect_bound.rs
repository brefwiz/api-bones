// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! The Rust half of `../../../features/connect_fleet_connect_bound.feature`.

use std::time::{Duration, Instant};

use api_bones::connect::{
    client_tls_config, failed_to_connect, fleet_client_builder, fleet_http_client,
};
use buffa_types::google::protobuf::{Empty, EmptyView};
use connectrpc::client::{CallOptions, ClientConfig, call_unary};
use connectrpc::{ConnectRpcService, Router, Server, Spec, StreamType};
use cucumber::{given, then, when};

use crate::world::FleetConnectBoundWorld;

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

#[when("the workload calls it over a fleet client")]
async fn when_call(world: &mut FleetConnectBoundWorld) {
    let _ = connectrpc::rustls::crypto::aws_lc_rs::default_provider().install_default();
    let (client, scheme) = if world.plaintext {
        (fleet_client_builder().plaintext_http2_only(), "http")
    } else {
        (fleet_http_client(client_tls_config()), "https")
    };
    let config = ClientConfig::new(
        format!("{scheme}://127.0.0.1:{}", world.port)
            .parse()
            .expect("uri"),
    );
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
    let Err(failure) = result else {
        panic!("the call must fail");
    };
    world.failure = Some(failure);
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
    assert!(
        failed_to_connect(failure),
        "not a connect failure: {failure:?}"
    );
}

#[then("the failure is not reported as a failure to connect")]
fn then_not_connect_failure(world: &mut FleetConnectBoundWorld) {
    let failure = world.failure.as_ref().expect("no failure");
    assert!(
        !failed_to_connect(failure),
        "wrongly a connect failure: {failure:?}"
    );
}
