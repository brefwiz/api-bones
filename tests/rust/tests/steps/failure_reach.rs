// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! The Rust half of `../../../features/connect_failure_reach.feature`.

use api_bones::Reach;
use api_bones_transport::{HttpTransport, reach_of};
use buffa_types::google::protobuf::{Empty, EmptyView};
use connectrpc::client::{CallOptions, ClientConfig, call_unary};
use connectrpc::{ConnectRpcService, Router, Server, Spec, StreamType};
use cucumber::{given, then, when};

use crate::world::FailureReachWorld;

#[given("a peer that refuses connections")]
async fn given_refusing_peer(world: &mut FailureReachWorld) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    world.port = listener.local_addr().expect("addr").port();
    drop(listener);
}

#[given("a peer that resets the connection once it has the request")]
async fn given_resetting_peer(world: &mut FailureReachWorld) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    world.port = listener.local_addr().expect("addr").port();
    world.peer = Some(crate::world::spawn_resetting_peer(listener));
}

#[given("a peer that answers every call with an error")]
async fn given_failing_peer(world: &mut FailureReachWorld) {
    let bound = Server::bind("127.0.0.1:0").await.expect("bind");
    world.port = bound.local_addr().expect("addr").port();
    // No service is registered, so the peer answers with a coded error.
    world.peer = Some(tokio::spawn(async move {
        bound
            .serve_with_service(ConnectRpcService::new(Router::new()))
            .await
            .expect("failing peer stopped serving");
    }));
}

#[when("the workload classifies the failure of a fleet call to it")]
async fn when_call(world: &mut FailureReachWorld) {
    let config = ClientConfig::new(
        format!("http://127.0.0.1:{}", world.port)
            .parse()
            .expect("uri"),
    );
    let result = call_unary::<_, Empty, EmptyView>(
        &HttpTransport::h2c(),
        &config,
        Spec::client("/probe.v1.Probe/Ping", StreamType::Unary),
        Empty::default(),
        CallOptions::default(),
    )
    .await;
    let Err(failure) = result else {
        panic!("the call must fail");
    };
    world.reach = Some(reach_of(&failure));
}

#[then(expr = "the failure reach is {string}")]
fn then_reach(world: &mut FailureReachWorld, expected: String) {
    assert_eq!(world.reach.expect("no call made").as_str(), expected);
}

#[then("the failure is replayable")]
fn then_replayable(world: &mut FailureReachWorld) {
    assert!(Reach::is_replayable(world.reach.expect("no call made")));
}

#[then("the failure is not replayable")]
fn then_not_replayable(world: &mut FailureReachWorld) {
    assert!(!Reach::is_replayable(world.reach.expect("no call made")));
}
