// SPDX-License-Identifier: MIT
import { cpSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

/** Load a separate copy of the published package with its own module state. */
export async function packageCopy() {
  const dist = dirname(fileURLToPath(import.meta.resolve("@brefwiz/api-bones-connect")));
  const directory = mkdtempSync(join(dirname(dist), ".connect-copy-"));
  try {
    cpSync(dist, join(directory, "dist"), { recursive: true });
    writeFileSync(join(directory, "package.json"), JSON.stringify({ type: "module" }));
    const root = await import(pathToFileURL(join(directory, "dist/index.js")).href) as typeof import("@brefwiz/api-bones-connect");
    const web = await import(pathToFileURL(join(directory, "dist/web.js")).href) as typeof import("@brefwiz/api-bones-connect/web");
    return { root, web };
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}
