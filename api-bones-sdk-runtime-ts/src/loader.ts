// SPDX-License-Identifier: MIT

/**
 * Starts something once, on first use. Concurrent callers share the one start.
 * A failed start is forgotten, so the next caller starts again.
 */
export function lazyOnce<T>(start: () => Promise<T>): () => Promise<T> {
  let started: Promise<T> | undefined;
  return () => {
    if (started === undefined) {
      const attempt = start();
      started = attempt;
      attempt.catch(() => {
        if (started === attempt) started = undefined;
      });
    }
    return started;
  };
}

interface NodeFiles {
  readFile(url: URL): Promise<Uint8Array>;
}

/**
 * What the wasm module's initialiser takes for the module at `url`. A `file:`
 * module can only be reached from Node, so its bytes are read; any other
 * location is handed back for the initialiser to load.
 */
export async function wasmInput(url: URL): Promise<URL | Uint8Array> {
  if (url.protocol !== "file:") return url;
  const specifier = "node:fs/promises";
  const files = (await import(specifier)) as NodeFiles;
  return files.readFile(url);
}
