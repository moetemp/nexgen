// Some sample protos are newer than the SDK. Install their real message types
// from the checked-in descriptors so converter tests use protobuf serialization,
// not constructor mocks. Existing SDK types are left intact.
import { readFileSync } from "node:fs";
import proto from "@temporalio/proto";
import { patchProtobufRoot } from "@temporalio/common/lib/protobufs";
import protobuf from "protobufjs";
import * as descriptor from "protobufjs/ext/descriptor.js";

const root = proto as unknown as protobuf.Root & Record<string, unknown>;

for (const filename of ["temporal_api.bin", "streams.bin"]) {
  const set = descriptor.FileDescriptorSet.decode(
    readFileSync(new URL(`../../descriptors/${filename}`, import.meta.url)),
  ) as unknown as descriptor.IFileDescriptorSet;
  for (const file of set.file) {
    const namespace = root.define(file.package ?? "");
    for (const message of file.messageType ?? []) {
      if (namespace.get(message.name ?? "") == null) {
        namespace.add(protobuf.Type.fromDescriptor(message, file.syntax));
      }
    }
    for (const enumeration of file.enumType ?? []) {
      if (namespace.get(enumeration.name ?? "") == null) {
        namespace.add(protobuf.Enum.fromDescriptor(enumeration));
      }
    }
  }
}
root.resolveAll();

const patched = patchProtobufRoot(root);
for (const name of Object.keys(root.nested ?? {})) {
  root[name] = patched[name];
}
