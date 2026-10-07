// SPDX-License-Identifier: MIT
/**
 * `tests/features/connect_workload_identity.feature`, answered through the
 * Node entry of `@brefwiz/api-bones-connect`, where workload identity ships.
 */

import assert from "node:assert/strict";

import { Given, Then, When, World } from "@cucumber/cucumber";

import {
  WorkloadIdentityError,
  clientTlsIdentityFor,
  trustDomainOf,
  workloadPeerFor,
} from "@brefwiz/api-bones-connect/node";

type SvidWatcher = Parameters<typeof clientTlsIdentityFor>[0];

interface WorkloadWorld extends World {
  spiffeId: string;
  peer?: ReturnType<typeof workloadPeerFor>;
  peerFailure?: unknown;
}

Given("a workload whose SVID names {string}", function (this: WorkloadWorld, spiffeId: string) {
  this.spiffeId = spiffeId;
});

Then("its trust domain is {string}", function (this: WorkloadWorld, expected: string) {
  assert.equal(trustDomainOf(this.spiffeId), expected === "none" ? null : expected);
});

Then(
  "deriving its TLS identity fails as {string}",
  function (this: WorkloadWorld, kind: string) {
    // Only `current()` is read before the trust domain is refused.
    const watcher = { current: () => ({ spiffeId: this.spiffeId }) } as unknown as SvidWatcher;
    let failure: unknown;
    try {
      clientTlsIdentityFor(watcher);
    } catch (error) {
      failure = error;
    }
    assert.ok(failure instanceof WorkloadIdentityError, `expected a WorkloadIdentityError, got ${String(failure)}`);
    assert.equal(failure.kind, kind);
  },
);

When("it reaches the workload {string}", function (this: WorkloadWorld, service: string) {
  const watcher = { current: () => ({ spiffeId: this.spiffeId }) } as unknown as SvidWatcher;
  const peer = workloadPeerFor(watcher, service);
  this.peer = peer;
});

When("it tries to reach the workload {string}", function (this: WorkloadWorld, service: string) {
  const watcher = { current: () => ({ spiffeId: this.spiffeId }) } as unknown as SvidWatcher;
  try {
    this.peer = workloadPeerFor(watcher, service);
  } catch (error) {
    this.peerFailure = error;
  }
});

Then("the peer address is {string}", function (this: WorkloadWorld, url: string) {
  assert.equal(this.peer?.url, url);
});

Then("the peer identity is {string}", function (this: WorkloadWorld, spiffeId: string) {
  assert.equal(this.peer?.spiffeId, spiffeId);
});

Then("reaching the peer fails as {string}", function (this: WorkloadWorld, kind: string) {
  const failure = this.peerFailure;
  assert.ok(failure instanceof WorkloadIdentityError, `expected a WorkloadIdentityError, got ${String(failure)}`);
  assert.equal(failure.kind, kind);
});
