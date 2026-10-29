import * as common from "@temporalio/common";
import Long from "long";
import type * as nexus from "nexus-rpc";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

import {
  type NexusServiceClient,
  Stream,
  StreamServiceClientExtension,
} from "../wit/stream-service/both/client/index.ts";
import {
  type AppendRecordsResponse,
  type CreateStreamResult,
  type DescribeStreamResponse,
  type ReadRecordsResponse,
  StreamReadStart,
  type StreamRef,
} from "../wit/stream-service/both/models.ts";

const STREAM_SERVICE = "temporal.api.streamservice.v1.StreamService";
const SYSTEM_ENDPOINT = "__temporal_system";

interface Call {
  service: string;
  endpoint: string;
  operation: string;
  input: unknown;
}

/** A fake SDK client that extends the extension of the Stream service. */
class Client extends StreamServiceClientExtension(Object) {
  public readonly calls: Call[] = [];
  public readonly operationIds: string[] = [];
  public readonly outputTypes: (nexus.TypeInfo<unknown, unknown> | undefined)[] = [];
  public nexusClients = 0;

  public constructor(private readonly answers: Record<string, unknown[]>) {
    super();
  }

  public createNexusClient<T extends nexus.ServiceDefinition>(options: {
    service: T;
    endpoint: string;
  }): NexusServiceClient<T> {
    this.nexusClients += 1;
    return {
      startOperation: async (operation, input, { id }) => {
        this.calls.push({
          service: options.service.name,
          endpoint: options.endpoint,
          operation: operation.name,
          input,
        });
        this.operationIds.push(id);
        this.outputTypes.push(operation.outputType);
        const answer = this.answers[operation.name].shift();
        return { result: async () => answer as never };
      },
    };
  }
}

// The model converters decode a body with the payload converter of the Workflow.
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

async function readAll<T>(pages: AsyncIterable<T>): Promise<T[]> {
  const read: T[] = [];
  for await (const page of pages) {
    read.push(page);
  }
  return read;
}

