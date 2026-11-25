// SPDX-License-Identifier: MIT
import { readdirSync, statSync } from "node:fs";
import { join } from "node:path";

/** Every file below `directory`, recursively, skipping dependency and build output. */
export function filesUnder(directory: string): string[] {
  return readdirSync(directory).flatMap((name) => {
    if (name === "node_modules" || name === "dist") return [];
    const path = join(directory, name);
    return statSync(path).isDirectory() ? filesUnder(path) : [path];
  });
}
