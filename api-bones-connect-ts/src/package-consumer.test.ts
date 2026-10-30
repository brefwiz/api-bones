// SPDX-License-Identifier: MIT
import { afterAll, beforeAll, describe, it } from "vitest";
import { createPackedConsumerProof } from "../../tests/typescript/steps/packed_consumer_support.ts";

describe("packed optional transport peers", () => {
  let proof: ReturnType<typeof createPackedConsumerProof>;
  beforeAll(() => {
    proof = createPackedConsumerProof();
  }, 120_000);
  afterAll(() => proof?.close());
  it(
    "keeps the neutral root usable and refuses missing adapters",
    () => proof.proveNeutral(),
    180_000,
  );
  it.each([["web"], ["node"], ["web", "node"]])(
    "loads selected real adapters: %j",
    (...entries) => proof.proveSelected(entries),
    180_000,
  );
  it(
    "resolves transports through the linked SDK boundary",
    () => proof.proveLinked(),
    360_000,
  );
  it.each(["web", "node"])(
    "rejects an incompatible explicit adapter: %s",
    (entry) => proof.proveIncompatible(entry),
    180_000,
  );
});
