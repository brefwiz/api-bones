// SPDX-License-Identifier: MIT
import assert from "node:assert/strict";
import { Given, Then, When, World } from "@cucumber/cucumber";
import { ROOT_CONTEXT, context, propagation, trace, TraceFlags, type Context, type ContextManager, type TextMapPropagator, type TextMapSetter, type TextMapGetter } from "@opentelemetry/api";
import { W3CBaggagePropagator, W3CTraceContextPropagator } from "@opentelemetry/core";
import type { UnaryRequest } from "@connectrpc/connect";
import { injectTraceContext, addTraceContextInterceptor, captureTraceContext } from "@brefwiz/api-bones-otel";
import { unaryMethodFixture } from "./support.ts";

const identity = ["authorization", "x-org-id", "x-org-path", "x-subject-id"];
const selected = trace.setSpanContext(ROOT_CONTEXT, { traceId: "4bf92f3577b34da6a3ce929d0e0e4736", spanId: "00f067aa0ba902b7", traceFlags: TraceFlags.SAMPLED });
const selectedContext = propagation.setBaggage(selected, propagation.createBaggage({ sample: { value: "selected" } }));
const manager: ContextManager = {
  active: () => selectedContext,
  with: (_ctx, fn, thisArg, ...args) => fn.call(thisArg, ...args),
  bind: (_ctx, target) => target,
  enable() { return this; },
  disable() { return this; },
};
context.setGlobalContextManager(manager);
class ExtensionPropagator implements TextMapPropagator {
  constructor(private readonly fail: boolean) {}
  inject<C>(ctx: Context, carrier: C, setter: TextMapSetter<C>): void {
    new W3CTraceContextPropagator().inject(ctx, carrier, setter);
    new W3CBaggagePropagator().inject(ctx, carrier, setter);
    setter.set(carrier, "x-custom-context", ctx === selectedContext ? "selected-context" : "wrong-context");
    for (const key of identity) setter.set(carrier, key.toUpperCase(), "telemetry-identity");
    if (this.fail) throw new Error("instrumentation failure");
  }
  extract<C>(ctx: Context, _carrier: C, _getter: TextMapGetter<C>): Context { return ctx; }
  fields(): string[] { return ["traceparent", "baggage", "x-custom-context", ...identity]; }
}
interface PropagationWorld extends World { propagationHeaders: Headers; propagationFails: boolean }
Given("propagation identity headers are {string}", function (this: PropagationWorld, state: string) {
  this.propagationHeaders = new Headers();
  if (state === "present") for (const key of identity) this.propagationHeaders.set(key, "caller-identity");
  else assert.equal(state, "absent");
});
Given("a propagator carrying trace context, baggage and custom context", function (this: PropagationWorld) { this.propagationFails = false; });
Given("a propagator that fails after writing permitted context", function (this: PropagationWorld) { this.propagationFails = true; });
When("telemetry is injected through the {string} entry", async function (this: PropagationWorld, entry: string) {
  propagation.disable();
  assert.equal(propagation.setGlobalPropagator(new ExtensionPropagator(this.propagationFails)), true);
  if (entry === "helper") {
    const captured = captureTraceContext();
    assert.equal(captured, selectedContext);
    injectTraceContext(this.propagationHeaders, captured);
  } else {
    assert.equal(entry, "wrapper");
    const request = { header: this.propagationHeaders, method: unaryMethodFixture("pkg.v1.Telemetry", "Send"), stream: false } as UnaryRequest;
    const reached = new Error("next transport reached");
    await assert.rejects(addTraceContextInterceptor()(async (req) => {
      assert.equal(req, request);
      throw reached;
    })(request), (error) => error === reached);
  }
});
Then("request identity headers remain {string}", function (this: PropagationWorld, state: string) {
  for (const key of identity) assert.equal(this.propagationHeaders.get(key), state === "present" ? "caller-identity" : null);
});
function arrived(headers: Headers): void {
  assert.equal(headers.get("traceparent"), "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01");
  assert.equal(headers.get("baggage"), "sample=selected");
  assert.equal(headers.get("x-custom-context"), "selected-context");
}
Then("trace context, baggage and custom context reach the request", function (this: PropagationWorld) { arrived(this.propagationHeaders); });
Then("permitted context written before failure reaches the request", function (this: PropagationWorld) { arrived(this.propagationHeaders); });
