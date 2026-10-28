import * as common from "@temporalio/common";
import type { temporal } from "@temporalio/proto";
import Long from "long";
import {
  afterEach,
  beforeEach,
  describe,
  expect,
  expectTypeOf,
  test,
  vi,
} from "vitest";

import {
  type ReadRecordsResponse,
  StreamCloseStatus,
  describeStreamResponseTransferTypeConverter,
  readRecordsRequestTransferTypeConverter,
  readRecordsResponseTransferTypeConverter,
} from "../wit/stream-service/models.ts";
import { createStream } from "../wit/stream-service/operations/create-stream.ts";
import { Stream } from "../wit/stream-service/resources.ts";

const startOperation = vi.hoisted(() => vi.fn());
vi.mock("@temporalio/workflow", () => ({
  createNexusServiceClient: () => ({ startOperation }),
  workflowInfo: () => ({
    workflowId: "chat-1",
    firstExecutionRunId: "r1",
    runId: "r2",
  }),
}));

// A body goes through the payload converter of the Workflow.
beforeEach(() => {
  vi.stubGlobal("__TEMPORAL_ACTIVATOR__", {
    payloadConverter: common.defaultPayloadConverter,
  });
});

afterEach(() => vi.unstubAllGlobals());

class Turn {
  public constructor(public readonly role: string) {}
}

const turnType: common.TypeInfo<Turn, { role: string }> = {
  transferTypeConverter: {
    toTransferType: (turn) => ({ role: turn.role }),
    fromTransferType: (value) => new Turn(value.role),
  },
};

