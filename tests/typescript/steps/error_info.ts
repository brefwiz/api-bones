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
  ErrorArgError,
  encodeErrorInfo,
  encodeValidationFailure,
  VALIDATION_FAILURE_TYPE,
  violationsOf,
  type ErrorArg,
  type ValidationFailureDetail,
} from "@brefwiz/api-bones-connect";

import { connectCodeOf } from "./support.ts";

const BASE_MESSAGE = "refused";

interface ErrorInfoWorld extends World {
  failure?: ConnectError;
  readCode?: string;
  readEmitter?: string;
  readText?: string;
  readArgs?: ReadonlyMap<string, ErrorArg>;
  encoded?: Uint8Array;
  refused?: unknown;
  violations?: ValidationFailureDetail[];
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
  this.readArgs = read.args;
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

/** `name=type:value` items separated by `;`. `repeat:c*N` is a string of `N` copies of `c`. */
function parseArgs(spec: string): Array<[string, ErrorArg]> {
  return spec
    .split(";")
    .map((item) => item.trim())
    .filter((item) => item !== "")
    .map((item): [string, ErrorArg] => {
      const eq = item.indexOf("=");
      const typed = item.slice(eq + 1);
      const colon = typed.indexOf(":");
      const kind = typed.slice(0, colon);
      const value = typed.slice(colon + 1);
      const name = item.slice(0, eq);
      switch (kind) {
        case "string":
          return [name, { kind: "string", value }];
        case "repeat": {
          const star = value.lastIndexOf("*");
          return [name, { kind: "string", value: value.slice(0, star).repeat(Number(value.slice(star + 1))) }];
        }
        case "int64":
          return [name, { kind: "int64", value: BigInt(value) }];
        case "double":
          return [name, { kind: "double", value: Number(value) }];
        case "timestamp": {
          const [seconds, nanos] = value.split("/");
          return [name, { kind: "timestamp", seconds: BigInt(seconds), nanos: Number(nanos) }];
        }
        default:
          throw new Error(`unknown argument type ${kind}`);
      }
    });
}

function describe(arg: ErrorArg): string {
  switch (arg.kind) {
    case "string":
      return `string:${arg.value}`;
    case "int64":
      return `int64:${arg.value}`;
    case "double":
      return `double:${arg.value}`;
    case "timestamp":
      return `timestamp:${arg.seconds}/${arg.nanos}`;
  }
}

Given(
  "a refusal {string} whose ErrorInfo detail has code {string} and emitter {string} and arguments {string}",
  function (this: ErrorInfoWorld, connectCode: string, code: string, emitter: string, spec: string) {
    const failure = new ConnectError(BASE_MESSAGE, connectCodeOf(connectCode));
    failure.details = [
      { type: ERROR_INFO_TYPE, value: encodeErrorInfo({ code, emitter, args: new Map(parseArgs(spec)) }) },
    ];
    this.failure = failure;
  },
);

When(
  "an ErrorInfo with code {string} is built with arguments {string}",
  function (this: ErrorInfoWorld, code: string, spec: string) {
    try {
      this.encoded = encodeErrorInfo({ code, emitter: "", args: new Map(parseArgs(spec)) });
    } catch (error) {
      this.refused = error;
    }
  },
);

When(
  "an ErrorInfo with code {string} is built with {int} numbered arguments",
  function (this: ErrorInfoWorld, code: string, count: number) {
    const args = new Map(
      Array.from({ length: count }, (_, i): [string, ErrorArg] => [`k${i}`, { kind: "int64", value: 1n }]),
    );
    try {
      this.encoded = encodeErrorInfo({ code, emitter: "", args });
    } catch (error) {
      this.refused = error;
    }
  },
);

Then("its argument {string} is {string}", function (this: ErrorInfoWorld, name: string, value: string) {
  const arg = this.readArgs?.get(name);
  assert.equal(arg === undefined ? undefined : describe(arg), expected(value));
});

Then("it carries {int} arguments", function (this: ErrorInfoWorld, count: number) {
  assert.equal(this.readArgs?.size, count);
});

Then("its text is {string}", function (this: ErrorInfoWorld, value: string) {
  // A Connect error's own `message` leads with `[code] `; the text is what follows.
  assert.equal(this.readText?.replace(/^\[[a-z_]+\] /, ""), value);
});

Then("building it ends as {string}", function (this: ErrorInfoWorld, outcome: string) {
  if (this.refused instanceof ErrorArgError) {
    assert.equal(this.refused.reason, outcome);
  } else {
    assert.equal(this.refused, undefined);
    assert.equal(this.encoded === undefined ? "refused" : "accepted", outcome);
  }
});

Given(
  "the refusal has a violation at {string} for rule {string} and arguments {string}",
  function (this: ErrorInfoWorld, field: string, rule: string, spec: string) {
    const failure = failureOf(this);
    failure.details = [
      ...failure.details,
      {
        type: VALIDATION_FAILURE_TYPE,
        value: encodeValidationFailure({ field, message: "", rule, args: new Map(parseArgs(spec)) }),
      },
    ];
  },
);

Given(
  "the refusal has a ValidationFailure detail with the wire value {string}",
  function (this: ErrorInfoWorld, wire: string) {
    const failure = failureOf(this);
    failure.details = [
      ...failure.details,
      { type: VALIDATION_FAILURE_TYPE, value: Uint8Array.from(Buffer.from(wire, "base64")) },
    ];
  },
);

// The arguments read are those of the first violation.
When("the violations of the refusal are read", function (this: ErrorInfoWorld) {
  const read = violationsOf(failureOf(this));
  this.violations = read;
  this.readArgs = read[0]?.args ?? new Map();
});

Then("it carries {int} violations", function (this: ErrorInfoWorld, count: number) {
  assert.equal(this.violations?.length, count);
});

Then(
  "violation {int} is at {string} for rule {string}",
  function (this: ErrorInfoWorld, index: number, field: string, rule: string) {
    const violation = this.violations?.[index - 1];
    assert.deepEqual([violation?.field, violation?.rule], [field, rule]);
  },
);
