// SPDX-License-Identifier: MIT
use super::native_generated::*;
use super::native_world::{NativeWorld, NoopWake, Recording};
use cucumber::{given, then, when};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use std::{
    future::Future,
    task::{Context, Poll, Waker},
};
#[given("a generated native client backed by a recording transport")]
fn recording(w: &mut NativeWorld) {
    w.transport = Some(Recording::default())
}
#[given("a generated native client backed by a denying transport")]
fn denying(w: &mut NativeWorld) {
    w.transport = Some(Recording {
        denied: true,
        ..Default::default()
    })
}
#[given("a generated native client backed by a transport that fails one acknowledgement")]
fn fail_ack(w: &mut NativeWorld) {
    let transport = Recording::default();
    transport.fail_ack_once.store(true, Ordering::Relaxed);
    w.transport = Some(transport)
}
#[given("a generated native client backed by a transport that interrupts one acknowledgement")]
fn interrupt_ack(w: &mut NativeWorld) {
    let transport = Recording::default();
    transport.interrupt_ack_once.store(true, Ordering::Relaxed);
    w.transport = Some(transport)
}
#[given("a generated native client backed by a transport that fails one negative acknowledgement")]
fn fail_nak(w: &mut NativeWorld) {
    let transport = Recording::default();
    transport.fail_nak_once.store(true, Ordering::Relaxed);
    w.transport = Some(transport)
}
#[given("a proto descriptor with native operations and no RPC methods")]
fn descriptor(w: &mut NativeWorld) {
    let bytes = include_bytes!(concat!(env!("OUT_DIR"), "/native.pb"));
    w.first_generation = Some(api_bones_sdk_gen::native::generate_sources(bytes).unwrap())
}
#[given("an ambiguous native descriptor")]
fn ambiguous(_: &mut NativeWorld) {}
fn publication() -> Publication {
    Publication {
        message_id: "stable-1".into(),
        entity_id: "entity-1".into(),
        payload: vec![0, 255],
    }
}
#[when("it publishes opaque bytes with stable message identity")]
async fn publish(w: &mut NativeWorld) {
    let c = FixtureNativeV1NativeQueueClient::new(w.transport.clone().unwrap());
    w.publish_result = Some(
        c.publish(publication(), CallOptions::default())
            .await
            .unwrap(),
    );
}
#[when("it publishes through the buf plugin generated client")]
async fn plugin_publish(w: &mut NativeWorld) {
    let c = FixtureNativeV1NativeQueueClient::new(w.transport.clone().unwrap());
    w.publish_result = Some(
        c.publish(publication(), CallOptions::default())
            .await
            .unwrap(),
    );
}
#[when("it receives one durable delivery")]
async fn receive(w: &mut NativeWorld) {
    let c = FixtureNativeV1NativeQueueClient::new(w.transport.clone().unwrap());
    let mut s = c.deliver(CallOptions::default()).await.unwrap();
    let delivery = s.next().await.unwrap().unwrap();
    w.handle = Some(delivery.ack_handle.clone());
    w.delivery = Some(delivery);
}
#[when("it attempts publication")]
async fn denied(w: &mut NativeWorld) {
    let c = FixtureNativeV1NativeQueueClient::new(w.transport.clone().unwrap());
    w.error = c.publish(publication(), CallOptions::default()).await.err()
}
#[when("it publishes with cancellation and deadline options")]
async fn options(w: &mut NativeWorld) {
    let c = FixtureNativeV1NativeQueueClient::new(w.transport.clone().unwrap());
    let deadline = Instant::now() + Duration::from_secs(1);
    let cancel = Arc::new(AtomicBool::new(false));
    w.expected_deadline = Some(deadline);
    w.expected_cancel = Some(Arc::clone(&cancel));
    let o = CallOptions {
        deadline: Some(deadline),
        cancelled: Some(cancel),
    };
    c.publish(publication(), o).await.unwrap();
}
#[when("it publishes with an already cancelled option")]
async fn cancelled(w: &mut NativeWorld) {
    let c = FixtureNativeV1NativeQueueClient::new(w.transport.clone().unwrap());
    let o = CallOptions {
        deadline: None,
        cancelled: Some(Arc::new(AtomicBool::new(true))),
    };
    w.error = c.publish(publication(), o).await.err()
}
#[when("it publishes with an expired deadline")]
async fn expired(w: &mut NativeWorld) {
    let c = FixtureNativeV1NativeQueueClient::new(w.transport.clone().unwrap());
    let o = CallOptions {
        deadline: Some(Instant::now() - Duration::from_secs(1)),
        cancelled: None,
    };
    w.error = c.publish(publication(), o).await.err()
}
#[when("native clients are generated twice")]
fn generated(w: &mut NativeWorld) {
    let bytes = include_bytes!(concat!(env!("OUT_DIR"), "/native.pb"));
    w.second_generation = Some(api_bones_sdk_gen::native::generate_sources(bytes).unwrap())
}
#[when("native client generation is attempted")]
fn attempt(w: &mut NativeWorld) {
    let bytes = include_bytes!(concat!(env!("OUT_DIR"), "/native-ambiguous.pb"));
    let error = api_bones_sdk_gen::native::generate_sources(bytes)
        .unwrap_err()
        .to_string();
    w.generation_failed = true;
    w.generation_error = Some(error)
}
#[when("checked generated output differs from descriptor generation")]
fn stale(w: &mut NativeWorld) {
    let bytes = include_bytes!(concat!(env!("OUT_DIR"), "/native.pb"));
    let base = std::env::temp_dir().join(format!("native-stale-{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let rust = base.join("native.rs");
    let ts = base.join("native.ts");
    std::fs::write(&rust, "stale").unwrap();
    std::fs::write(&ts, "stale").unwrap();
    let error = api_bones_sdk_gen::native::generate_descriptor_set_bytes(bytes, &rust, &ts, true)
        .unwrap_err()
        .to_string();
    w.stale_errors.push(error);
    let (fresh, _) = api_bones_sdk_gen::native::generate_sources(bytes).unwrap();
    std::fs::write(&rust, fresh).unwrap();
    let error = api_bones_sdk_gen::native::generate_descriptor_set_bytes(bytes, &rust, &ts, true)
        .unwrap_err()
        .to_string();
    w.stale_errors.push(error);
}
#[when("it starts publication with an already cancelled option")]
async fn starts_cancelled(w: &mut NativeWorld) {
    cancelled(w).await
}
#[then("transport receives one typed publication")]
fn saw_publish(w: &mut NativeWorld) {
    assert_eq!(
        w.transport
            .as_ref()
            .unwrap()
            .calls
            .lock()
            .unwrap()
            .as_slice(),
        ["publish"]
    );
    let publication = w.transport.as_ref().unwrap().publication.lock().unwrap();
    let publication = publication.as_ref().unwrap();
    assert_eq!(publication.message_id, "stable-1");
    assert_eq!(publication.entity_id, "entity-1");
    assert_eq!(publication.payload, [0, 255]);
    assert_eq!(w.publish_result, Some(PublishResult { duplicate: false }));
}
#[then("delivery can be acknowledged")]
async fn ack(w: &mut NativeWorld) {
    assert_delivery(w);
    let c = FixtureNativeV1NativeQueueClient::new(w.transport.clone().unwrap());
    c.acknowledge(w.handle.as_ref().unwrap(), CallOptions::default())
        .await
        .unwrap();
    assert_eq!(
        w.transport
            .as_ref()
            .unwrap()
            .calls
            .lock()
            .unwrap()
            .as_slice(),
        ["deliver", "acknowledge"]
    );
    assert_eq!(
        w.transport
            .as_ref()
            .unwrap()
            .handle_bytes
            .lock()
            .unwrap()
            .as_deref(),
        Some([7].as_slice())
    );
    assert_default_options(w);
}
#[then("delivery can be negatively acknowledged")]
async fn nak(w: &mut NativeWorld) {
    assert_delivery(w);
    let c = FixtureNativeV1NativeQueueClient::new(w.transport.clone().unwrap());
    c.negative_acknowledge(w.handle.as_ref().unwrap(), CallOptions::default())
        .await
        .unwrap();
    assert_eq!(
        w.transport
            .as_ref()
            .unwrap()
            .calls
            .lock()
            .unwrap()
            .as_slice(),
        ["deliver", "negative_acknowledge"]
    );
    assert_eq!(
        w.transport
            .as_ref()
            .unwrap()
            .handle_bytes
            .lock()
            .unwrap()
            .as_deref(),
        Some([7].as_slice())
    );
    assert_default_options(w);
}
fn assert_delivery(w: &NativeWorld) {
    let delivery = w.delivery.as_ref().unwrap();
    assert_eq!(delivery.message_id, "m1");
    assert_eq!(delivery.entity_id, "e1");
    assert_eq!(delivery.payload, [1]);
    assert_eq!(delivery.ack_handle.value, [7]);
}
fn assert_default_options(w: &NativeWorld) {
    let binding = w.transport.as_ref().unwrap().options.lock().unwrap();
    let options = binding.as_ref().unwrap();
    assert!(options.deadline.is_none());
    assert!(options.cancelled.is_none());
}
#[when("it acknowledges the delivery")]
async fn ack_when(w: &mut NativeWorld) {
    let c = FixtureNativeV1NativeQueueClient::new(w.transport.clone().unwrap());
    c.acknowledge(w.handle.as_ref().unwrap(), CallOptions::default())
        .await
        .unwrap()
}
#[when("it negatively acknowledges the delivery")]
async fn nak_when(w: &mut NativeWorld) {
    let c = FixtureNativeV1NativeQueueClient::new(w.transport.clone().unwrap());
    c.negative_acknowledge(w.handle.as_ref().unwrap(), CallOptions::default())
        .await
        .unwrap()
}
#[when("first acknowledgement attempt fails")]
async fn ack_fails(w: &mut NativeWorld) {
    let c = FixtureNativeV1NativeQueueClient::new(w.transport.clone().unwrap());
    assert_transport_failure(
        c.acknowledge(w.handle.as_ref().unwrap(), CallOptions::default()),
        "ack_failed",
    )
    .await
}
#[when("first acknowledgement attempt is interrupted")]
fn ack_interrupted(w: &mut NativeWorld) {
    let c = FixtureNativeV1NativeQueueClient::new(w.transport.clone().unwrap());
    let mut future = Box::pin(c.acknowledge(w.handle.as_ref().unwrap(), CallOptions::default()));
    let waker = Waker::from(Arc::new(NoopWake));
    let mut context = Context::from_waker(&waker);
    assert!(matches!(future.as_mut().poll(&mut context), Poll::Pending));
    drop(future)
}
#[when("first negative acknowledgement attempt fails")]
async fn nak_fails(w: &mut NativeWorld) {
    let c = FixtureNativeV1NativeQueueClient::new(w.transport.clone().unwrap());
    assert_transport_failure(
        c.negative_acknowledge(w.handle.as_ref().unwrap(), CallOptions::default()),
        "nak_failed",
    )
    .await
}

async fn assert_transport_failure(
    future: impl Future<Output = Result<(), NativeError>>,
    expected_code: &str,
) {
    let error = future.await.unwrap_err();
    assert!(matches!(error, NativeError::Transport { ref code } if code == expected_code));
}
#[then("reusing the acknowledgement handle is refused")]
async fn reused(w: &mut NativeWorld) {
    let c = FixtureNativeV1NativeQueueClient::new(w.transport.clone().unwrap());
    let error = c
        .acknowledge(w.handle.as_ref().unwrap(), CallOptions::default())
        .await
        .unwrap_err();
    assert!(matches!(error,NativeError::Transport{ref code} if code=="ack_handle_used"))
}
#[then("reusing the negatively acknowledged handle is refused without dispatch")]
async fn nak_reused(w: &mut NativeWorld) {
    let c = FixtureNativeV1NativeQueueClient::new(w.transport.clone().unwrap());
    let before = w.transport.as_ref().unwrap().calls.lock().unwrap().len();
    let ack = c
        .acknowledge(w.handle.as_ref().unwrap(), CallOptions::default())
        .await
        .unwrap_err();
    let nak = c
        .negative_acknowledge(w.handle.as_ref().unwrap(), CallOptions::default())
        .await
        .unwrap_err();
    assert!(matches!(ack, NativeError::Transport { ref code } if code == "ack_handle_used"));
    assert!(matches!(nak, NativeError::Transport { ref code } if code == "ack_handle_used"));
    assert_eq!(
        w.transport.as_ref().unwrap().calls.lock().unwrap().len(),
        before
    );
}
#[then("generated client reports permission denied")]
fn permission(w: &mut NativeWorld) {
    assert!(matches!(
        w.error.as_ref(),
        Some(NativeError::PermissionDenied { .. })
    ))
}
#[then("transport receives cancellation and deadline options")]
fn saw_options(w: &mut NativeWorld) {
    let binding = w.transport.as_ref().unwrap().options.lock().unwrap();
    let o = binding.as_ref().unwrap();
    assert!(o.deadline.is_some());
    assert!(!o.cancelled.as_ref().unwrap().load(Ordering::Relaxed));
    assert_eq!(o.deadline, w.expected_deadline);
    assert!(Arc::ptr_eq(
        o.cancelled.as_ref().unwrap(),
        w.expected_cancel.as_ref().unwrap()
    ));
}
#[then("generated client reports cancellation")]
fn cancelled_error(w: &mut NativeWorld) {
    assert!(matches!(w.error.as_ref(), Some(NativeError::Cancelled)))
}
#[then("generated client reports deadline exceeded")]
fn deadline_error(w: &mut NativeWorld) {
    assert!(matches!(
        w.error.as_ref(),
        Some(NativeError::DeadlineExceeded)
    ))
}
#[then("native transport receives no call")]
fn no_call(w: &mut NativeWorld) {
    assert!(
        w.transport
            .as_ref()
            .unwrap()
            .calls
            .lock()
            .unwrap()
            .is_empty()
    )
}
#[then("generated outputs are byte identical")]
fn identical(w: &mut NativeWorld) {
    assert_eq!(w.first_generation, w.second_generation)
}
#[then("descriptor RPC inventory remains empty")]
fn empty(_: &mut NativeWorld) {
    let bytes = include_bytes!(concat!(env!("OUT_DIR"), "/native.pb"));
    assert_eq!(
        api_bones_sdk_gen::native::rpc_method_count(bytes).unwrap(),
        0
    )
}
#[then("generation fails with a descriptor error")]
fn failed(w: &mut NativeWorld) {
    assert!(w.generation_failed);
    assert_eq!(
        w.generation_error.as_deref(),
        Some("duplicate native operation \"publish\" in Ambiguous")
    )
}
#[then("stale output is reported for both targets")]
fn stale_reported(w: &mut NativeWorld) {
    assert!(w.stale_errors[0].contains("native.rs"));
    assert!(w.stale_errors[1].contains("native.ts"));
}
#[then("cancellation is returned by the async result")]
fn async_cancel(w: &mut NativeWorld) {
    cancelled_error(w)
}
#[given("native descriptors with invalid and colliding identifiers")]
fn invalid_identifiers(_: &mut NativeWorld) {}
#[given("an ordinary RPC descriptor without native annotations")]
fn rpc_only(w: &mut NativeWorld) {
    let bytes = include_bytes!(concat!(env!("OUT_DIR"), "/rpc-only.pb"));
    let files = ["fixtures/rpc-only.proto".to_owned()];
    w.plugin_empty = api_bones_sdk_gen::native::generate_plugin_output(bytes, &files, "rust")
        .unwrap()
        .is_none()
        && api_bones_sdk_gen::native::generate_plugin_output(bytes, &files, "typescript")
            .unwrap()
            .is_none();
}
#[given("native declarations selected from multiple source directories")]
fn multi_directory(w: &mut NativeWorld) {
    let bytes = include_bytes!(concat!(env!("OUT_DIR"), "/native-multi.pb"));
    let files = [
        "fixtures/native.proto".to_owned(),
        "fixtures/other/native-second.proto".to_owned(),
    ];
    let (_, rust) = api_bones_sdk_gen::native::generate_plugin_output(bytes, &files, "rust")
        .unwrap()
        .unwrap();
    let (_, ts) = api_bones_sdk_gen::native::generate_plugin_output(bytes, &files, "typescript")
        .unwrap()
        .unwrap();
    w.plugin_multi = rust.contains("FixtureNativeV1NativeQueueClient")
        && rust.contains("FixtureSecondV1SecondQueueClient")
        && ts.contains("FixtureNativeV1NativeQueueClient")
        && ts.contains("FixtureSecondV1SecondQueueClient");
}
#[when("native plugin generation is attempted")]
fn plugin_attempt(_: &mut NativeWorld) {}
#[then("no native client files are emitted for either target")]
fn no_plugin_files(w: &mut NativeWorld) {
    assert!(w.plugin_empty)
}
#[then("one output per target contains every selected native client")]
fn all_plugin_clients(w: &mut NativeWorld) {
    assert!(w.plugin_multi)
}
#[when("native client generation validates identifiers")]
fn validate_identifiers(w: &mut NativeWorld) {
    for name in [
        "native-leading-digit",
        "native-keyword",
        "native-constructor",
        "native-transport",
        "native-collision",
        "native-rust2024-keyword",
    ] {
        let bytes = match name {
            "native-leading-digit" => {
                include_bytes!(concat!(env!("OUT_DIR"), "/native-leading-digit.pb")).as_slice()
            }
            "native-keyword" => {
                include_bytes!(concat!(env!("OUT_DIR"), "/native-keyword.pb")).as_slice()
            }
            "native-constructor" => {
                include_bytes!(concat!(env!("OUT_DIR"), "/native-constructor.pb")).as_slice()
            }
            "native-transport" => {
                include_bytes!(concat!(env!("OUT_DIR"), "/native-transport.pb")).as_slice()
            }
            "native-rust2024-keyword" => {
                include_bytes!(concat!(env!("OUT_DIR"), "/native-rust2024-keyword.pb")).as_slice()
            }
            _ => include_bytes!(concat!(env!("OUT_DIR"), "/native-collision.pb")).as_slice(),
        };
        w.identifier_errors.push(
            api_bones_sdk_gen::native::generate_sources(bytes)
                .unwrap_err()
                .to_string(),
        );
    }
    let bytes = include_bytes!(concat!(env!("OUT_DIR"), "/native-same-name.pb"));
    let (rust, ts) = api_bones_sdk_gen::native::generate_sources(bytes).unwrap();
    w.disambiguated = rust.contains("FixtureAlphaV1SameNameClient")
        && rust.contains("FixtureBetaV1SameNameClient")
        && ts.contains("FixtureAlphaV1SameNameClient")
        && ts.contains("FixtureBetaV1SameNameClient");
    let collision = include_bytes!(concat!(env!("OUT_DIR"), "/native-symbol-collision.pb"));
    w.identifier_errors.push(
        api_bones_sdk_gen::native::generate_sources(collision)
            .unwrap_err()
            .to_string(),
    );
}
#[then("unsafe identifiers are rejected and package names disambiguate clients")]
fn identifiers_rejected(w: &mut NativeWorld) {
    assert_eq!(w.identifier_errors.len(), 7);
    assert!(
        w.identifier_errors[..6]
            .iter()
            .all(|error| error.contains("native operation"))
    );
    assert_eq!(
        w.identifier_errors[6],
        "native services collide on generated client name \"FooBarQueueClient\""
    );
    assert!(w.disambiguated)
}
#[then("acknowledgement retry succeeds with same handle")]
async fn ack_retry(w: &mut NativeWorld) {
    let c = FixtureNativeV1NativeQueueClient::new(w.transport.clone().unwrap());
    c.acknowledge(w.handle.as_ref().unwrap(), CallOptions::default())
        .await
        .unwrap();
    assert_eq!(
        w.transport
            .as_ref()
            .unwrap()
            .handle_bytes
            .lock()
            .unwrap()
            .as_deref(),
        Some([7].as_slice())
    )
}
#[then("negative acknowledgement retry succeeds with same handle")]
async fn nak_retry(w: &mut NativeWorld) {
    let c = FixtureNativeV1NativeQueueClient::new(w.transport.clone().unwrap());
    c.negative_acknowledge(w.handle.as_ref().unwrap(), CallOptions::default())
        .await
        .unwrap();
    assert_eq!(
        w.transport
            .as_ref()
            .unwrap()
            .handle_bytes
            .lock()
            .unwrap()
            .as_deref(),
        Some([7].as_slice())
    )
}
