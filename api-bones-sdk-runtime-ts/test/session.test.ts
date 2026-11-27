// SPDX-License-Identifier: MIT
import { fromBinary } from "@bufbuild/protobuf";
import { afterEach, describe, expect, it, vi } from "vitest";
import { BonesError, JsSpanSink, RUNTIME_BOUNDARY, hostOf, pageOrigin, type Credential, type CredentialAsk, type SdkSession } from "../src/index.js";
import { CredentialAnswerSchema } from "../src/generated/bones/sdk/v1/boundary_pb.js";

const alice: Credential = { subject: "alice", org: "acme", headers: { authorization: "Bearer a" } };

/** What the host answered, as the core decodes it. */
const answered = (bytes: Uint8Array) => fromBinary(CredentialAnswerSchema, bytes).answer;

async function ask(session: SdkSession, refresh: boolean, signal: AbortSignal = new AbortController().signal): Promise<Uint8Array> {
  const read = hostOf(session, new JsSpanSink()).credential;
  if (read === undefined) throw new Error("the host has no credential");
  return read(refresh, signal);
}

describe("RUNTIME_BOUNDARY", () => {
  it("is the boundary version this runtime speaks", () => {
    expect(RUNTIME_BOUNDARY).toBe(1);
  });
});

describe("hostOf", () => {
  it("is anonymous without a credential when the session is", () => {
    const host = hostOf({ credential: "anonymous" }, new JsSpanSink());
    expect(host.anonymous).toBe(true);
    expect(host.credential).toBeUndefined();
  });

  it("answers the credential of the session, passing it the ask", async () => {
    const seen: CredentialAsk[] = [];
    const signal = new AbortController().signal;
    const bytes = await ask(
      {
        credential: async (request) => {
          seen.push(request);
          return alice;
        },
      },
      true,
      signal,
    );
    expect(seen).toEqual([{ refresh: true, signal }]);
    expect(seen[0]?.signal).toBe(signal);
    const answer = answered(bytes);
    expect(answer.case).toBe("credential");
    expect(answer.case === "credential" ? [answer.value.subject, answer.value.org, answer.value.headers] : undefined).toEqual([
      "alice",
      "acme",
      { authorization: "Bearer a" },
    ]);
  });

  it("answers the credential in full on every read, whatever it answered before", async () => {
    let current = alice;
    const read = hostOf({ credential: async () => current }, new JsSpanSink()).credential!;
    const signal = new AbortController().signal;
    const subjects = async (refresh: boolean) => {
      const answer = answered(await read(refresh, signal));
      expect(answer.case).toBe("credential");
      return answer.case === "credential" ? [answer.value.subject, answer.value.org, answer.value.headers] : undefined;
    };
    expect(await subjects(false)).toEqual(["alice", "acme", { authorization: "Bearer a" }]);
    expect(await subjects(false)).toEqual(["alice", "acme", { authorization: "Bearer a" }]);
    expect(await subjects(true)).toEqual(["alice", "acme", { authorization: "Bearer a" }]);
    current = { subject: "bob", org: "other", headers: { authorization: "Bearer b" } };
    expect(await subjects(false)).toEqual(["bob", "other", { authorization: "Bearer b" }]);
  });

  it("keeps no state: an aborted read leaves nothing for the next one", async () => {
    let current = alice;
    const controller = new AbortController();
    const host = hostOf(
      {
        credential: async ({ signal }) => {
          const answer = current;
          if (signal.aborted) throw new Error("aborted");
          return answer;
        },
      },
      new JsSpanSink(),
    );
    const read = host.credential!;
    controller.abort();
    await expect(read(false, controller.signal)).rejects.toThrow("aborted");
    current = { ...alice, headers: { authorization: "Bearer b" } };
    const next = answered(await read(false, new AbortController().signal));
    expect(next.case).toBe("credential");
    expect(next.case === "credential" ? next.value.headers : undefined).toEqual({ authorization: "Bearer b" });
  });

  it("serves several clients from one session, each answered the credential", async () => {
    const session: SdkSession = { credential: async () => alice };
    const first = hostOf(session, new JsSpanSink()).credential!;
    const second = hostOf(session, new JsSpanSink()).credential!;
    const signal = new AbortController().signal;
    expect(answered(await first(false, signal)).case).toBe("credential");
    expect(answered(await second(false, signal)).case).toBe("credential");
    expect(answered(await first(false, signal)).case).toBe("credential");
  });

  it("rejects when the credential source rejects, so the core fails the call closed", async () => {
    const failure = new Error("token endpoint down");
    await expect(ask({ credential: async () => Promise.reject(failure) }, false)).rejects.toBe(failure);
  });

  it("tells the session of an unauthenticated refusal, and ignores it when the session has no listener", () => {
    const told: string[] = [];
    hostOf({ credential: "anonymous", onUnauthenticated: () => told.push("told") }, new JsSpanSink()).unauthenticated();
    expect(told).toEqual(["told"]);
    expect(() => hostOf({ credential: "anonymous" }, new JsSpanSink()).unauthenticated()).not.toThrow();
  });

  it("hands the spans to the sink", () => {
    const calls: Array<[string, Uint8Array]> = [];
    const sink = {
      start: (bytes: Uint8Array) => {
        calls.push(["start", bytes]);
        return new Uint8Array([1]);
      },
      end: (bytes: Uint8Array) => void calls.push(["end", bytes]),
    } as unknown as JsSpanSink;
    const host = hostOf({ credential: "anonymous" }, sink);
    const opened = new Uint8Array([9]);
    expect(host.start(opened)).toEqual(new Uint8Array([1]));
    host.end(new Uint8Array([8]));
    expect(calls).toEqual([
      ["start", opened],
      ["end", new Uint8Array([8])],
    ]);
  });
});

describe("pageOrigin", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("is the origin of the page", () => {
    vi.stubGlobal("location", { origin: "https://app.example.test" });
    expect(pageOrigin()).toBe("https://app.example.test");
  });

  it.each([[undefined], [""], ["null"]])("is refused as invalid_argument when the page has no origin (%s)", (origin) => {
    if (origin === undefined) vi.stubGlobal("location", undefined);
    else vi.stubGlobal("location", { origin });
    let refusal: unknown;
    try {
      pageOrigin();
    } catch (error) {
      refusal = error;
    }
    expect(refusal).toBeInstanceOf(BonesError);
    expect(refusal).toMatchObject({
      code: "VALIDATION_FAILED",
      emitter: "bones",
      platform: "VALIDATION_FAILED",
      connectCode: "invalid_argument",
      message: "no page origin; pass baseUrl",
    });
  });
});
