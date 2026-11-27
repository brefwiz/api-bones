// SPDX-License-Identifier: MIT
/**
 * The TypeScript answer to `tests/features/test_service_preflight.feature`:
 * the `api-bones-test-service` bin of `@brefwiz/api-bones-test-service`, run as
 * the separate loopback process a browser page under test calls, asked for
 * the CORS preflight a page at an origin sends before its first call.
 */

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { request } from "node:http";
import type { ChildProcess } from "node:child_process";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";

import { AfterAll, Given, Then, When } from "@cucumber/cucumber";

const SERVER = fileURLToPath(import.meta.resolve("@brefwiz/api-bones-test-service/bin"));

let child: ChildProcess | undefined;
let url = "";
let control = "";
let answer: Response | undefined;

/** Starts the test service process once and returns the base URL of its services. */
async function running(): Promise<string> {
  if (child !== undefined) return url;
  const started = spawn("node", [SERVER], { stdio: ["pipe", "pipe", "inherit"] });
  child = started;
  url = await new Promise<string>((resolve, reject) => {
    started.once("error", reject);
    started.once("exit", (code) => reject(new Error(`the test service exited with ${code} before it was ready`)));
    createInterface({ input: started.stdout! }).once("line", (line) => {
      const ready = JSON.parse(line) as { url: string; control: string };
      control = ready.control;
      resolve(ready.url);
    });
  });
  return url;
}

AfterAll(async function () {
  const started = child;
  if (started === undefined) return;
  child = undefined;
  const ended = new Promise<void>((resolve) => started.once("exit", () => resolve()));
  started.stdin!.end();
  started.kill();
  await ended;
});

Given("the test service is running", { timeout: 30_000 }, async function () {
  assert.notEqual(await running(), "", "the test service has no address");
});

When("a page at {string} asks to call it with the headers {string}", async function (origin: string, headers: string) {
  answer = await fetch(`${url}/contract.v1.Widgets/GetWidget`, {
    method: "OPTIONS",
    headers: { origin, "access-control-request-method": "POST", "access-control-request-headers": headers },
  });
});

function preflight(): Response {
  assert.ok(answer, "a When sent no preflight");
  return answer;
}

Then("the preflight is answered with status {int}", function (status: number) {
  assert.equal(preflight().status, status);
});

Then("the preflight allows the origin {string}", function (origin: string) {
  assert.equal(preflight().headers.get("access-control-allow-origin"), origin);
});

Then("the preflight allows the headers {string}", function (headers: string) {
  assert.equal(preflight().headers.get("access-control-allow-headers"), headers);
});

Then("the preflight allows no origin", function () {
  const response = preflight();
  for (const name of ["access-control-allow-origin", "access-control-allow-headers", "access-control-allow-methods"]) {
    assert.equal(response.headers.get(name), null, name);
  }
});

let controlStatus = 0;

/** One request to the control port; a `host` header overrides the Host the port checks. */
function controlCall(method: string, path: string, headers: Record<string, string>, body?: string): Promise<number> {
  return new Promise((resolve, reject) => {
    const call = request(`${control}${path}`, { method, headers }, (response) => {
      response.resume();
      response.once("end", () => resolve(response.statusCode ?? 0));
    });
    call.once("error", reject);
    call.end(body);
  });
}

When("the control port is asked for the seen calls under the Host {string}", async function (host: string) {
  controlStatus = await controlCall("GET", "/seen", { host });
});

When(
  "the control port is sent the widget {string} named {string} as {string}",
  async function (id: string, name: string, contentType: string) {
    controlStatus = await controlCall("POST", "/configure", { "content-type": contentType }, JSON.stringify({ widgets: { [id]: name } }));
  },
);

Then("the control port answers with status {int}", function (status: number) {
  assert.equal(controlStatus, status);
});

Then("the service answers a request for the widget {string} with status {int}", async function (id: string, status: number) {
  const response = await fetch(`${url}/contract.v1.Widgets/GetWidget`, {
    method: "POST",
    headers: { "content-type": "application/json", "connect-protocol-version": "1" },
    body: JSON.stringify({ id }),
  });
  assert.equal(response.status, status);
});
