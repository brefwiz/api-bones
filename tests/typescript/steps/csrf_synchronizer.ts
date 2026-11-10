// SPDX-License-Identifier: MIT
/**
 * `tests/features/connect_csrf_synchronizer.feature`, answered through the
 * browser entry of `@brefwiz/api-bones-connect`.
 */

import assert from "node:assert/strict";

import { EmptySchema, StringValueSchema } from "@bufbuild/protobuf/wkt";
import { Code, ConnectError } from "@connectrpc/connect";
import { After, Before, Given, Then, When, World } from "@cucumber/cucumber";

import { configureConnectTransport, forgetCsrfSynchronizer } from "@brefwiz/api-bones-connect/web";

import { packageCopy } from "./package_copy.ts";
import { descriptors, policy, SYNC } from "./csrf_fixtures.ts";

const { unary, watch } = descriptors(StringValueSchema, EmptySchema);

const SEEN = "x-seen-csrf-token";

interface Seen {
  method: string;
  token: string | null;
}

interface Refusal {
  token: string;
  status: number;
  code: string;
}

interface CsrfWorld extends World {
  tag: string;
  base: string;
  signInToken: string;
  seen: Seen[];
  refusals: Refusal[];
  outcome: "succeeded" | "refused" | undefined;
  last: Headers | undefined;
  error: ConnectError | undefined;
}

/**
 * A BFF that records the token each call sent and echoes it back in `x-seen-csrf-token`.
 * A call listed in `world.refusals` is refused with the listed status and token; sign-in hands out the token it names in
 * the request message; every other call succeeds.
 */
function bffFor(world: CsrfWorld): typeof fetch {
  return async (input, init) => {
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    const method = new URL(url, "https://unused.invalid").pathname.split("/").pop() ?? "";
    const token = new Headers(init?.headers).get(SYNC);
    world.seen.push({ method, token });
    const echo: Record<string, string> = token ? { [SEEN]: token } : {};

    if (method === "SignIn") {
      return new Response(new Uint8Array(), {
        status: 200,
        headers: {
          ...echo,
          "content-type": "application/proto",
          [SYNC]: world.signInToken,
        },
      });
    }
    const refusal = world.refusals.shift();
    if (refusal) {
      return new Response(JSON.stringify({ code: refusal.code, message: "csrf" }), {
        status: refusal.status,
        headers: {
          ...echo,
          "content-type": "application/json",
          [SYNC]: refusal.token,
        },
      });
    }
    return new Response(new Uint8Array(), {
      status: 200,
      headers: { ...echo, "content-type": "application/proto" },
    });
  };
}

/** The page-wide synchronizer memory a reload empties (storage in the tab outlives it). */
const PAGE_MEMORY = Symbol.for("@brefwiz/api-bones-connect/csrf-tokens/v1");

function installTabStorage(refuses: boolean): void {
  const held = new Map<string, string>();
  const refuse = (): never => {
    throw new Error("storage refused");
  };
  Object.defineProperty(globalThis, "sessionStorage", {
    configurable: true,
    value: refuses
      ? { getItem: refuse, setItem: refuse, removeItem: refuse }
      : {
          getItem: (k: string) => held.get(k) ?? null,
          setItem: (k: string, v: string) => void held.set(k, v),
          removeItem: (k: string) => void held.delete(k),
        },
  });
}

let scenarios = 0;

After(function () {
  Reflect.deleteProperty(globalThis, "sessionStorage");
});

Before(function (this: CsrfWorld) {
  installTabStorage(false);
  this.tag = `s${++scenarios}`;
  this.signInToken = "";
  this.base = "";
  this.seen = [];
  this.refusals = [];
  this.outcome = undefined;
  this.last = undefined;
  this.error = undefined;
});

/** Each scenario gets its own origin, so a token left by another scenario cannot satisfy this one. */
function scoped(world: CsrfWorld, base: string): string {
  const url = new URL(base);
  url.hostname = `${world.tag}.${url.hostname}`;
  return url.href.replace(/\/$/, "");
}

function optionsFor(world: CsrfWorld, base: string) {
  return {
    baseUrl: scoped(world, base),
    profile: "webapp" as const,
    policy,
    fetch: bffFor(world),
  };
}

Given("the tab refuses storage", function () {
  installTabStorage(true);
});

When("the page is reloaded", function () {
  const memory = (globalThis as Record<symbol, unknown>)[PAGE_MEMORY];
  assert.ok(memory instanceof Map, "the page keeps no synchronizer memory");
  memory.clear();
});

When("the synchronizer for {string} is forgotten", function (this: CsrfWorld, base: string) {
  forgetCsrfSynchronizer(scoped(this, base));
});

