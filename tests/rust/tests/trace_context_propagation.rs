// SPDX-License-Identifier: MIT
use api_bones::propagation::inject_current;
use api_bones_tower::TraceContextLayer;
use brefwiz_cucumber_steps::TwoPass;
use cucumber::{World, given, then, when};
use http::{HeaderMap, Request, Response};
use opentelemetry::{
    Context, KeyValue,
    baggage::BaggageExt,
    global,
    propagation::{Extractor, Injector, TextMapPropagator, text_map_propagator::FieldIter},
    trace::{SpanContext, SpanId, TraceContextExt, TraceFlags, TraceId, TraceState},
};
use opentelemetry_sdk::propagation::{BaggagePropagator, TraceContextPropagator};
use tower::{Layer, Service};

const IDENTITY: [&str; 4] = ["authorization", "x-org-id", "x-org-path", "x-subject-id"];

#[derive(Debug)]
struct ExtensionPropagator {
    fail: bool,
    fields: Vec<String>,
}
impl TextMapPropagator for ExtensionPropagator {
    fn inject_context(&self, cx: &Context, injector: &mut dyn Injector) {
        TraceContextPropagator::new().inject_context(cx, injector);
        BaggagePropagator::new().inject_context(cx, injector);
        injector.set("x-custom-context", cx.get::<String>().unwrap().clone());
        for key in IDENTITY {
            injector.set(key.to_uppercase().as_str(), "telemetry-identity".into());
        }
        if self.fail {
            panic!("instrumentation failure");
        }
    }
    fn extract_with_context(&self, cx: &Context, _: &dyn Extractor) -> Context {
        cx.clone()
    }
    fn fields(&self) -> FieldIter<'_> {
        FieldIter::new(&self.fields)
    }
}

#[derive(Debug, Default, World)]
struct PropagationWorld {
    headers: HeaderMap,
    fail: bool,
}

#[given(expr = "propagation identity headers are {string}")]
fn identity(world: &mut PropagationWorld, state: String) {
    world.headers.clear();
    if state == "present" {
        for key in IDENTITY {
            world
                .headers
                .insert(key, "caller-identity".parse().unwrap());
        }
    } else {
        assert_eq!(state, "absent");
    }
}
#[given(expr = "a propagator carrying trace context, baggage and custom context")]
fn normal(world: &mut PropagationWorld) {
    world.fail = false;
}
#[given(expr = "a propagator that fails after writing permitted context")]
fn failing(world: &mut PropagationWorld) {
    world.fail = true;
}
#[when(expr = "telemetry is injected through the {string} entry")]
async fn inject(world: &mut PropagationWorld, entry: String) {
    global::set_text_map_propagator(ExtensionPropagator {
        fail: world.fail,
        fields: vec![],
    });
    let span = SpanContext::new(
        TraceId::from_hex("4bf92f3577b34da6a3ce929d0e0e4736").unwrap(),
        SpanId::from_hex("00f067aa0ba902b7").unwrap(),
        TraceFlags::SAMPLED,
        true,
        TraceState::default(),
    );
    let cx = Context::new()
        .with_remote_span_context(span)
        .with_baggage([KeyValue::new("sample", "selected")])
        .with_value("selected-context".to_owned());
    let guard = cx.attach();
    if entry == "helper" {
        inject_current(&mut world.headers);
    } else {
        assert_eq!(entry, "wrapper");
        let mut service =
            TraceContextLayer::new().layer(tower::service_fn(|request: Request<()>| async move {
                Ok::<_, std::convert::Infallible>(Response::new(request.into_parts().0.headers))
            }));
        let mut request = Request::new(());
        *request.headers_mut() = world.headers.clone();
        // Dispatch occurs synchronously while the explicitly selected context is attached.
        let response = service.call(request);
        drop(guard);
        world.headers = response.await.unwrap().into_body();
        return;
    }
    drop(guard);
}
#[then(expr = "request identity headers remain {string}")]
fn unchanged(world: &mut PropagationWorld, state: String) {
    for key in IDENTITY {
        assert_eq!(
            world.headers.get(key).map(|v| v.to_str().unwrap()),
            if state == "present" {
                Some("caller-identity")
            } else {
                None
            }
        );
    }
}
fn context_arrived(world: &PropagationWorld) {
    assert_eq!(
        world.headers["traceparent"],
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
    );
    assert_eq!(world.headers["baggage"], "sample=selected");
    assert_eq!(world.headers["x-custom-context"], "selected-context");
}
#[then(expr = "trace context, baggage and custom context reach the request")]
fn all_context(world: &mut PropagationWorld) {
    context_arrived(world);
}
#[then(expr = "permitted context written before failure reaches the request")]
fn prior_context(world: &mut PropagationWorld) {
    context_arrived(world);
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    TwoPass {
        skip_tags: vec!["wip".into()],
        isolated_tag: "isolated".into(),
        parallel: 1,
    }
    .run::<PropagationWorld>(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../features/trace_context_propagation.feature"
        )
        .to_owned(),
    )
    .await;
}
