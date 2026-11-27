// SPDX-License-Identifier: MIT
// Bundles the generated test-service wrapper with the runtime resolved from this
// package's node_modules, the registry release at the generator's floor
// version. A name the wrapper imports that the release does not export fails
// the bundle with MISSING_EXPORT.

import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { rolldown } from "rolldown";

const RUNTIME = "@brefwiz/api-bones-sdk-runtime";
const entry = resolve(
  fileURLToPath(new URL(".", import.meta.url)),
  "../../api-bones-test-service-sdk-ts/src/generated/index.ts",
);
const runtimeEntry = fileURLToPath(import.meta.resolve(RUNTIME));

const bare = (id) => !id.startsWith(".") && !id.startsWith("/") && !/^[A-Za-z]:/.test(id) && !id.startsWith("\0");

try {
  const build = await rolldown({
    input: entry,
    platform: "browser",
    logLevel: "silent",
    resolve: { alias: { [RUNTIME]: runtimeEntry } },
    external: (id) => id !== RUNTIME && bare(id),
    onLog: (_level, log) => {
      if (log.code === "MISSING_EXPORT") {
        throw new Error(log.message);
      }
    },
  });
  await build.generate({ format: "esm" });
} catch (error) {
  console.error(`the wrapper does not bundle against ${RUNTIME} at the floor version:`);
  console.error(error instanceof Error ? error.message : error);
  process.exit(1);
}
