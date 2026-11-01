// SPDX-License-Identifier: MIT
//
// Test-only: the hand-built descriptor of bones/v1/stream.proto, shared by the
// unit tests and the contract lane's steps. Not part of the published build.

import { create, createFileRegistry, type MessageShape } from "@bufbuild/protobuf";
import {
  FieldDescriptorProto_Label,
  FieldDescriptorProto_Type,
  FieldDescriptorProtoSchema,
  FileDescriptorProtoSchema,
  FileDescriptorSetSchema,
  type FieldDescriptorProto,
} from "@bufbuild/protobuf/wkt";

export const T = FieldDescriptorProto_Type;

export function field(
  name: string,
  jsonName: string,
  number: number,
  type: FieldDescriptorProto_Type,
  extra: Partial<FieldDescriptorProto> = {},
) {
  return create(FieldDescriptorProtoSchema, {
    name,
    jsonName,
    number,
    type,
    label: FieldDescriptorProto_Label.OPTIONAL,
    ...extra,
  });
}

export const streamFile = create(FileDescriptorProtoSchema, {
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
});

/** A field carrying a `bones.v1.StreamControl` under the given number. */
export const control = (number: number, extra: Partial<FieldDescriptorProto> = {}) =>
  field("bones_control", "bonesControl", number, T.MESSAGE, {
    typeName: ".bones.v1.StreamControl",
    ...extra,
  });

/** A registry of the stream file and one application file that depends on it. */
export function registryWith(appFile: MessageShape<typeof FileDescriptorProtoSchema>) {
  return createFileRegistry(create(FileDescriptorSetSchema, { file: [streamFile, appFile] }));
}
