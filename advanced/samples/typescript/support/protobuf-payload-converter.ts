import proto from "@temporalio/proto";
import { DefaultPayloadConverterWithProtobufs } from "@temporalio/common/lib/protobufs.js";

export const payloadConverter = new DefaultPayloadConverterWithProtobufs({
  protobufRoot: proto,
});
