// SPDX-License-Identifier: MIT

const localStates = new Map<string, object>();

/** Share compatible state across package copies in one page, with a local fallback. */
export function pageShared<T extends object>(
  name: string,
  create: () => T,
  accepts: (value: unknown) => value is T,
): T {
  const local = (): T => {
    const existing = localStates.get(name);
    if (accepts(existing)) return existing;
    const value = create();
    localStates.set(name, value);
    return value;
  };
  try {
    const key = Symbol.for(`@brefwiz/api-bones-connect/${name}/v1`);
    const page = globalThis as typeof globalThis & Record<symbol, unknown>;
    const existing = page[key];
    if (accepts(existing)) return existing;
    if (key in page) return local();
    const value = local();
    Object.defineProperty(page, key, { value, enumerable: false, writable: false, configurable: false });
    return value;
  } catch {
    return local();
  }
}
