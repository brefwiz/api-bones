// SPDX-License-Identifier: MIT
//
// Test-only: the hand-built descriptor of bones/v1/stream.proto, shared by the
// unit tests and the contract lane's steps. Plain data with no imports, so
// either runner builds the descriptor with its own protobuf runtime. Not part
// of the published build.

/** FieldDescriptorProto.Type numbers. */
export const T = { UINT32: 13, UINT64: 4, STRING: 9, MESSAGE: 11 } as const;

const OPTIONAL = 1;

export interface FieldExtra {
  typeName?: string;
  oneofIndex?: number;
  label?: number;
}

export function field(
  name: string,
  jsonName: string,
  number: number,
  type: number,
  extra: FieldExtra = {},
) {
  return { name, jsonName, number, type, label: OPTIONAL, ...extra };
}

export const streamFile = {
  name: "bones/v1/stream.proto",
  package: "bones.v1",
  syntax: "proto3",
  messageType: [
    {
      name: "Drain",
      field: [field("cut_in_ms", "cutInMs", 1, T.UINT32)],
    },
    {
      name: "Checkpoint",
      field: [
        field("domain", "domain", 1, T.STRING),
        field("live_from", "liveFrom", 2, T.UINT64),
        field("delivered_through", "deliveredThrough", 3, T.UINT64),
        field("epoch", "epoch", 4, T.UINT64),
      ],
    },
    {
      name: "StreamControl",
      oneofDecl: [{ name: "signal" }],
      field: [
        field("drain", "drain", 1, T.MESSAGE, { typeName: ".bones.v1.Drain", oneofIndex: 0 }),
        field("checkpoint", "checkpoint", 2, T.MESSAGE, {
          typeName: ".bones.v1.Checkpoint",
          oneofIndex: 0,
        }),
      ],
    },
  ],
};

/** A field carrying a `bones.v1.StreamControl` under the given number. */
export const control = (number: number, extra: FieldExtra = {}) =>
  field("bones_control", "bonesControl", number, T.MESSAGE, {
    typeName: ".bones.v1.StreamControl",
    ...extra,
  });
