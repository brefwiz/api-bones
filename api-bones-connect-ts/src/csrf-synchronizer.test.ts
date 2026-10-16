// SPDX-License-Identifier: MIT
import { EmptySchema, StringValueSchema } from "@bufbuild/protobuf/wkt";
import { Code } from "@connectrpc/connect";
import { afterEach, describe, expect, it, vi } from "vitest";

import { descriptors, policy, SYNC } from "../../tests/typescript/steps/csrf_fixtures.ts";
import { configureConnectTransport } from "./web.js";

const { unary, watch } = descriptors(StringValueSchema, EmptySchema);

type Answer = { status: number; token?: string; code?: string };

/** A BFF stand-in: answers in order and records the synchronizer each call sent. */
function bff(answers: Answer[], sentTokens: Array<string | null> = []): typeof fetch {
  let i = 0;
  return async (_input, init) => {
    sentTokens.push(new Headers(init?.headers).get(SYNC));
    const answer = answers[Math.min(i++, answers.length - 1)];
    const headers: Record<string, string> = {
      "content-type": answer.status === 200 ? "application/proto" : "application/json",
    };
    if (answer.token) headers[SYNC] = answer.token;
    const body =
      answer.status === 200
        ? new Uint8Array()
        : JSON.stringify({ code: answer.code ?? "permission_denied", message: "csrf" });
    return new Response(body, { status: answer.status, headers });
  };
}

function transportFor(origin: string, fetch: typeof globalThis.fetch) {
  return configureConnectTransport({
    baseUrl: `${origin}/app`,
    profile: "webapp",
    policy,
    fetch,
  });
}

const call = (t: ReturnType<typeof transportFor>, name = "Book") =>
  t.unary(unary(name), undefined, undefined, undefined, { value: "x" });

describe("csrf synchronizer", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("attaches the token a session-establishing response carried to the next POST", async () => {
    const sent: Array<string | null> = [];
    const t = transportFor("https://a.example.com", bff([{ status: 200, token: "t1" }, { status: 200 }], sent));
    await call(t, "SignIn");
    await call(t);
    expect(sent).toEqual([null, "t1"]);
  });

  it("retries once with the fresh token when refused for a stale one", async () => {
    const sent: Array<string | null> = [];
    const t = transportFor(
      "https://b.example.com",
      bff([{ status: 200, token: "old" }, { status: 403, token: "new" }, { status: 200 }, { status: 200 }], sent),
    );
    await call(t, "SignIn");
    await call(t);
    expect(sent).toEqual([null, "old", "new"]);
    await call(t);
    expect(sent[3]).toBe("new");
  });

  it("does not retry when the refusal carries the token that was sent", async () => {
    const sent: Array<string | null> = [];
    const t = transportFor(
      "https://c.example.com",
      bff([{ status: 200, token: "same" }, { status: 403, token: "same" }], sent),
    );
    await call(t, "SignIn");
    await expect(call(t)).rejects.toMatchObject({ code: Code.PermissionDenied });
    expect(sent).toEqual([null, "same"]);
  });

  it("does not retry a refusal that carries no token", async () => {
    const sent: Array<string | null> = [];
    const t = transportFor("https://d.example.com", bff([{ status: 403 }], sent));
    await expect(call(t)).rejects.toMatchObject({ code: Code.PermissionDenied });
    expect(sent).toHaveLength(1);
  });

  it("retries at most once", async () => {
    const sent: Array<string | null> = [];
    const t = transportFor(
      "https://e.example.com",
      bff([{ status: 403, token: "n1" }, { status: 403, token: "n2" }], sent),
    );
    await expect(call(t)).rejects.toMatchObject({ code: Code.PermissionDenied });
    expect(sent).toEqual([null, "n1"]);
  });

  it("shares the token between transports on one origin only", async () => {
    const first: Array<string | null> = [];
    const second: Array<string | null> = [];
    const other: Array<string | null> = [];
    await call(transportFor("https://f.example.com", bff([{ status: 200, token: "shared" }], first)), "SignIn");
    await call(transportFor("https://f.example.com", bff([{ status: 200 }], second)));
    await call(transportFor("https://g.example.com", bff([{ status: 200 }], other)));
    expect(second).toEqual(["shared"]);
    expect(other).toEqual([null]);
  });

  it("never retries a stream", async () => {
    const sent: Array<string | null> = [];
    const t = transportFor("https://h.example.com", bff([{ status: 403, token: "fresh" }], sent));
    await expect(
      t.stream(watch(), undefined, undefined, undefined, (async function* () {
        yield { value: "x" };
      })()),
    ).rejects.toMatchObject({ code: Code.PermissionDenied });
    expect(sent).toHaveLength(1);
  });

  it("keeps the token off the public lane", async () => {
    const sent: Array<string | null> = [];
    const t = configureConnectTransport({
      baseUrl: "https://i.example.com/app",
      profile: "webapp",
      policy,
      fetch: bff([{ status: 200, token: "t1" }, { status: 200 }], sent),
    });
    await call(t, "SignIn");
    await t.unary(
      unary("GetWeek"),
      undefined,
      undefined,
      undefined,
      { value: "x" },
    );
    expect(sent).toEqual([null, null]);
  });

  it("does not retry a non-PermissionDenied failure that carries a new token", async () => {
    const sent: Array<string | null> = [];
    const t = transportFor(
      "https://j.example.com",
      bff([{ status: 200, token: "old" }, { status: 401, code: "unauthenticated", token: "new" }], sent),
    );
    await call(t, "SignIn");
    await expect(call(t)).rejects.toMatchObject({ code: Code.Unauthenticated });
    expect(sent).toEqual([null, "old"]);
  });

  it("does not retry when a product interceptor set the token the refusal echoes", async () => {
    const sent: Array<string | null> = [];
    const t = configureConnectTransport({
      baseUrl: "https://k.example.com/app",
      profile: "webapp",
      policy,
      fetch: bff([{ status: 403, token: "mine" }], sent),
      interceptors: [
        (next) => (req) => {
          req.header.set("X-CSRF-Token", "mine");
          return next(req);
        },
      ],
    });
    await expect(call(t)).rejects.toMatchObject({ code: Code.PermissionDenied });
    expect(sent).toEqual(["mine"]);
  });

  const page = (origin: string) => {
    vi.stubGlobal("location", { href: `${origin}/`, origin });
    vi.stubGlobal("document", {
      querySelector: () => ({ getAttribute: () => "from-meta" }),
    });
  };

  it("falls back to the page meta token when none is stored, and a stored token wins", async () => {
    page("https://l.example.com");
    const sent: Array<string | null> = [];
    const t = transportFor(
      "https://l.example.com",
      bff([{ status: 200, token: "stored" }, { status: 200 }, { status: 200 }], sent),
    );
    await call(t, "SignIn");
    await call(t);
    expect(sent).toEqual(["from-meta", "stored"]);
  });

  it("never sends the page meta token to another origin", async () => {
    page("https://page.example.com");
    const sent: Array<string | null> = [];
    await call(transportFor("https://elsewhere.example.com", bff([{ status: 200 }], sent)));
    expect(sent).toEqual([null]);
  });
});
