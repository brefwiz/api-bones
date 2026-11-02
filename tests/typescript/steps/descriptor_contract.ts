// SPDX-License-Identifier: MIT
/**
 * Compiles a scenario's one contract file together with the shipped
 * `bones/v1` files, through `buf`, into a protobuf-es registry. Shared by the
 * descriptor-reflection step files so each adds only what its scenario needs.
 */

import { execFileSync } from "node:child_process";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { createFileRegistry, fromBinary, type FileRegistry } from "@bufbuild/protobuf";
import { FileDescriptorSetSchema } from "@bufbuild/protobuf/wkt";

const SHIPPED = fileURLToPath(new URL("../../../api-bones-protos/proto", import.meta.url));
const VENDORED = fileURLToPath(new URL("../../../api-bones-protos/vendor", import.meta.url));
const BUF = fileURLToPath(new URL("../node_modules/.bin/buf", import.meta.url));

/** The registry for `source` and the scratch directory the caller removes afterwards. */
export function compileContract(prefix: string, source: string): { workdir: string; registry: FileRegistry } {
  const workdir = mkdtempSync(join(tmpdir(), `${prefix}-`));
  writeFileSync(join(workdir, "buf.yaml"), "version: v2\n");
  cpSync(SHIPPED, workdir, { recursive: true, filter: (src) => !src.endsWith("buf.yaml") });
  cpSync(VENDORED, workdir, { recursive: true });
  mkdirSync(join(workdir, "contract", "v1"), { recursive: true });
  writeFileSync(join(workdir, "contract", "v1", "contract.proto"), source);
  const image = join(workdir, "image.binpb");
  execFileSync(BUF, ["build", workdir, "-o", image], { stdio: "pipe" });
  return { workdir, registry: createFileRegistry(fromBinary(FileDescriptorSetSchema, readFileSync(image))) };
}
