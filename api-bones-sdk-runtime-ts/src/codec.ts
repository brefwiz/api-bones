// SPDX-License-Identifier: MIT
import { fromBinary, type DescMessage, type MessageShape } from "@bufbuild/protobuf";
import { Total_Kind } from "./generated/bones/v1/pagination_pb.js";
import { BatchChunkSchema, PageSchema } from "./generated/bones/sdk/v1/boundary_pb.js";
import { errorOf } from "./call-error.js";
import { BonesError } from "./errors.js";
import type { Page } from "./list-call.js";

/** One result of a batch chunk, in request order. */
export type BatchItem<T> = { readonly ok: T } | { readonly error: BonesError };

/**
 * The page a handle delivered: the bytes of a `bones.sdk.v1.Page`, each of its
 * items decoded by the item message's schema.
 */
export function decodePage<Desc extends DescMessage>(delivered: Uint8Array, schema: Desc): Page<MessageShape<Desc>> {
  const page = fromBinary(PageSchema, delivered);
  return {
    items: page.items.map((item) => fromBinary(schema, item)),
    revision: page.revision,
    fromCache: page.fromCache,
    hasNext: page.hasNext,
    ...(page.total === undefined ? {} : { total: { value: page.total.value, kind: page.total.kind === Total_Kind.EXACT ? ("exact" as const) : ("estimate" as const) } }),
  };
}

/**
 * The chunk a handle delivered: the bytes of a `bones.sdk.v1.BatchChunk`, one
 * outcome per requested id in request order, each item decoded by the item
 * message's schema. An outcome that names neither an item nor an error is
 * reported as a failed precondition of that id.
 */
export function decodeBatchChunk<Desc extends DescMessage>(bytes: Uint8Array, schema: Desc): BatchItem<MessageShape<Desc>>[] {
  return fromBinary(BatchChunkSchema, bytes).results.map((result): BatchItem<MessageShape<Desc>> => {
    switch (result.outcome.case) {
      case "item":
        return { ok: fromBinary(schema, result.outcome.value) };
      case "error":
        return { error: errorOf(result.outcome.value) };
      default:
        return {
          error: new BonesError({
            code: "PRECONDITION_FAILED",
            emitter: "bones",
            platform: "PRECONDITION_FAILED",
            connectCode: "failed_precondition",
            message: "the batch outcome named neither an item nor an error",
          }),
        };
    }
  });
}
