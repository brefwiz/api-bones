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
  readCode?: string;
  readEmitter?: string;
  readText?: string;
}

function failureOf(world: ErrorInfoWorld): ConnectError {
  if (!world.failure) throw new Error("no refusal given");
  return world.failure;
}

function expected(value: string): string | undefined {
  return value === "none" ? undefined : value;
}

Given(
  "a refusal {string} whose ErrorInfo detail has code {string} and emitter {string}",
  function (this: ErrorInfoWorld, connectCode: string, code: string, emitter: string) {
    const failure = new ConnectError(BASE_MESSAGE, connectCodeOf(connectCode));
    failure.details = [
      {
        type: ERROR_INFO_TYPE,
        value: encodeErrorInfo({ code, emitter }),
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
  const read = BonesError.from(failureOf(this));
  this.readCode = read.code;
  this.readEmitter = read.emitter;
  this.readText = read.message;
});

Then("its error code is {string}", function (this: ErrorInfoWorld, value: string) {
  assert.equal(this.readCode, expected(value));
});

Then("its emitter is {string}", function (this: ErrorInfoWorld, value: string) {
  assert.equal(this.readEmitter, expected(value));
});

Then("its text ends with {string}", function (this: ErrorInfoWorld, value: string) {
  const token = expected(value);
  if (token === undefined) {
    assert.equal(this.readText?.includes("[bones-error"), false);
  } else {
    assert.equal(this.readText?.endsWith(token), true);
  }
});