Given("a webapp transport for {string}", function (this: CsrfWorld, base: string) {
  this.base = base;
});

Given("it signed in and holds the token {string}", async function (this: CsrfWorld, token: string) {
  this.signInToken = token;
  await configureConnectTransport(optionsFor(this, this.base)).unary(
    unary("SignIn"),
    undefined,
    undefined,
    undefined,
    { value: "x" },
  );
  this.seen = [];
});

When(
  "it signs in and the BFF answers with the token {string}",
  async function (this: CsrfWorld, token: string) {
    this.signInToken = token;
    await configureConnectTransport(optionsFor(this, this.base)).unary(
      unary("SignIn"),
      undefined,
      undefined,
      undefined,
      { value: "x" },
    );
  },
);

When(
  "the BFF refuses the next call and hands out the token {string}",
  function (this: CsrfWorld, token: string) {
    this.refusals.push({ token, status: 403, code: "permission_denied" });
  },
);

When(
  "the BFF fails the next call as unauthenticated and hands out the token {string}",
  function (this: CsrfWorld, token: string) {
    this.refusals.push({ token, status: 401, code: "unauthenticated" });
  },
);

When("it calls the session method {string}", async function (this: CsrfWorld, name: string) {
  try {
    const result = await configureConnectTransport(optionsFor(this, this.base)).unary(
      unary(name),
      undefined,
      undefined,
      undefined,
      { value: "x" },
    );
    this.last = result.header;
    this.outcome = "succeeded";
  } catch (err) {
    this.error = ConnectError.from(err);
    this.last = this.error.metadata;
    this.outcome = "refused";
  }
});

When("it reads the public method {string}", async function (this: CsrfWorld, name: string) {
  const result = await configureConnectTransport(optionsFor(this, this.base)).unary(
    unary(name),
    undefined,
    undefined,
    undefined,
    { value: "x" },
  );
  this.last = result.header;
});

When("it opens the server stream {string}", async function (this: CsrfWorld, _name: string) {
  try {
    await configureConnectTransport(optionsFor(this, this.base)).stream(
      watch(),
      undefined,
      undefined,
      undefined,
      (async function* () {
        yield { value: "x" };
      })(),
    );
    this.outcome = "succeeded";
  } catch {
    this.outcome = "refused";
  }
});

When(
  "a second transport for {string} calls the session method {string}",
  async function (this: CsrfWorld, base: string, name: string) {
    const result = await configureConnectTransport(optionsFor(this, base)).unary(
      unary(name),
      undefined,
      undefined,
      undefined,
      { value: "x" },
    );
    this.last = result.header;
  },
);

Given(
  "another package copy on the page learned a generated synchronizer from {string}",
  async function (this: CsrfWorld, base: string) {
    const copy = await packageCopy();
    assert.notEqual(copy.web.configureConnectTransport, configureConnectTransport);
    this.signInToken = crypto.randomUUID();
    await copy.web
      .configureConnectTransport(optionsFor(this, base))
      .unary(unary("SignIn"), undefined, undefined, undefined, { value: "x" });
    this.seen = [];
  },
);

Then(
  "the {string} call carried the generated synchronizer",
  function (this: CsrfWorld, name: string) {
    assert.ok(this.last, "no answer was kept");
    assert.equal(this.last.get(SEEN), this.signInToken);
    assert.deepEqual(this.seen, [{ method: name, token: this.signInToken }]);
  },
);

Then(
  "the {string} call carried the token {string}",
  function (this: CsrfWorld, _name: string, token: string) {
    assert.ok(this.last, "no answer was kept");
    assert.equal(this.last.get(SEEN), token);
  },
);

Then("the {string} call carried no token", function (this: CsrfWorld, name: string) {
  assert.ok(this.last, "no answer was kept");
  assert.equal(this.last.get(SEEN), null);
  assert.deepEqual(this.seen.at(-1), { method: name, token: null });
});

Then("the call failed as {word}", function (this: CsrfWorld, code: string) {
  assert.ok(this.error, "the call did not fail");
  assert.equal(Code[this.error.code], code);
});

Then(
  "the call was sent twice, with {string} and then {string}",
  function (this: CsrfWorld, first: string, second: string) {
    assert.deepEqual(
      this.seen.map((s) => s.token),
      [first, second],
    );
  },
);

Then("the call was sent once", function (this: CsrfWorld) {
  assert.equal(this.seen.length, 1);
});

Then("the call succeeded", function (this: CsrfWorld) {
  assert.equal(this.outcome, "succeeded");
});

Then("the call was refused", function (this: CsrfWorld) {
  assert.equal(this.outcome, "refused");
});
