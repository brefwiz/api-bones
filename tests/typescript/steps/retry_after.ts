// SPDX-License-Identifier: MIT
/**
 * The TypeScript half of `tests/features/connect_retry_after.feature`.
 *
 * The Rust half answers the same file; neither keeps its own copy of the rows.
 *
 * The refused call goes over loopback HTTP: a real Connect node handler throws
 * the refusal built with the shipped `resourceExhaustedRetryAfter`, and a real
 * Connect web transport reads the refusal back from the server's response.
 */

import assert from "node:assert/strict";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";

import { After, Given, Then, When, World } from "@cucumber/cucumber";

import {
  Code,
  ConnectError,
  resourceExhaustedRetryAfter,
  retryAfter,
  withRetryAfter,
} from "@brefwiz/api-bones-connect";
import { connectNodeAdapter } from "@connectrpc/connect-node";
import { createConnectTransport } from "@connectrpc/connect-web";

import { CONNECT_CODE_NAMES, unaryMethodFixture } from "./support.ts";

const MESSAGE = "slow down";
const RETRY_INFO_TYPE = "google.rpc.RetryInfo";

function retryInfoValue(waitMs: number): Uint8Array {
  const detail = (withRetryAfter(new ConnectError(MESSAGE, Code.ResourceExhausted), waitMs).details as Array<{
    type: string;
    value: Uint8Array;
  }>).find((candidate) => candidate.type === RETRY_INFO_TYPE);
  assert.ok(detail, "withRetryAfter attaches no RetryInfo");
  return detail.value;
}

interface RetryAfterWorld extends World {
  waitMs?: number;
  failure?: ConnectError;
  server?: Server;
  waitRead?: boolean;
  readWaitMs?: number;
}

function failureOf(world: RetryAfterWorld): ConnectError {
  if (!world.failure) throw new Error("no refusal given or sent");
  return world.failure;
}

function unpadded(bytes: Uint8Array): string {
  return Buffer.from(bytes).toString("base64").replace(/=+$/, "");
}

After(async function (this: RetryAfterWorld) {
  const server = this.server;
  if (server?.listening) {
    await new Promise<void>((resolve) => {
      server.close(() => resolve());
      server.closeAllConnections();
    });
  }
});

Given(
  "a service that refuses a call and asks the caller to wait {int} ms",
  function (this: RetryAfterWorld, waitMs: number) {
    this.waitMs = waitMs;
  },
);

When("the refused call is sent", async function (this: RetryAfterWorld) {
  assert.notEqual(this.waitMs, undefined, "no wait given");
  const waitMs = this.waitMs as number;
  const method = unaryMethodFixture("pkg.v1.RetryAfterService", "Method");
  const handler = connectNodeAdapter({
    routes(router) {
      router.rpc(method, async () => {
        throw resourceExhaustedRetryAfter(MESSAGE, waitMs);
      });
    },
  });
  this.server = createServer(handler);
  const server = this.server;
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  let status: number | undefined;
  const transport = createConnectTransport({
    baseUrl: `http://127.0.0.1:${(server.address() as AddressInfo).port}`,
    fetch: async (input, init) => {
      const response = await fetch(input, init);
      status = response.status;
      return response;
    },
  });
  try {
    await transport.unary(
      unaryMethodFixture("pkg.v1.RetryAfterService", "Method"),
      undefined,
      undefined,
      undefined,
      { value: "x" },
    );
    assert.fail("the refuser refuses every call");
  } catch (err) {
    this.failure = ConnectError.from(err);
  }
  assert.equal(status, 429, "the server answers a refusal with HTTP 429");
});

Given(
  "a refusal with Retry-After {string} and RetryInfo {string}",
  function (this: RetryAfterWorld, header: string, info: string) {
    const refusal = new ConnectError(MESSAGE, Code.ResourceExhausted);
    if (header !== "none") refusal.metadata.set("retry-after", header);
    if (info !== "none") {
      refusal.details = [{ type: RETRY_INFO_TYPE, value: retryInfoValue(Number(info) * 1000) }];
    }
    this.failure = refusal;
  },
);

Given(
  "a refusal with the RetryInfo wire value {string}",
  function (this: RetryAfterWorld, wire: string) {
    const refusal = new ConnectError(MESSAGE, Code.ResourceExhausted);
    refusal.details = [{ type: RETRY_INFO_TYPE, value: Uint8Array.from(Buffer.from(wire, "base64")) }];
    this.failure = refusal;
  },
);

When("the client reads the wait", function (this: RetryAfterWorld) {
  const waitMs = retryAfter(failureOf(this));
  this.waitRead = true;
  this.readWaitMs = waitMs;
});

Then("the call is refused as {string}", function (this: RetryAfterWorld, code: string) {
  assert.equal(CONNECT_CODE_NAMES.get(failureOf(this).code), code);
});

Then("the refusal's message is {string}", function (this: RetryAfterWorld, message: string) {
  assert.equal(failureOf(this).rawMessage, message);
});

Then("the refusal's Retry-After is {string}", function (this: RetryAfterWorld, seconds: string) {
  assert.equal(failureOf(this).metadata.get("retry-after"), seconds);
});

Then("the refusal's Cache-Control is {string}", function (this: RetryAfterWorld, value: string) {
  assert.equal(failureOf(this).metadata.get("cache-control"), value);
});

Then("the refusal's RetryInfo is {int} seconds", function (this: RetryAfterWorld, seconds: number) {
  const detail = (failureOf(this).details as Array<{ type: string; value: Uint8Array }>).find(
    (candidate) => candidate.type === RETRY_INFO_TYPE,
  );
  assert.ok(detail, "the refusal carries no RetryInfo");
  const readBack = new ConnectError(MESSAGE, Code.ResourceExhausted);
  readBack.details = [detail];
  assert.equal(retryAfter(readBack), seconds * 1000);
});

Then("the wait the client reads is {string}", function (this: RetryAfterWorld, millis: string) {
  assert.ok(this.waitRead, "the wait was not read");
  assert.equal(this.readWaitMs, millis === "none" ? undefined : Number(millis));
});

Then(
  "a RetryInfo of {int} seconds is written as {string}",
  function (this: RetryAfterWorld, seconds: number, wire: string) {
    assert.equal(unpadded(retryInfoValue(seconds * 1000)), wire);
  },
);
