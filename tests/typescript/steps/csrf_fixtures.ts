// SPDX-License-Identifier: MIT
/**
 * Policy and method descriptors shared by the step lane and the unit tests
 * for the browser transport's CSRF synchronizer. Imports are type-only so
 * either runner resolves it with its own protobuf runtime.
 */

import type { DescMessage, DescMethodStreaming, DescMethodUnary } from "@bufbuild/protobuf";

export const SYNC = "x-csrf-token";

const orgField = { name: "org_handle", jsonName: "orgHandle", number: 1 };

const method = (rpc: string, extra: object = {}) => ({
  rpc: `/pkg.v1.SessionService/${rpc}`,
  procedure: "unary",
  idempotency: "UNSPECIFIED",
  browserCache: { scope: "NO_STORE", maxAgeSeconds: 0 },
  sensitivity: "UNSPECIFIED",
  maxEncodedUrlBytes: 4096,
  ...extra,
});

export const policy = {
  schemaVersion: 1,
  methods: [
    method("GetWeek", {
      idempotency: "NO_SIDE_EFFECTS",
      sensitivity: "NON_SENSITIVE",
      publicRead: { maxAgeSeconds: 60, origins: "OWNER_CONFIRMED", orgField },
    }),
    method("Book"),
  ],
};

/** Unary and server-streaming descriptors over the given message schemas. */
export function descriptors<I extends DescMessage, O extends DescMessage>(input: I, output: O) {
  const base = (name: string, methodKind: string, idempotency: number) => ({
    kind: "rpc",
    name,
    localName: name.charAt(0).toLowerCase() + name.slice(1),
    parent: { typeName: "pkg.v1.SessionService" },
    methodKind,
    input,
    output,
    idempotency,
    deprecated: false,
  });
  return {
    unary: (name: string) =>
      base(name, "unary", name.startsWith("Get") ? 1 : 0) as unknown as DescMethodUnary<I, O>,
    watch: () => base("Watch", "server_streaming", 0) as unknown as DescMethodStreaming<I, O>,
  };
}
