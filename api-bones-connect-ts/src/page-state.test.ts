// SPDX-License-Identifier: MIT
import { Code, ConnectError } from "@connectrpc/connect";
import { afterEach, expect, it, vi } from "vitest";

import { pageShared } from "./page-state.js";

afterEach(() => vi.restoreAllMocks());

it("shares not-delivered evidence between independent package copies", async () => {
  vi.resetModules();
  const copyA = await import("./delivery-evidence.js");
  vi.resetModules();
  const copyB = await import("./delivery-evidence.js");
  expect(copyA.markNotDelivered).not.toBe(copyB.markNotDelivered);
  const error = new ConnectError("never sent", Code.Internal);
  copyA.markNotDelivered(error);
  expect(copyB.isNotDelivered(error)).toBe(true);
  expect(copyB.isNotDelivered(new ConnectError("never sent", Code.Internal))).toBe(false);
});

it("ignores a foreign value without replacing it or throwing", () => {
  const name = "test-foreign";
  const key = Symbol.for(`@brefwiz/api-bones-connect/${name}/v1`);
  const foreign = {};
  Object.defineProperty(globalThis, key, { value: foreign });
  const get = () => pageShared(name, () => new Map<string, string>(), (v): v is Map<string, string> => v instanceof Map);
  expect(get).not.toThrow();
  expect(get()).toBeInstanceOf(Map);
  expect(get()).toBe(get());
  expect(Reflect.get(globalThis, key)).toBe(foreign);
});

it("keeps a local instance when the page refuses installation", () => {
  const key = Symbol.for("@brefwiz/api-bones-connect/test-locked/v1");
  const original = Object.defineProperty;
  const define = vi.spyOn(Object, "defineProperty").mockImplementation((target, property, descriptor) => {
    if (target === globalThis && property === key) throw new TypeError("locked page");
    return original(target, property, descriptor);
  });
  const get = () => pageShared("test-locked", () => new WeakSet<object>(), (v): v is WeakSet<object> => v instanceof WeakSet);
  expect(get).not.toThrow();
  expect(get()).toBe(get());
  define.mockRestore();
});
