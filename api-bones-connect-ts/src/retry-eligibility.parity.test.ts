// SPDX-License-Identifier: MIT
/**
 * The TypeScript half of the shared parity corpus.
 *
 * `test-fixtures/parity/connect-retry-eligibility.json` is answered by BOTH
 * languages. Neither keeps its own copy of the cases: the signature list is
 * transcribed into each language by hand, and a corpus each side wrote for
 * itself would let the two drift apart exactly as the implementations could.
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { Code, ConnectError } from "@connectrpc/connect";
import { describe, expect, it } from "vitest";

import {
  connectionFailureAsUnavailable,
  isConnectionWriteFailure,
  isReplayableTransportFailure,
  isUnpromptedRetryable,
} from "./retry-eligibility.js";

interface ParityCase {
  readonly id: string;
  readonly code: string;
  readonly message: string;
  readonly connection_write_failure: boolean;
  readonly unprompted_retryable: boolean;
  readonly replayable: boolean;
  readonly normalized_code: string;
}

const CORPUS_PATH = fileURLToPath(
  new URL("../../test-fixtures/parity/connect-retry-eligibility.json", import.meta.url),
);

const CODES: Readonly<Record<string, Code>> = {
  internal: Code.Internal,
  unavailable: Code.Unavailable,
  unauthenticated: Code.Unauthenticated,
  aborted: Code.Aborted,
  resource_exhausted: Code.ResourceExhausted,
  permission_denied: Code.PermissionDenied,
};

const NAMES: ReadonlyMap<Code, string> = new Map(
  Object.entries(CODES).map(([name, code]) => [code, name] as const),
);

function codeOf(name: string): Code {
  const code = CODES[name];
  if (code === undefined) throw new Error(`corpus names a code this test cannot build: ${name}`);
  return code;
}

describe("connect retry-eligibility parity corpus", () => {
  const cases = JSON.parse(readFileSync(CORPUS_PATH, "utf8")).cases as ParityCase[];

  it("is not empty", () => {
    // An empty corpus agrees with itself in every language.
    expect(cases.length).toBeGreaterThan(0);
  });

  it.each(cases.map((c) => [c.id, c] as const))("%s answers as declared", (_id, testCase) => {
    const err = new ConnectError(testCase.message, codeOf(testCase.code));

    expect(isConnectionWriteFailure(err)).toBe(testCase.connection_write_failure);
    expect(isUnpromptedRetryable(err.code)).toBe(testCase.unprompted_retryable);
    expect(isReplayableTransportFailure(err)).toBe(testCase.replayable);
    expect(NAMES.get(connectionFailureAsUnavailable(err).code)).toBe(testCase.normalized_code);
  });
});
