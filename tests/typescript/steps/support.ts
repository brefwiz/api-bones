// SPDX-License-Identifier: MIT
/**
 * Fixtures shared across this step lane's files.
 *
 * Every contract here answers a single-method `pkg.v1.<Service>/Method`
 * Connect service and the generated-policy document describing it, and the
 * retry contracts both name Connect codes by their contract string. Keeping
 * one copy of each shape means a step file adds only what its own scenario
 * needs.
 */

import type { DescMethodUnary } from "@bufbuild/protobuf";
import { EmptySchema, StringValueSchema } from "@bufbuild/protobuf/wkt";
import { Code } from "@connectrpc/connect";

export const CONNECT_CODES: Readonly<Record<string, Code>> = {
  internal: Code.Internal,
  unavailable: Code.Unavailable,
  unauthenticated: Code.Unauthenticated,
  aborted: Code.Aborted,
  resource_exhausted: Code.ResourceExhausted,
  permission_denied: Code.PermissionDenied,
};

export const CONNECT_CODE_NAMES: ReadonlyMap<Code, string> = new Map(
  Object.entries(CONNECT_CODES).map(([name, code]) => [code, name] as const),
);

/** The Connect code a contract table names by string, or a loud failure on a typo. */
export function connectCodeOf(name: string): Code {
  const code = CONNECT_CODES[name];
  if (code === undefined) {
    throw new Error(`the contract names a code this step cannot build: ${name}`);
  }
  return code;
}

/** A single-method unary Connect service, generic enough for any step fixture. */
export function unaryMethodFixture(
  serviceTypeName: string,
  methodName: string,
): DescMethodUnary<typeof StringValueSchema, typeof EmptySchema> {
  return {
    kind: "rpc",
    name: methodName,
    localName: methodName.charAt(0).toLowerCase() + methodName.slice(1),
    parent: { typeName: serviceTypeName },
    methodKind: "unary",
    input: StringValueSchema,
    output: EmptySchema,
    idempotency: 0,
    deprecated: false,
  } as unknown as DescMethodUnary<typeof StringValueSchema, typeof EmptySchema>;
}

/** The generated-policy document declaring that same method at the given idempotency. */
export function unaryPolicyDoc(rpc: string, idempotency: string): unknown {
  return {
    schemaVersion: 1,
    methods: [
      {
        rpc,
        procedure: "unary",
        idempotency,
        browserCache: { scope: "NO_STORE", maxAgeSeconds: 0 },
        sensitivity: "UNSPECIFIED",
        maxEncodedUrlBytes: 4096,
      },
    ],
  };
}
