// SPDX-License-Identifier: MIT
/**
 * What a scenario of the node-transport feature changes on the host's global
 * object, and gives back. The steps call these and never touch the global
 * object themselves: the host's shape is scenario setup, not part of any call
 * the SDK makes.
 */

/** The slot the wasm core's Node host exchange is installed in. */
const EXCHANGE_SLOT = Symbol.for("brefwiz.api-bones.node-exchange");

/** The globals a scenario defined to make the host look like another runtime, as they were. */
const defined: { name: string; had: boolean; value: unknown }[] = [];

/** The exchange a scenario took out of its slot. */
let taken: { value: unknown } | undefined;

const host = globalThis as unknown as Record<string | symbol, unknown>;

/** Makes the host look like a runtime that defines `name`. */
export function defineHostGlobal(name: string): void {
  defined.push({ name, had: name in host, value: host[name] });
  host[name] = {};
}

/** Takes the api-bones exchange out of its slot. */
export function withdrawExchange(): void {
  taken = { value: host[EXCHANGE_SLOT] };
  delete host[EXCHANGE_SLOT];
}

/**
 * Takes `crypto` off the global object and returns the call that gives it
 * back, for a host that loses its randomness after the SDK was opened.
 */
export function withdrawRandomValues(): () => void {
  const crypto = Object.getOwnPropertyDescriptor(globalThis, "crypto");
  Object.defineProperty(globalThis, "crypto", { value: undefined, configurable: true });
  return () => {
    if (crypto === undefined) delete host["crypto"];
    else Object.defineProperty(globalThis, "crypto", crypto);
  };
}

/** Gives back everything the scenario changed on the global object. */
export function restoreHost(): void {
  for (const { name, had, value } of defined.splice(0).reverse()) {
    if (had) host[name] = value;
    else delete host[name];
  }
  if (taken !== undefined) {
    host[EXCHANGE_SLOT] = taken.value;
    taken = undefined;
  }
}
