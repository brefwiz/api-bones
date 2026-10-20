// SPDX-License-Identifier: MIT
import { describe, expect, it } from "vitest";

import { CSRF_HEADER, csrfReplayToken } from "./csrf.js";

describe("csrfReplayToken", () => {
  it("names the header", () => {
    expect(CSRF_HEADER).toBe("X-CSRF-Token");
  });

  it.each<[number, string | null, string | null, string | null]>([
    [403, "fresh", "stale", "fresh"],
    [403, "same", "same", null],
    [403, null, "sent", null],
    [403, "", "sent", null],
    [401, "fresh", "stale", null],
    [200, "fresh", "stale", null],
    [403, "fresh", null, "fresh"],
    [403, null, null, null],
  ])("status %i refusal %s sent %s -> %s", (status, refusal, sent, want) => {
    expect(csrfReplayToken(status, refusal, sent)).toBe(want);
  });
});
