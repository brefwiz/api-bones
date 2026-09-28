// SPDX-License-Identifier: MIT
import { Given, Then, When } from "@cucumber/cucumber";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { AckHandle, NativeError, FixtureNativeV1NativeQueueClient, type CallOptions, type Delivery, type NativeTransport, type Publication } from "../generated/native.js";
import { FixtureNativeV1NativeQueueClient as PluginNativeQueueClient } from "../generated/plugin-native.js";

class RecordingTransport implements NativeTransport {
  readonly calls: string[] = [];
  options?: CallOptions;
  publication?: Publication;
  handle?: AckHandle;
  failAckOnce = false;
  interruptAckOnce = false;
  failNakOnce = false;
  constructor(readonly denied = false) {}
  async publish(operation: string, request: Publication, options: CallOptions) {
    this.calls.push(operation); this.options = options; this.publication = request;
    assert.equal(request.messageId, "stable-1"); assert.deepEqual(request.payload, new Uint8Array([0, 255]));
    if (this.denied) throw new NativeError("permission_denied", "provider refused publication");
    return { duplicate: false };
  }
  async *deliver(operation: string): AsyncIterable<Delivery> { this.calls.push(operation); yield { messageId: "m1", entityId: "e1", payload: new Uint8Array([1]), ackHandle: new AckHandle(new Uint8Array([7])) }; }
  async acknowledge(operation: string, handle: AckHandle, options: CallOptions): Promise<void> { this.calls.push(operation); this.handle=handle; this.options=options; if(this.failAckOnce){this.failAckOnce=false;throw new NativeError("transport","ack_failed");} if(this.interruptAckOnce){this.interruptAckOnce=false;throw new NativeError("cancelled","ack_cancelled");} }
  async negativeAcknowledge(operation: string, handle: AckHandle, options: CallOptions): Promise<void> { this.calls.push(operation); this.handle=handle; this.options=options; if(this.failNakOnce){this.failNakOnce=false;throw new NativeError("transport","nak_failed");} }
}