describe("stream-service client target", () => {
  test("a created stream publishes on the system endpoint", async () => {
    const ref: StreamRef = { id: "s1" };
    const appended: AppendRecordsResponse = {
      firstOffset: Long.fromNumber(0),
      lastOffset: Long.fromNumber(0),
    };
    const created: CreateStreamResult = { stream: ref, created: true };
    const client = new Client({ CreateStream: [created], AppendRecords: [appended] });

    const stream = await client.createStream({ id: "s1" });
    const response = await stream.publish([]);

    expect(stream).toBeInstanceOf(Stream);
    expect([stream.stream, stream.created]).toEqual([ref, true]);
    expect(response.lastOffset?.toNumber()).toBe(0);
    expect(client.calls).toEqual([
      {
        service: STREAM_SERVICE,
        endpoint: SYSTEM_ENDPOINT,
        operation: "CreateStream",
        input: { id: "s1" },
      },
      {
        service: STREAM_SERVICE,
        endpoint: SYSTEM_ENDPOINT,
        operation: "AppendRecords",
        input: { stream: ref, records: [] },
      },
    ]);
  });

  test("a stored stream ref rebuilds the handle", async () => {
    const ref: StreamRef = { id: "s2" };
    const described: DescribeStreamResponse = { lastOffset: Long.fromNumber(7) };
    const client = new Client({ DescribeStream: [described] });

    const response = await client.getStream(ref, undefined).describe();

    expect(response.lastOffset?.toNumber()).toBe(7);
    expect(client.calls).toEqual([
      {
        service: STREAM_SERVICE,
        endpoint: SYSTEM_ENDPOINT,
        operation: "DescribeStream",
        input: { stream: ref },
      },
    ]);
  });

  test("a read that doesn't wait stops at an empty page that didn't move", async () => {
    const ref: StreamRef = { id: "s1" };
    const pages: ReadRecordsResponse<unknown>[] = [
      {
        records: [{ offset: Long.fromNumber(1) }, { offset: Long.fromNumber(2) }],
        nextOffset: Long.fromNumber(3),
      },
      { nextOffset: Long.fromNumber(3) },
    ];
    const client = new Client({ ReadRecords: [...pages] });

    const read = await readAll(client.iterReadRecords({ stream: ref, last: 2 }));

    expect(read).toEqual(pages);
    expect(client.calls.map((call) => call.input)).toEqual([
      { stream: ref, last: 2 },
      {
        stream: ref,
        fromOffset: Long.fromNumber(3),
        start: undefined,
        last: undefined,
      },
    ]);
    // The iteration uses one Nexus client, and each page is a new operation.
    expect(client.nexusClients).toBe(1);
    expect(new Set(client.operationIds).size).toBe(pages.length);
  });

  test("a waiting read goes on after an empty page at the same offset", async () => {
    const ref: StreamRef = { id: "s1" };
    const pages: ReadRecordsResponse<unknown>[] = [
      { records: [{ offset: Long.fromNumber(1) }], nextOffset: Long.fromNumber(2) },
      { nextOffset: Long.fromNumber(2) },
      { done: true },
    ];
    const client = new Client({ ReadRecords: [...pages] });

    const read = await readAll(
      client.iterReadRecords({
        stream: ref,
        fromOffset: Long.fromNumber(1),
        wait: "30s",
      }),
    );

    expect(read).toEqual(pages);
    expect(client.calls.map((call) => call.input)).toMatchObject([
      { stream: ref, fromOffset: Long.fromNumber(1), wait: "30s" },
      { stream: ref, fromOffset: Long.fromNumber(2), wait: "30s" },
      { stream: ref, fromOffset: Long.fromNumber(2), wait: "30s" },
    ]);
  });

  test("a stream read from the end stays at the first next offset", async () => {
    const ref: StreamRef = { id: "s1" };
    const pages: ReadRecordsResponse<unknown>[] = [
      { nextOffset: Long.fromNumber(5) },
      { records: [{ offset: Long.fromNumber(5) }], nextOffset: Long.fromNumber(6) },
      { done: true },
    ];
    const client = new Client({
      CreateStream: [{ stream: ref }],
      ReadRecords: [...pages],
    });
    const stream = await client.createStream({ id: "s1" });

    const read = await readAll(
      stream.iterRead(undefined, StreamReadStart.StreamReadStartEnd, undefined, "30s"),
    );

    expect(read).toEqual(pages);
    expect(client.calls.slice(1).map((call) => call.input)).toMatchObject([
      { stream: ref, start: StreamReadStart.StreamReadStartEnd, wait: "30s" },
      { stream: ref, fromOffset: Long.fromNumber(5), start: undefined, wait: "30s" },
      { stream: ref, fromOffset: Long.fromNumber(6), start: undefined, wait: "30s" },
    ]);
    // The iterator uses the Nexus client that `createStream` gave the handle.
    expect(client.nexusClients).toBe(1);
  });

  test("a done first page ends the read", async () => {
    const ref: StreamRef = { id: "s1" };
    const pages: ReadRecordsResponse<unknown>[] = [
      {
        records: [{ offset: Long.fromNumber(0) }],
        nextOffset: Long.fromNumber(1),
        done: true,
      },
    ];
    const client = new Client({ ReadRecords: [...pages] });

    const read = await readAll(client.iterReadRecords({ stream: ref }));

    expect(read).toEqual(pages);
    expect(client.calls.map((call) => call.input)).toEqual([{ stream: ref }]);
  });

  test("a changed page doesn't change the next request", async () => {
    const ref: StreamRef = { id: "s1" };
    const client = new Client({
      ReadRecords: [
        { records: [{ offset: Long.fromNumber(1) }], nextOffset: Long.fromNumber(2) },
        { done: true },
      ],
    });

    for await (const page of client.iterReadRecords({
      stream: ref,
      fromOffset: Long.fromNumber(1),
      wait: "30s",
    })) {
      page.nextOffset = Long.fromNumber(9);
    }

    expect(client.calls.map((call) => call.input)).toMatchObject([
      { stream: ref, fromOffset: Long.fromNumber(1), wait: "30s" },
      { stream: ref, fromOffset: Long.fromNumber(2), wait: "30s" },
    ]);
  });

  test("a handle decodes the bodies of a read with its body type", async () => {
    const client = new Client({
      CreateStream: [{ stream: { id: "s1" } }],
      ReadRecords: [{ done: true }],
    });
    const stream = await client.createStream({ id: "s1" }, turnType);

    await stream.read();

    expect(stream.bodyType).toBe(turnType);
    // The type info of the started operation decodes the answer with the body type.
    const decoded = client.outputTypes[1]?.transferTypeConverter?.fromTransferType({
      records: [
        {
          item: "data",
          data: { body: common.defaultPayloadConverter.toPayload({ role: "model" }) },
          offset: Long.fromNumber(1),
        },
      ],
    }) as ReadRecordsResponse<Turn>;
    expect(decoded.records?.[0].item).toEqual({
      tag: "data",
      value: { body: new Turn("model") },
    });
  });

  test("a client stream converts as its fields", async () => {
    const client = new Client({
      CreateStream: [{ stream: { id: "s1" }, created: false }],
    });
    const stream = await client.createStream({ id: "s1" }, turnType);
    const converter = common.defaultPayloadConverter;

    // The Nexus client and the body type aren't data.
    expect(converter.fromPayload(converter.toPayload(stream))).toStrictEqual({
      stream: { id: "s1" },
      created: false,
    });
  });

  test("a client class can't get an operation from two bases", () => {
    class Base {
      public createStream(): void {}
    }

    expect(() => StreamServiceClientExtension(Base)).toThrow(
      "Base and StreamServiceClientExtension both have createStream",
    );
  });

  test("a client class can't get an operation from a field of its base", () => {
    class Base {
      public readonly createStream = "a field";
    }
    class FieldClient extends StreamServiceClientExtension(Base) {
      public createNexusClient(): never {
        throw new Error("not called");
      }
    }

    expect(() => new FieldClient()).toThrow(
      "Base and StreamServiceClientExtension both have createStream",
    );
  });
});
