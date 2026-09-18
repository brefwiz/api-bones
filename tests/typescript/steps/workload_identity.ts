// SPDX-License-Identifier: MIT
/**
 * `tests/features/connect_workload_identity.feature`, answered through the
 * Node entry of `@brefwiz/api-bones-connect`, where workload identity ships.
 */

import assert from "node:assert/strict";

import { Given, Then, World } from "@cucumber/cucumber";

import {
  WorkloadIdentityError,
  clientTlsIdentityFor,
  trustDomainOf,
} from "@brefwiz/api-bones-connect/node";

type SvidWatcher = Parameters<typeof clientTlsIdentityFor>[0];

interface WorkloadWorld extends World {
  spiffeId: string;
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
