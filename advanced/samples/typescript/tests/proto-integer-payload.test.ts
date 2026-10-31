import * as common from "@temporalio/common";
import {
  DefaultPayloadConverterWithProtobufs,
  ProtobufBinaryPayloadConverter,
} from "@temporalio/common/lib/protobufs";
import proto from "@temporalio/proto";
import Long from "long";
import { describe, expect, test } from "vitest";

import {
  signedValueTransferTypeConverter,
  unsignedValueTransferTypeConverter,
} from "../wit/integer-bindings/models.ts";

describe("SDK protobuf integer payloads", () => {
  test.each([
    [
      "json/protobuf",
      new DefaultPayloadConverterWithProtobufs({ protobufRoot: proto }),
    ],
    [
      "binary/protobuf",
      new common.CompositePayloadConverter(new ProtobufBinaryPayloadConverter(proto)),
    ],
  ] as const)(
    "preserves signed and unsigned 64-bit values through %s",
    (encoding, converter) => {
      // These well-known messages are shipped by @temporalio/proto; no sample
      // descriptors or mock constructors are needed for the payload boundary.
      for (const [transfer, value] of [
        [signedValueTransferTypeConverter, Long.fromString("9007199254740993")],
        [signedValueTransferTypeConverter, Long.fromString("-9007199254740993")],
        [
          unsignedValueTransferTypeConverter,
          Long.fromString("18446744073709551615", true),
        ],
      ] as const) {
        const typeInfo = { transferTypeConverter: transfer };
        const model = { value };
        const payload = common.toPayloadWithTypeInfo(
          converter,
          model,
          undefined,
          typeInfo,
        );
        expect(new TextDecoder().decode(payload.metadata?.encoding)).toBe(encoding);
        expect(new TextDecoder().decode(payload.metadata?.messageType)).toBe(
          transfer === signedValueTransferTypeConverter
            ? "google.protobuf.Int64Value"
            : "google.protobuf.UInt64Value",
        );
        if (encoding === "json/protobuf") {
          expect(JSON.parse(new TextDecoder().decode(payload.data ?? undefined))).toBe(
            value.toString(),
          );
        }
        const decoded = common.fromPayloadWithTypeInfo(
          converter,
          payload,
          undefined,
          typeInfo,
        );
        expect(Long.isLong(decoded.value)).toBe(true);
        expect(decoded.value?.toString()).toBe(value.toString());
        expect(decoded.value?.unsigned).toBe(value.unsigned);
      }
    },
  );
});
