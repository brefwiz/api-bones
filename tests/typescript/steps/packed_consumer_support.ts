// SPDX-License-Identifier: MIT
import { spawnSync } from "node:child_process";
import {
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, sep } from "node:path";
import { fileURLToPath } from "node:url";
import assert from "node:assert/strict";

/** Real installed-package proof shared by Vitest and the owning Gherkin lane. */
export function createPackedConsumerProof() {
  const owner = fileURLToPath(
    new URL("../../../api-bones-connect-ts/", import.meta.url),
  );
  const lock = JSON.parse(
    readFileSync(join(owner, "package-lock.json"), "utf8"),
  );
  const root = mkdtempSync(join(tmpdir(), "packed-transport-"));
  const environment = { ...process.env, NODE_PATH: "", NODE_OPTIONS: "" };
  const adapters = [
    "@connectrpc/connect-web",
    "@connectrpc/connect-node",
  ] as const;
  let tarball: string;

  function run(command: string, args: string[], cwd: string) {
    return spawnSync(command, args, {
      cwd,
      env: environment,
      encoding: "utf8",
      timeout: 120_000,
    });
  }

  function consumer(selected: readonly string[], incompatible?: string) {
    const cwd = mkdtempSync(join(root, "consumer-"));
    const dependencies: Record<string, string> = {
      "@brefwiz/api-bones-connect": `file:${tarball}`,
      "@bufbuild/protobuf":
        lock.packages["node_modules/@bufbuild/protobuf"].version,
      "@connectrpc/connect":
        lock.packages["node_modules/@connectrpc/connect"].version,
    };
    for (const name of selected)
      dependencies[name] =
        name === incompatible
          ? "1.6.1"
          : lock.packages[`node_modules/${name}`].version;
    writeFileSync(
      join(cwd, "package.json"),
      JSON.stringify({
        name: "isolated-transport-consumer",
        private: true,
        type: "module",
        dependencies,
      }),
    );
    const install = run(
      "npm",
      [
        "install",
        "--strict-peer-deps",
        "--ignore-scripts",
        "--no-audit",
        "--no-fund",
      ],
      cwd,
    );
    return { cwd, install };
  }

  function probe(cwd: string, entry: string) {
    const installed = realpathSync(
      join(cwd, "node_modules/@brefwiz/api-bones-connect"),
    );
    assert.equal(installed.startsWith(realpathSync(cwd) + sep), true);
    writeFileSync(
      join(cwd, "probe.mjs"),
      `import { fileURLToPath } from "node:url";
import { realpathSync } from "node:fs";
import { sep } from "node:path";
const resolved = realpathSync(fileURLToPath(import.meta.resolve(${JSON.stringify(entry)})));
if (!resolved.startsWith(${JSON.stringify(installed)} + sep)) throw new Error("entry escaped installed package");
await import(${JSON.stringify(entry)});
`,
    );
    return run(process.execPath, [join(cwd, "probe.mjs")], cwd);
  }

  try {
    const packed = run(
      "npm",
      ["pack", "--json", "--ignore-scripts", "--pack-destination", root],
      owner,
    );
    assert.equal(packed.status, 0);
    tarball = join(root, JSON.parse(packed.stdout)[0].filename);
  } catch (error) {
    rmSync(root, { recursive: true, force: true });
    throw error;
  }
  return {
    close() {
      rmSync(root, { recursive: true, force: true });
    },
    proveNeutral() {
      const { cwd, install } = consumer([]);
      assert.equal(install.status, 0);
      const manifest = JSON.parse(
        readFileSync(
          join(cwd, "node_modules/@brefwiz/api-bones-connect/package.json"),
          "utf8",
        ),
      );
      assert.deepEqual(
        manifest.peerDependencies,
        lock.packages[""].peerDependencies,
      );
      for (const name of adapters) {
        assert.equal(manifest.peerDependencies[name], "^2.0.0");
        assert.deepEqual(manifest.peerDependenciesMeta[name], {
          optional: true,
        });
        const absent = run(
          process.execPath,
          ["--input-type=module", "-e", `import(${JSON.stringify(name)})`],
          cwd,
        );
        assert.notEqual(absent.status, 0);
        assert.ok(absent.stderr.includes("ERR_MODULE_NOT_FOUND"));
      }
      assert.equal(probe(cwd, "@brefwiz/api-bones-connect").status, 0);
      for (const entry of ["web", "node"]) {
        const missing = probe(cwd, `@brefwiz/api-bones-connect/${entry}`);
        assert.notEqual(missing.status, 0);
        assert.ok(missing.stderr.includes("ERR_MODULE_NOT_FOUND"));
        assert.ok(missing.stderr.includes(`@connectrpc/connect-${entry}`));
      }
    },
    proveSelected(entries: readonly string[]) {
      assert.ok(entries.length > 0);
      assert.ok(entries.every((entry) => entry === "web" || entry === "node"));
      const { cwd, install } = consumer(
        entries.map((entry) => `@connectrpc/connect-${entry}`),
      );
      assert.equal(install.status, 0);
      for (const entry of entries)
        assert.equal(
          probe(cwd, `@brefwiz/api-bones-connect/${entry}`).status,
          0,
        );
    },
    proveLinked() {
      const sdk = consumer(["@connectrpc/connect-web"]);
      assert.equal(sdk.install.status, 0);
      const sdkManifestPath = join(sdk.cwd, "package.json");
      const sdkManifest = JSON.parse(readFileSync(sdkManifestPath, "utf8"));
      sdkManifest.name = "fixture-generated-sdk";
      sdkManifest.exports = "./index.mjs";
      writeFileSync(sdkManifestPath, JSON.stringify(sdkManifest));
      writeFileSync(
        join(sdk.cwd, "index.mjs"),
        `import { configureConnectTransport } from "@brefwiz/api-bones-connect/web";
export const transportFactory = configureConnectTransport;
export const ownerUrl = import.meta.resolve("@brefwiz/api-bones-connect/web");
export const adapterUrl = import.meta.resolve("@connectrpc/connect-web");
`,
      );
      const app = consumer([]);
      assert.equal(app.install.status, 0);
      const linked = run(
        "npm",
        [
          "install",
          `file:${sdk.cwd}`,
          "--install-links=false",
          "--strict-peer-deps",
          "--ignore-scripts",
          "--no-audit",
          "--no-fund",
        ],
        app.cwd,
      );
      assert.equal(linked.status, 0);
      assert.equal(
        realpathSync(join(app.cwd, "node_modules/fixture-generated-sdk")),
        realpathSync(sdk.cwd),
      );
      const installedOwner = realpathSync(
        join(sdk.cwd, "node_modules/@brefwiz/api-bones-connect"),
      );
      const installedAdapter = realpathSync(
        join(sdk.cwd, "node_modules/@connectrpc/connect-web"),
      );
      assert.equal(
        installedOwner.startsWith(realpathSync(sdk.cwd) + sep),
        true,
      );
      assert.equal(
        installedAdapter.startsWith(realpathSync(sdk.cwd) + sep),
        true,
      );
      writeFileSync(
        join(app.cwd, "sdk-probe.mjs"),
        `import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { realpathSync } from "node:fs";
import { sep } from "node:path";
import { transportFactory, ownerUrl, adapterUrl } from "fixture-generated-sdk";
assert.equal(typeof transportFactory, "function");
assert.ok(realpathSync(fileURLToPath(ownerUrl)).startsWith(${JSON.stringify(installedOwner)} + sep));
assert.ok(realpathSync(fileURLToPath(adapterUrl)).startsWith(${JSON.stringify(installedAdapter)} + sep));
`,
      );
      assert.equal(
        run(process.execPath, [join(app.cwd, "sdk-probe.mjs")], app.cwd).status,
        0,
      );
    },
    proveIncompatible(entry: string) {
      assert.ok(entry === "web" || entry === "node");
      const adapter = `@connectrpc/connect-${entry}`;
      const { install } = consumer([adapter], adapter);
      assert.notEqual(install.status, 0);
      assert.ok(install.stderr.includes("ERESOLVE"));
      assert.ok(install.stderr.includes(adapter));
    },
  };
}