describe("stream-service generated output", () => {
  // No SDK has these protos yet, so proto/streams.d.ts declares them. The converters fail to
  // compile when those declarations drift from what they read and write.
  test("maps each read item kind through the proto oneof", () => {
    const model: ReadRecordsResponse<string> = {
      records: [
        {
          item: {
            tag: "data",
            value: { body: "hi" },
          },
          offset: Long.fromNumber(1),
        },
        {
          item: {
            tag: "superseded",
            value: { producerId: "workflow-1", failedTasks: [Long.fromNumber(7)] },
          },
          offset: Long.fromNumber(2),
        },
        {
          item: {
            tag: "close",
            value: { status: StreamCloseStatus.StreamCloseStatusCompleted },
          },
          offset: Long.fromNumber(3),
        },
      ],
      nextOffset: Long.fromNumber(4),
      done: true,
    };

    const proto: temporal.api.streamservice.v1.IReadRecordsResponse =
      readRecordsResponseTransferTypeConverter.toTransferType(model);

    expect(proto.records?.map((record) => record.item)).toEqual([
      "data",
      "superseded",
      "close",
    ]);
    expect(proto.records?.[1].superseded?.failedTasks?.[0].toNumber()).toBe(7);
    expect(readRecordsResponseTransferTypeConverter.fromTransferType(proto)).toEqual(
      model,
    );
  });

  test("refuses a read item without its oneof case", () => {
    const proto: temporal.api.streamservice.v1.IReadRecordsResponse = {
      records: [{ offset: Long.fromNumber(1) }],
    };

    expect(() =>
      readRecordsResponseTransferTypeConverter.fromTransferType(proto),
    ).toThrow("missing required field StreamRecord.item");
  });

  test("maps every field of a read request through its proto", () => {
    const proto: temporal.api.streamservice.v1.IReadRecordsRequest = {
      stream: { id: "stream-1", owner: { businessId: "workflow-1" } },
      fromOffset: Long.fromNumber(7),
      maxRecords: 10,
    };

    const model = readRecordsRequestTransferTypeConverter.fromTransferType(proto);

    expect(model.stream.id).toBe("stream-1");
    expect(model.stream.owner?.businessId).toBe("workflow-1");
    expect(model.fromOffset?.toNumber()).toBe(7);
    expect(model.maxRecords).toBe(10);
    expect(
      readRecordsRequestTransferTypeConverter.toTransferType(model).stream?.id,
    ).toBe("stream-1");
  });

  test("passes 64-bit values to a resource method's request as they are", async () => {
    // A Brook offset is `ms << 20 | seq`, which a `number` can't hold.
    const offset = Long.fromString("1880158843699200005");
    startOperation.mockResolvedValue({
      result: async () => ({ records: [], nextOffset: offset.add(1) }),
    });
    const stream = new Stream({ id: "stream-1" }, undefined, undefined);

    const page = await stream.read(offset);
    await stream.read(page.nextOffset);

    expect(startOperation.mock.calls.map(([, request]) => request.fromOffset)).toEqual([
      offset,
      offset.add(1),
    ]);
    expect(String(startOperation.mock.calls[1][1].fromOffset)).toBe(
      "1880158843699200006",
    );
  });

  test("decodes the bodies of a read with the handle's body type", async () => {
    startOperation.mockResolvedValue({ result: async () => ({ records: [] }) });
    const stream = new Stream({ id: "stream-1" }, undefined, undefined, turnType);

    const page = await stream.read();

    expectTypeOf(stream).toEqualTypeOf<Stream<Turn>>();
    expectTypeOf(page).toEqualTypeOf<ReadRecordsResponse<Turn>>();
    // The type info of the started operation decodes the answer with the body type.
    const [operation] = startOperation.mock.lastCall!;
    const decoded = operation.outputType.transferTypeConverter.fromTransferType({
      records: [
        {
          item: "data",
          data: { body: common.defaultPayloadConverter.toPayload({ role: "model" }) },
          offset: Long.fromNumber(1),
        },
      ],
    });
    expect(decoded.records[0].item.value.body).toStrictEqual(new Turn("model"));
  });

  test("takes the body type and the sources of a create from the definition", async () => {
    startOperation.mockResolvedValue({
      result: async () => ({ stream: { id: "chat-1:r1:turns" }, created: true }),
    });

    const stream = await createStream({ name: "turns", type: turnType }, {});

    expectTypeOf(stream).toEqualTypeOf<Stream<Turn>>();
    expect(stream.bodyType).toBe(turnType);
    // The Workflow sources give the id from the definition, and the owner from the run. The ref
    // copies the id.
    const [, request] = startOperation.mock.lastCall!;
    expect(request.id).toBe("chat-1:r1:turns");
    expect(request.owner).toEqual({ id: "wf:r2", runId: "r2" });
    expect(request.ref).toStrictEqual({ backend: undefined, id: "chat-1:r1:turns" });
  });

  test("wraps each published value, and returns the last offset", async () => {
    startOperation
      .mockResolvedValueOnce({
        result: async () => ({
          firstOffset: Long.fromNumber(4),
          lastOffset: Long.fromNumber(5),
        }),
      })
      .mockResolvedValueOnce({ result: async () => ({}) });
    const stream = new Stream<string>({ id: "s1" }, undefined, undefined);

    const lastOffset = await stream.publish("a", "b");
    const [, request] = startOperation.mock.calls.at(-1)!;
    const empty = await stream.publish();

    expectTypeOf(lastOffset).toEqualTypeOf<Long>();
    expect(lastOffset.toNumber()).toBe(5);
    expect(request.records).toStrictEqual([{ body: "a" }, { body: "b" }]);
    // `lastOffset` has no presence on the wire, so an empty publish answers zero.
    expect(empty).toBe(Long.ZERO);
  });

  test("closes a Stream at the end of `await using`, with no failure", async () => {
    startOperation.mockResolvedValueOnce({ result: async () => ({}) });
    const stream = new Stream<string>({ id: "s1" }, undefined, undefined);

    await stream[Symbol.asyncDispose]();

    const [operation, request] = startOperation.mock.lastCall!;
    expect(operation.name).toBe("CloseStream");
    expect(request.stream).toStrictEqual({ id: "s1" });
    expect(request.failure).toBeUndefined();
  });

  test("passes an enum number that it doesn't know through", () => {
    // A newer server can send a Stream status that this code doesn't know.
    const proto: temporal.api.streamservice.v1.IDescribeStreamResponse = {
      status: 9 as temporal.api.enums.v1.StreamStatus,
    };

    const model = describeStreamResponseTransferTypeConverter.fromTransferType(proto);

    expect(model.status).toBe(9);
    expect(
      describeStreamResponseTransferTypeConverter.toTransferType(model).status,
    ).toBe(9);
  });
});
