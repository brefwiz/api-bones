// SPDX-License-Identifier: MIT
/**
 * The TypeScript half of `tests/features/connect_error_info.feature`.
 *
 * The Rust half answers the same file; neither keeps its own copy of the rows.
 */

import assert from "node:assert/strict";

import { Given, Then, When, World } from "@cucumber/cucumber";

import {
  BonesError,
  ConnectError,
  ERROR_INFO_TYPE,
  encodeErrorInfo,
} from "@brefwiz/api-bones-connect";

import { connectCodeOf } from "./support.ts";

const BASE_MESSAGE = "refused";

interface ErrorInfoWorld extends World {
  failure?: ConnectError;
  read?: BonesError;
}

function failureOf(world: ErrorInfoWorld): ConnectError {
  if (!world.failure) throw new Error("no refusal given");
  return world.failure;
}

function readOf(world: ErrorInfoWorld): BonesError {
  if (!world.read) throw new Error("the refusal was not read");
  return world.read;
}

function expected(value: string): string | undefined {
  return value === "none" ? undefined : value;
}

Given(
  "a refusal {string} whose ErrorInfo detail has code {string}, emitter {string} and version {string}",
  function (
    this: ErrorInfoWorld,
    connectCode: string,
    code: string,
    emitter: string,
    version: string,
  ) {
    const failure = new ConnectError(BASE_MESSAGE, connectCodeOf(connectCode));
    failure.details = [
      {
        type: ERROR_INFO_TYPE,
        value: encodeErrorInfo({ code, emitter, emitterVersion: version }),
      },
    ];
    this.failure = failure;
  },
);

Given(
  "a refusal {string} whose ErrorInfo detail has the wire value {string}",
  function (this: ErrorInfoWorld, connectCode: string, wire: string) {
    const failure = new ConnectError(BASE_MESSAGE, connectCodeOf(connectCode));
    failure.details = [{ type: ERROR_INFO_TYPE, value: Uint8Array.from(Buffer.from(wire, "base64")) }];
    this.failure = failure;
  },
);

Given(
  "a refusal {string} with no ErrorInfo detail",
  function (this: ErrorInfoWorld, connectCode: string) {
    this.failure = new ConnectError(BASE_MESSAGE, connectCodeOf(connectCode));
  },
);

When("the refusal is read as an SDK error", function (this: ErrorInfoWorld) {
  this.read = new BonesError(failureOf(this));
});

Then("its error code is {string}", function (this: ErrorInfoWorld, value: string) {
  assert.equal(readOf(this).code, expected(value));
});

Then("its emitter is {string}", function (this: ErrorInfoWorld, value: string) {
  assert.equal(readOf(this).emitter, expected(value));
});

Then("its text ends with {string}", function (this: ErrorInfoWorld, value: string) {
  const text = readOf(this).message;
  const token = expected(value);
  if (token === undefined) {
    assert.ok(!text.includes("[bones-error"), `text: ${text}`);
  } else {
    assert.ok(text.endsWith(token), `text: ${text}`);
  }
});
