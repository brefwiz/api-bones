// SPDX-License-Identifier: MIT
import { readFileSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { filesUnder } from "./files-under.js";

const packageRoot = fileURLToPath(new URL("..", import.meta.url));
const sourceDir = join(packageRoot, "src");

/** Source text with comments removed: prose may name what the code must not do. */
function code(file: string): string {
  return readFileSync(join(sourceDir, file), "utf8").replace(/\/\*[\s\S]*?\*\/|\/\/.*$/gm, "");
}

/**
 * What the package must not decide. A few files carry a word as data the core
 * reported or as the shape of a value that crosses the boundary: they are named
 * beside the pattern, and the decision itself stays in the core.
 */
const forbidden: Array<[string, RegExp, readonly string[]]> = [
  ["a network call", /\bfetch\s*\(|XMLHttpRequest|axios|WebSocket|EventSource/, []],
  ["a timer, so no backoff", /setTimeout|setInterval|\bsleep\b|requestAnimationFrame/, []],
  // The wait a refusal named is a datum of the error, never acted on here.
  ["retry", /retr(y|ies)|backoff|jitter|attempt\s*[<>+]/i, ["errors.ts", "call-error.ts"]],
  ["cursor handling", /next_?cursor|page_?token|\bcursor\s*[:=]|PageRequest/i, []],
  // The decoder of a batch chunk hands over what the core delivered, in order.
  ["batching", /batch|coalesc/i, ["codec.ts"]],
  ["CSRF", /csrf|xsrf/i, []],
  // The session answers the core's credential ask; whether, when and how often to ask is the core's.
  ["credentials", /credential|authorization|bearer|\bcookie/i, ["session.ts"]],
  ["a deadline", /deadline|timeout|AbortSignal\.timeout/i, []],
];

describe("the package holds no decision logic", () => {
  const files = filesUnder(sourceDir)
    .filter((path) => path.endsWith(".ts"))
    .map((path) => relative(sourceDir, path))
    // The protobuf-es codec of the protos is generated, not written here.
    .filter((file) => !file.startsWith("generated/"));

  it("has sources to check", () => {
    expect(files.length).toBeGreaterThan(0);
  });

  it.each(forbidden)("contains no %s", (_what, pattern, allowed) => {
    for (const file of files.filter((name) => !allowed.includes(name))) expect({ file, found: pattern.exec(code(file))?.[0] }).toEqual({ file, found: undefined });
  });

  it("declares no HTTP client dependency", () => {
    const manifest = JSON.parse(readFileSync(join(packageRoot, "package.json"), "utf8")) as Record<string, Record<string, string> | undefined>;
    const names = ["dependencies", "peerDependencies", "devDependencies"].flatMap((group) => Object.keys(manifest[group] ?? {}));
    expect(names.filter((name) => /axios|connect|fetch|got$|undici|ky$/.test(name))).toEqual([]);
  });
});

describe("one OpenTelemetry API version", () => {
  it("is declared with a single range", () => {
    const manifest = JSON.parse(readFileSync(join(packageRoot, "package.json"), "utf8")) as Record<string, Record<string, string> | undefined>;
    const ranges = new Set(
      ["dependencies", "peerDependencies", "devDependencies", "optionalDependencies"]
        .map((group) => manifest[group]?.["@opentelemetry/api"])
        .filter((range): range is string => range !== undefined),
    );
    expect([...ranges]).toEqual([">=1.9.0 <1.10.0"]);
  });

  it("is the range the sibling package pins", () => {
    const sibling = JSON.parse(readFileSync(join(packageRoot, "..", "api-bones-connect-ts", "package.json"), "utf8")) as { dependencies: Record<string, string> };
    const own = JSON.parse(readFileSync(join(packageRoot, "package.json"), "utf8")) as { peerDependencies: Record<string, string> };
    expect(own.peerDependencies["@opentelemetry/api"]).toBe(sibling.dependencies["@opentelemetry/api"]);
  });
});
