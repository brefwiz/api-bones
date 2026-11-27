// SPDX-License-Identifier: MIT
import { create, fromBinary, toBinary, type DescMessage, type MessageInitShape } from "@bufbuild/protobuf";
import { Int32ValueSchema } from "@bufbuild/protobuf/wkt";
import {
  BatchChunkSchema,
  CallErrorSchema,
  PageSchema,
  SpanEndSchema,
  SpanStartSchema,
  SpanStartedSchema,
} from "../src/generated/bones/sdk/v1/boundary_pb.js";

/** The protobuf bytes of a message. */
export function bytesOf<Desc extends DescMessage>(schema: Desc, init: MessageInitShape<Desc>): Uint8Array {
  return toBinary(schema, create(schema, init));
}

export const callErrorBytes = (init: MessageInitShape<typeof CallErrorSchema>): Uint8Array => bytesOf(CallErrorSchema, init);

/** The `CallError` a refusal of `code` carries. */
export function refusal(emitter: string, code: string, platform: string, connectCode: string, message: string): Uint8Array {
  return callErrorBytes({ code, emitter, platform, connectCode, message });
}

/** One number as the item message the unit tests decode: an `Int32Value`. */
export const numberBytes = (value: number): Uint8Array => bytesOf(Int32ValueSchema, { value });
export const numberOf = (bytes: Uint8Array): number => fromBinary(Int32ValueSchema, bytes).value;

/** A page of numbers as the bytes a handle delivers. */
export function pageBytes(items: readonly number[], hasNext: boolean): Uint8Array {
  return bytesOf(PageSchema, { items: items.map(numberBytes), revision: "r1", fromCache: false, hasNext });
}

export const batchChunkBytes = (init: MessageInitShape<typeof BatchChunkSchema>): Uint8Array => bytesOf(BatchChunkSchema, init);
export const spanStartBytes = (init: MessageInitShape<typeof SpanStartSchema>): Uint8Array => bytesOf(SpanStartSchema, init);
export const spanEndBytes = (init: MessageInitShape<typeof SpanEndSchema>): Uint8Array => bytesOf(SpanEndSchema, init);

/** What a sink answered when a span started. */
export const startedOf = (bytes: Uint8Array): { traceparent: string; recording: boolean } => {
  const { traceparent, recording } = fromBinary(SpanStartedSchema, bytes);
  return { traceparent, recording };
};
