// SPDX-License-Identifier: MIT
/**
 * The TypeScript answer to `tests/sdk-call/features/sdk_wasm_load.feature`.
 *
 * The Makefile ts-test target sets `BREFWIZ_SDK_EVIDENCE` to a fresh directory
 * before cucumber starts, so the one wasm load of the run records its
 * provenance there. The steps read that record and compare it with the wasm
 * module of the installed package.
 */

import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { readFile, readdir } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

import { Given, Then, When } from "@cucumber/cucumber";
import { apiBonesSdk, clamp, init, sdkBuildInfo } from "@brefwiz/api-bones-test-service-sdk";

import type { SdkCallWorld } from "./sdk_call.ts";

const PACKAGE = "@brefwiz/api-bones-test-service-sdk";

/** The directory the lane collects evidence in. */
function evidenceDirectory(): string {
  const directory = process.env.BREFWIZ_SDK_EVIDENCE;
  assert.ok(directory, "the ts-test target did not set BREFWIZ_SDK_EVIDENCE");
  return directory;
}

/** The provenance lines the run recorded. */
async function recorded(): Promise<string[]> {
  const file = join(evidenceDirectory(), "sdk-provenance.jsonl");
  return existsSync(file) ? (await readFile(file, "utf8")).split("\n").filter((line) => line !== "") : [];
}

/** The wasm module of the installed package, found from where its entry resolves. */
async function installedWasm(): Promise<string> {
  let at = dirname(fileURLToPath(import.meta.resolve(PACKAGE)));
  while (!existsSync(join(at, "package.json"))) at = dirname(at);
  const dist = join(at, "dist", "generated-l2");
  return join(dist, (await readdir(dist)).find((name) => name.endsWith("_bg.wasm"))!);
}

Given("the run collects SDK evidence in the run's evidence directory", function () {
  evidenceDirectory();
});

When("the caller opens the SDK", async function (this: SdkCallWorld) {
  try {
    this.opened = await apiBonesSdk(this.session(), { baseUrl: this.provider.url, defaultPageSize: this.defaultPageSize });
  } catch (error) {
    this.refusal = error;
  }
});

Then("one SDK provenance line was recorded with the schema {string}", async function (schema: string) {
  const lines = await recorded();
  assert.equal(lines.length, 1, `provenance lines: ${lines.join(" | ")}`);
  assert.equal((JSON.parse(lines[0]!) as { schema: string }).schema, schema);
});

Then("its wasm digest is the SHA-256 of the installed wasm module", async function () {
  const [line] = await recorded();
  const digest = createHash("sha256").update(await readFile(await installedWasm())).digest("hex");
  assert.equal((JSON.parse(line!) as { wasmSha256: string }).wasmSha256, digest);
});

Then("its wasm digest is the one the SDK reports as its build info", async function () {
  const [line] = await recorded();
  const info = await sdkBuildInfo();
  assert.equal((JSON.parse(line!) as { wasmSha256: string }).wasmSha256, info.wasmSha256);
});

/**
 * What a fresh process of this lane does when it calls the SDK's `clamp`
 * before it awaits `init()`. The wasm module of this process is
 * already loaded by the scenarios that ran before, so the call has to be made
 * in a process that never loaded it. The child collects no evidence, so it
 * adds no provenance line to the run's record.
 */
const FRESH_CALLER = `
import { SdkNotInitializedError, clamp } from "${PACKAGE}";
let before;
try {
  clamp(5, 1, 10);
  before = "answered";
} catch (error) {
  before = error instanceof SdkNotInitializedError ? "SdkNotInitializedError" : String(error);
}
process.stdout.write(JSON.stringify({ before }));
`;

When("a fresh process calls the SDK's clamp function before it awaits the SDK's init", async function (this: SdkCallWorld) {
  const env = { ...process.env };
  delete env.BREFWIZ_SDK_EVIDENCE;
  const { stdout } = await promisify(execFile)(process.execPath, ["--input-type=module", "-e", FRESH_CALLER], {
    cwd: fileURLToPath(new URL("..", import.meta.url)),
    env,
  });
  this.fresh = JSON.parse(stdout) as { before: string };
});

Then("that call is refused as the SDK not initialised", function (this: SdkCallWorld) {
  assert.ok(this.fresh, "a When ran the fresh process");
  assert.equal(this.fresh.before, "SdkNotInitializedError");
});

When("the caller calls the SDK's clamp function after it awaits the SDK's init", async function (this: SdkCallWorld) {
  await init();
  const answer = clamp(5, 1, 10);
  this.staticAnswer = answer;
});

Then("the clamp answers {int}", function (this: SdkCallWorld, expected: number) {
  assert.equal(this.staticAnswer, expected);
});
