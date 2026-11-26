// SPDX-License-Identifier: MIT
// Writes dist/contract.binpb: the descriptor set of the test contract protos
// (tests/l2/proto) with the shipped bones/v1 protos they import, so the
// installed package reads a compiled descriptor set and needs no .proto files
// and no buf at run time. Runs as the `prebuild` step.

import { execFileSync } from "node:child_process";
import { cpSync, mkdirSync, mkdtempSync, copyFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const here = (relative) => fileURLToPath(new URL(relative, import.meta.url));
const BUF = here("../api-bones-test-service-ts/node_modules/.bin/buf");
const dist = here("../api-bones-test-service-ts/dist");

const workdir = mkdtempSync(join(tmpdir(), "test-service-contract-"));
try {
  writeFileSync(join(workdir, "buf.yaml"), "version: v2\n");
  cpSync(here("../api-bones-protos/proto"), workdir, { recursive: true, filter: (src) => !src.endsWith("buf.yaml") });
  cpSync(here("../api-bones-protos/vendor"), workdir, { recursive: true });
  cpSync(here("../tests/l2/proto"), workdir, { recursive: true });
  mkdirSync(dist, { recursive: true });
  const image = join(workdir, "image.binpb");
  execFileSync(BUF, ["build", workdir, "-o", image], { stdio: "inherit" });
  copyFileSync(image, join(dist, "contract.binpb"));
} finally {
  rmSync(workdir, { recursive: true, force: true });
}
