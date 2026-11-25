// SPDX-License-Identifier: MIT
import { Code, ConnectError } from "@connectrpc/connect";
import { describe, expect, it } from "vitest";
import type {
  PageError as RuntimePageError,
  isCursorRejected as runtimeIsCursorRejected,
} from "@brefwiz/api-bones-sdk-runtime";
import { PageError, isCursorRejected } from "./page.js";

/** `true` only when each type is assignable to the other. */
type Same<A, B> = [A] extends [B] ? ([B] extends [A] ? true : false) : false;

describe("the page error surface is structurally the one the runtime ships", () => {
  it("PageError and isCursorRejected", () => {
    const error: Same<
      Pick<PageError, "kind" | "deliveredPages" | "deliveredItems">,
      Pick<RuntimePageError, "kind" | "deliveredPages" | "deliveredItems">
    > = true;
    const signature: Same<typeof isCursorRejected, typeof runtimeIsCursorRejected> = true;
    expect([error, signature]).toEqual([true, true]);
  });

  it("a page error without a cursor rejection keeps the rpc kind and is not a cursor rejection", () => {
    const error = new PageError(new ConnectError("unavailable", Code.Unavailable), 2, 4);
    expect([error.kind, error.deliveredPages, error.deliveredItems, isCursorRejected(error)]).toEqual(["rpc", 2, 4, false]);
  });
});