type NativeWorld = { transport?: RecordingTransport; error?: unknown; handle?: AckHandle; delivery?: Delivery; publishResult?: {duplicate:boolean}; expectedSignal?: AbortSignal; expectedDeadline?: Date; generationFailed?: boolean; proof?: { rustHashes: [string,string]; typescriptHashes: [string,string]; rpcCount: number; rpcOnlyFiles:number; duplicateDiagnostic: string; staleRustDiagnostic: string; staleTypescriptDiagnostic: string; identifierDiagnostics:string[]; disambiguatedClients:[string,string]; multiDirectoryClients:[string,string] } };
const publication = (): Publication => ({ messageId: "stable-1", entityId: "entity-1", payload: new Uint8Array([0, 255]) });
Given("a generated native client backed by a recording transport", function(this: NativeWorld) { this.transport = new RecordingTransport(); });
Given("a generated native client backed by a denying transport", function(this: NativeWorld) { this.transport = new RecordingTransport(true); });
Given("a generated native client backed by a transport that fails one acknowledgement", function(this: NativeWorld) { this.transport=new RecordingTransport();this.transport.failAckOnce=true; });
Given("a generated native client backed by a transport that interrupts one acknowledgement", function(this: NativeWorld) { this.transport=new RecordingTransport();this.transport.interruptAckOnce=true; });
Given("a generated native client backed by a transport that fails one negative acknowledgement", function(this: NativeWorld) { this.transport=new RecordingTransport();this.transport.failNakOnce=true; });
const proof = () => JSON.parse(readFileSync(new URL("../generated/proof.json", import.meta.url), "utf8"));
Given("a proto descriptor with native operations and no RPC methods", function(this: NativeWorld) { this.proof = proof(); });
Given("an ambiguous native descriptor", function(this: NativeWorld) { this.proof = proof(); });
Given("native descriptors with invalid and colliding identifiers", function(this: NativeWorld) { this.proof=proof(); });
Given("an ordinary RPC descriptor without native annotations", function(this: NativeWorld) { this.proof=proof(); });
Given("native declarations selected from multiple source directories", function(this: NativeWorld) { this.proof=proof(); });
When("it publishes opaque bytes with stable message identity", async function(this: NativeWorld) { this.publishResult=await new FixtureNativeV1NativeQueueClient(this.transport!).publish(publication()); });
When("it publishes through the buf plugin generated client", async function(this: NativeWorld) { this.publishResult=await new PluginNativeQueueClient(this.transport!).publish(publication()); });
When("it receives one durable delivery", async function(this: NativeWorld) { for await (const delivery of new FixtureNativeV1NativeQueueClient(this.transport!).deliver()) { this.delivery=delivery; this.handle = delivery.ackHandle; break; } });
When("it acknowledges the delivery", async function(this: NativeWorld) { await new FixtureNativeV1NativeQueueClient(this.transport!).acknowledge(this.handle!); });
When("it negatively acknowledges the delivery", async function(this: NativeWorld) { await new FixtureNativeV1NativeQueueClient(this.transport!).negativeAcknowledge(this.handle!); });
When("first acknowledgement attempt fails", async function(this: NativeWorld) { await assert.rejects(()=>new FixtureNativeV1NativeQueueClient(this.transport!).acknowledge(this.handle!),/ack_failed/); });
When("first acknowledgement attempt is interrupted", async function(this: NativeWorld) { await assert.rejects(()=>new FixtureNativeV1NativeQueueClient(this.transport!).acknowledge(this.handle!),(error:unknown)=>error instanceof NativeError&&error.code==="cancelled"); });
When("first negative acknowledgement attempt fails", async function(this: NativeWorld) { await assert.rejects(()=>new FixtureNativeV1NativeQueueClient(this.transport!).negativeAcknowledge(this.handle!),/nak_failed/); });
When("it attempts publication", async function(this: NativeWorld) { try { await new FixtureNativeV1NativeQueueClient(this.transport!).publish(publication()); } catch (error) { this.error = error; } });
When("it publishes with cancellation and deadline options", async function(this: NativeWorld) { this.expectedSignal=new AbortController().signal; this.expectedDeadline=new Date(Date.now()+1000); await new FixtureNativeV1NativeQueueClient(this.transport!).publish(publication(), { signal:this.expectedSignal, deadline:this.expectedDeadline }); });
When("it publishes with an already cancelled option", async function(this: NativeWorld) { const controller = new AbortController(); controller.abort(); try { await new FixtureNativeV1NativeQueueClient(this.transport!).publish(publication(), { signal: controller.signal }); } catch (error) { this.error = error; } });
When("it publishes with an expired deadline", async function(this: NativeWorld) { try { await new FixtureNativeV1NativeQueueClient(this.transport!).publish(publication(), { deadline: new Date(0) }); } catch (error) { this.error = error; } });
When("native clients are generated twice", function(this: NativeWorld) { assert.equal(this.proof?.rustHashes[0], this.proof?.rustHashes[1]); assert.equal(this.proof?.typescriptHashes[0], this.proof?.typescriptHashes[1]); });
When("native client generation is attempted", function(this: NativeWorld) { this.generationFailed = this.proof?.duplicateDiagnostic === 'duplicate native operation "publish" in Ambiguous'; });
When("checked generated output differs from descriptor generation", function(this: NativeWorld) { assert.equal(this.proof?.staleRustDiagnostic,"stale generated output");assert.equal(this.proof?.staleTypescriptDiagnostic,"stale generated output"); });
When("it starts publication with an already cancelled option", async function(this: NativeWorld) { const controller=new AbortController();controller.abort();const result=new FixtureNativeV1NativeQueueClient(this.transport!).publish(publication(),{signal:controller.signal});assert.ok(result instanceof Promise);try{await result}catch(error){this.error=error;} });
When("native client generation validates identifiers", function(this: NativeWorld) { assert.deepEqual(this.proof?.identifierDiagnostics,["leading digit","reserved new","reserved constructor","reserved transport","case collision","rust 2024 keyword","generated symbol collision"]); });
When("native plugin generation is attempted", function(this: NativeWorld) {});
Then("transport receives one typed publication", function(this: NativeWorld) { assert.deepEqual(this.transport!.calls,["publish"]); assert.equal(this.transport!.publication!.messageId,"stable-1"); assert.equal(this.transport!.publication!.entityId,"entity-1"); assert.deepEqual(this.transport!.publication!.payload,new Uint8Array([0,255])); assert.deepEqual(this.publishResult,{duplicate:false}); });
Then("delivery can be acknowledged", async function(this: NativeWorld) { assert.equal(this.delivery!.messageId,"m1");assert.equal(this.delivery!.entityId,"e1");assert.deepEqual(this.delivery!.payload,new Uint8Array([1]));await new FixtureNativeV1NativeQueueClient(this.transport!).acknowledge(this.handle!);assert.deepEqual(this.transport!.calls,["deliver","acknowledge"]);assert.deepEqual(this.transport!.handle!.value,new Uint8Array([7]));assert.deepEqual(this.transport!.options,{}); });
Then("delivery can be negatively acknowledged", async function(this: NativeWorld) { assert.equal(this.delivery!.messageId,"m1");assert.equal(this.delivery!.entityId,"e1");assert.deepEqual(this.delivery!.payload,new Uint8Array([1]));await new FixtureNativeV1NativeQueueClient(this.transport!).negativeAcknowledge(this.handle!);assert.deepEqual(this.transport!.calls,["deliver","negative_acknowledge"]);assert.deepEqual(this.transport!.handle!.value,new Uint8Array([7]));assert.deepEqual(this.transport!.options,{}); });
Then("reusing the acknowledgement handle is refused", async function(this: NativeWorld) { await assert.rejects(() => new FixtureNativeV1NativeQueueClient(this.transport!).acknowledge(this.handle!), (error: unknown) => error instanceof NativeError && error.message.includes("already used")); });
Then("reusing the negatively acknowledged handle is refused without dispatch", async function(this: NativeWorld) { const client=new FixtureNativeV1NativeQueueClient(this.transport!);const before=this.transport!.calls.length;await assert.rejects(()=>client.acknowledge(this.handle!),(error:unknown)=>error instanceof NativeError&&error.message.includes("already used"));await assert.rejects(()=>client.negativeAcknowledge(this.handle!),(error:unknown)=>error instanceof NativeError&&error.message.includes("already used"));assert.equal(this.transport!.calls.length,before); });
Then("generated client reports permission denied", function(this: NativeWorld) { assert.ok(this.error instanceof NativeError); assert.equal(this.error.code, "permission_denied"); });
Then("transport receives cancellation and deadline options", function(this: NativeWorld) { assert.strictEqual(this.transport!.options?.signal,this.expectedSignal); assert.strictEqual(this.transport!.options?.deadline,this.expectedDeadline); });
Then("generated client reports cancellation", function(this: NativeWorld) { assert.ok(this.error instanceof NativeError); assert.equal(this.error.code, "cancelled"); });
Then("generated client reports deadline exceeded", function(this: NativeWorld) { assert.ok(this.error instanceof NativeError); assert.equal(this.error.code, "deadline_exceeded"); });
Then("native transport receives no call", function(this: NativeWorld) { assert.deepEqual(this.transport!.calls, []); });
Then("generated outputs are byte identical", function(this: NativeWorld) { assert.equal(this.proof?.rustHashes[0], this.proof?.rustHashes[1]); assert.equal(this.proof?.typescriptHashes[0], this.proof?.typescriptHashes[1]); });
Then("descriptor RPC inventory remains empty", function(this: NativeWorld) { assert.equal(this.proof?.rpcCount, 0); });
Then("generation fails with a descriptor error", function(this: NativeWorld) { assert.equal(this.generationFailed, true); });
Then("stale output is reported for both targets", function(this: NativeWorld) { assert.equal(this.proof?.staleRustDiagnostic,"stale generated output");assert.equal(this.proof?.staleTypescriptDiagnostic,"stale generated output"); });
Then("cancellation is returned by the async result", function(this: NativeWorld) { assert.ok(this.error instanceof NativeError);assert.equal(this.error.code,"cancelled"); });
Then("acknowledgement retry succeeds with same handle", async function(this: NativeWorld) { await new FixtureNativeV1NativeQueueClient(this.transport!).acknowledge(this.handle!);assert.deepEqual(this.transport!.handle!.value,new Uint8Array([7])); });
Then("negative acknowledgement retry succeeds with same handle", async function(this: NativeWorld) { await new FixtureNativeV1NativeQueueClient(this.transport!).negativeAcknowledge(this.handle!);assert.deepEqual(this.transport!.handle!.value,new Uint8Array([7])); });
Then("unsafe identifiers are rejected and package names disambiguate clients", function(this: NativeWorld) { assert.deepEqual(this.proof!.identifierDiagnostics,["leading digit","reserved new","reserved constructor","reserved transport","case collision","rust 2024 keyword","generated symbol collision"]);assert.deepEqual(this.proof!.disambiguatedClients,["FixtureAlphaV1SameNameClient","FixtureBetaV1SameNameClient"]); });
Then("no native client files are emitted for either target", function(this: NativeWorld) { assert.equal(this.proof?.rpcOnlyFiles,0); });
Then("one output per target contains every selected native client", function(this: NativeWorld) { assert.deepEqual(this.proof?.multiDirectoryClients,["FixtureNativeV1NativeQueueClient","FixtureSecondV1SecondQueueClient"]); });
