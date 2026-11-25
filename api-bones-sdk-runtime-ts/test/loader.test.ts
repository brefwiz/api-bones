// SPDX-License-Identifier: MIT
import { fileURLToPath, pathToFileURL } from "node:url";
import { describe, expect, it } from "vitest";
import { lazyOnce, wasmInput } from "../src/index.js";

describe("lazyOnce", () => {
  it("does not start until first use and starts once for concurrent callers", async () => {
    let starts = 0;
    const load = lazyOnce(async () => {
      starts += 1;
      return { ready: true };
    });
    expect(starts).toBe(0);
    const [a, b] = await Promise.all([load(), load()]);
    expect(a).toBe(b);
    await load();
    expect(starts).toBe(1);
  });

  it("forgets a failed start so the next caller starts again", async () => {
    let starts = 0;
    const load = lazyOnce(async () => {
      starts += 1;
      if (starts === 1) throw new Error("module unreachable");
      return starts;
    });
    await expect(load()).rejects.toThrow("module unreachable");
    expect(await load()).toBe(2);
  });
});

describe("wasmInput", () => {
  it("reads the bytes of a file module", async () => {
    const input = await wasmInput(pathToFileURL(fileURLToPath(import.meta.url)));
    expect(input).toBeInstanceOf(Uint8Array);
  });

  it("hands any other location back for the initialiser to load", async () => {
    const url = new URL("https://assets.example.test/sdk_bg.wasm");
    expect(await wasmInput(url)).toBe(url);
  });
});
