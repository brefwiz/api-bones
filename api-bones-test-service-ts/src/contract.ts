// SPDX-License-Identifier: MIT

import { readFileSync } from "node:fs";

import { createFileRegistry, fromBinary, type FileRegistry } from "@bufbuild/protobuf";
import { FileDescriptorSetSchema } from "@bufbuild/protobuf/wkt";

/** The descriptor set the build writes beside the compiled sources, from the contract protos of `tests/l2/proto`. */
const DESCRIPTOR_SET = new URL("./contract.binpb", import.meta.url);

let registry: FileRegistry | undefined;

/** The registry of the test contract (`contract.v1.WidgetsService`, `NumbersService` and `EventsService`) with the shipped `bones/v1` files it imports. */
export function contractRegistry(): FileRegistry {
  registry ??= createFileRegistry(fromBinary(FileDescriptorSetSchema, readFileSync(DESCRIPTOR_SET)));
  return registry;
}
