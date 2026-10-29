import "./setup-protos.ts";
import type { Client } from "@temporalio/client";
import * as common from "@temporalio/common";
import type { temporal } from "@temporalio/proto";
import Long from "long";
import type * as nexus from "nexus-rpc";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

import type { NexusServiceClient } from "../wit/stream-service/both/client/nexus-client.ts";

import {
  ExecutionType,
  type ReadRecordsResponse,
  StreamCloseStatus,
  StreamStatus,
  describeStreamResponseTransferTypeConverter,
  readRecordsRequestTransferTypeConverter,
  readRecordsResponseTransferTypeConverter,
} from "../wit/stream-service/both/models.ts";
import { createStream } from "../wit/stream-service/both/operations/create-stream.ts";
import { Stream } from "../wit/stream-service/both/resources.ts";

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

describe("stream-service generated output", () => {
  beforeEach(() => {
    startOperation.mockReset();
  });

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

  test("gives no item for a read record whose item kind only a newer server knows", () => {
    const proto: temporal.api.streamservice.v1.IReadRecordsResponse = {
      records: [{ offset: Long.fromNumber(1) }],
    };

    const model = readRecordsResponseTransferTypeConverter.fromTransferType(proto);

    expect(model.records?.[0].item).toBeUndefined();
    expect(model.records?.[0].offset?.toNumber()).toBe(1);
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

  test("decodes an enum number that it doesn't know as the fallback case", () => {
    // A newer server can send a Stream status that this code doesn't know.
    const proto: temporal.api.streamservice.v1.IDescribeStreamResponse = {
      status: 9 as temporal.api.enums.v1.StreamStatus,
    };

    const model = describeStreamResponseTransferTypeConverter.fromTransferType(proto);

    expect(model.status).toBe(StreamStatus.StreamStatusUnspecified);
  });

  test("a Workflow create names the run chain that owns the stream", async () => {
    startOperation.mockResolvedValue({
      result: async () => ({ stream: { id: "chat-1:r1:turns" } }),
    });

    await createStream({ name: "turns" }, { backend: "redis" });

    const [, request] = startOperation.mock.calls[0];
    expect(request.ref).toEqual({
      backend: "redis",
      id: "chat-1:r1:turns",
      owner: {
        type: ExecutionType.ExecutionTypeWorkflow,
        businessId: "chat-1",
        runId: "r1",
      },
    });
  });

  test("preserves offsets above 2^53 through resource calls and proto conversion", async () => {
    const offset = Long.fromString("1880158843699200005");
    startOperation.mockResolvedValue({
      result: async () => ({ records: [], nextOffset: offset.add(1) }),
    });
    const stream = new Stream({ id: "stream-1" }, undefined);

    const page = await stream.read(offset);
    await stream.read(page.nextOffset);

    const requests = startOperation.mock.calls.map(([, request]) => request);
    expect(requests.map((request) => request.fromOffset.toString())).toEqual([
      "1880158843699200005",
      "1880158843699200006",
    ]);
    for (const request of requests) {
      const proto = readRecordsRequestTransferTypeConverter.toTransferType(request);
      const model = readRecordsRequestTransferTypeConverter.fromTransferType(proto);
      expect(model.fromOffset?.equals(request.fromOffset)).toBe(true);
    }
  });

  test("the SDK's Nexus service client is the client target's Nexus client", () => {
    // Only `tsc` checks this, on the return type.
    function nexusClient<T extends nexus.ServiceDefinition>(
      client: Client,
      options: { service: T; endpoint: string },
    ): NexusServiceClient<T> {
      return client.nexus.createServiceClient(options);
    }

    expect(nexusClient).toBeTypeOf("function");
  });
});
