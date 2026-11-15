// SPDX-License-Identifier: MIT
// The package's published surface: the schema subpath, the root entry, and the
// files an `npm pack` would ship.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { test } from "node:test";

const require = createRequire(import.meta.url);

test("the schema subpath is the combined JSON Schema", () => {
  const schema = JSON.parse(readFileSync(require.resolve("@brefwiz/api-bones-types/schema.json"), "utf8"));
  assert.equal(schema.title, "ApiBones");
  for (const name of ["ApiError", "PaginatedResponse", "Principal", "OrgId"]) {
    assert.ok(name in schema.definitions, `${name} is defined`);
  }
});

test("the root entry loads", async () => {
  const entry = await import("@brefwiz/api-bones-types");
  assert.deepEqual(Object.keys(entry), []);
});

test("the packed file list holds the build, the schema and the licence", () => {
  const [pack] = JSON.parse(execFileSync("npm", ["pack", "--dry-run", "--json", "--ignore-scripts"], { encoding: "utf8" }));
  const paths = pack.files.map((file) => file.path);
  for (const required of ["package.json", "LICENSE", "schema/api-bones.json", "dist/index.js", "dist/index.d.ts"]) {
    assert.ok(paths.includes(required), `${required} is packed`);
  }
  const stray = paths.filter(
    (path) => !["package.json", "LICENSE", "schema/api-bones.json"].includes(path) && !path.startsWith("dist/"),
  );
  assert.deepEqual(stray, []);
});
