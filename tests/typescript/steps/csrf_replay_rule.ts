// SPDX-License-Identifier: MIT
/**
 * The TypeScript half of `tests/features/connect_csrf_replay_rule.feature`.
 *
 * The Rust half answers the same file; neither keeps its own copy of the rows.
 */

import assert from "node:assert/strict";

import { Given, Then, World } from "@cucumber/cucumber";

import { csrfReplayToken } from "@brefwiz/api-bones-connect";

interface ReplayWorld extends World {
  status?: number;
  refusal?: string | null;
  sent?: string | null;
}

const token = (name: string): string | null => (name === "none" ? null : name);

Given(
  "a refusal with status {int} carrying the token {string} for a call that sent {string}",
  function (this: ReplayWorld, status: number, refusal: string, sent: string) {
    this.status = status;
    this.refusal = token(refusal);
    this.sent = token(sent);
  },
);

Then("the call is replayed with {string}", function (this: ReplayWorld, expected: string) {
  const replay = csrfReplayToken(this.status ?? 0, this.refusal ?? null, this.sent ?? null);
  assert.equal(replay ?? "none", expected);
});
