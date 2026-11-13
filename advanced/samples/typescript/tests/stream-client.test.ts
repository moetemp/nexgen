import {
  type Context,
  asyncLocalStorage as activityContext,
} from "@temporalio/activity";
import * as common from "@temporalio/common";
import Long from "long";
import type * as nexus from "nexus-rpc";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

import "../support/client.ts";
import type { NexusServiceClient } from "../wit/stream-service/both/client/nexus-client.ts";
import { PollIterator } from "../wit/stream-service/both/client/poll.ts";
import type {
  AppendRecordsResponse,
  CreateStreamResult,
  DescribeStreamResponse,
  Producer,
  ReadRecordsRequest,
  ReadRecordsResponse,
  StreamRecord,
  StreamRef,
} from "../wit/stream-service/both/models.ts";

/** The `Stream` of the client target, as these tests use it. */
interface ClientStream<BodyT = unknown> {
  readonly ref: StreamRef<BodyT>;
  readonly created: boolean | undefined;
  readonly bodyType: common.TypeInfo<BodyT> | undefined;
  append(records: BodyT[], producer?: Producer): Promise<AppendRecordsResponse>;
  read(): Promise<ReadRecordsResponse<BodyT>>;
  iterRead(
    fromOffset?: Uint8Array,
    maxRecords?: number,
    wait?: common.Duration,
  ): AsyncGenerator<ReadRecordsResponse<BodyT>, void>;
  iterReadItems(
    fromOffset?: Uint8Array,
    maxRecords?: number,
    wait?: common.Duration,
  ): PollIterator<
    ReadRecordsRequest<BodyT>,
    StreamRecord<BodyT>,
    ReadRecordsResponse<BodyT>
  >;
  describe(): Promise<DescribeStreamResponse>;
  [Symbol.asyncDispose](): Promise<void>;
}

interface StreamServiceClient {
  createStream<BodyT>(
    input: { id: string; store?: string },
    bodyType?: common.TypeInfo<BodyT>,
  ): Promise<ClientStream<BodyT>>;
  getStream<BodyT>(
    ref: StreamRef<BodyT>,
    created: boolean | undefined,
  ): ClientStream<BodyT>;
}

// The tsconfig excludes the stream output, which doesn't typecheck here, so the tests load it at
// run time.
const clientModule = "../wit/stream-service/both/client/index.ts";
const { Stream, StreamServiceClientExtension } = (await import(clientModule)) as {
  Stream: abstract new (...args: never[]) => ClientStream;
  StreamServiceClientExtension: <TBase extends abstract new (...args: any[]) => object>(
    Base: TBase,
  ) => abstract new (...args: any[]) => InstanceType<TBase> & StreamServiceClient;
};

const CLIENT_PRODUCER: Producer = {
  id: "client:sample",
  kind: { tag: "client", value: {} },
};
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

// The model converters decode a body with the payload converter of the Workflow. The stub makes
// the sources see a Workflow too, so it has the info of a first attempt and a random source.
beforeEach(() => {
  vi.stubGlobal("__TEMPORAL_ACTIVATOR__", {
    payloadConverter: common.defaultPayloadConverter,
    info: { attempt: 1 },
    currentRandom: Math.random,
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

function offset(n: number): Uint8Array {
  return Uint8Array.of(n);
}

function record(n: number): StreamRecord<unknown> {
  return { body: undefined, offset: offset(n) };
}

async function readAll<T>(pages: AsyncIterable<T>): Promise<T[]> {
  const read: T[] = [];
  for await (const page of pages) {
    read.push(page);
  }
  return read;
}

describe("stream-service client target", () => {
  test("a created stream appends on the system endpoint", async () => {
    const ref: StreamRef<unknown> = { id: "s1" };
    const appended: AppendRecordsResponse = {
      firstOffset: offset(0),
      lastOffset: offset(0),
    };
    const created: CreateStreamResult<unknown> = { ref: ref, created: true };
    const client = new Client({ CreateStream: [created], AppendRecords: [appended] });

    const stream = await client.createStream({ id: "s1" });
    const response = await stream.append([]);

    expect(stream).toBeInstanceOf(Stream);
    expect([stream.ref, stream.created]).toEqual([ref, true]);
    expect(response.lastOffset).toEqual(offset(0));
    expect(client.calls).toEqual([
      {
        service: STREAM_SERVICE,
        endpoint: SYSTEM_ENDPOINT,
        operation: "CreateStream",
        // The client builds the ref from the id, and fills a new request id.
        input: { streamRef: { id: "s1" }, requestId: expect.any(String) },
      },
      {
        service: STREAM_SERVICE,
        endpoint: SYSTEM_ENDPOINT,
        operation: "AppendRecords",
        // The client fills the Producer from its default.
        input: {
          ref: ref,
          records: [],
          producer: { id: "client:sample", kind: { tag: "client", value: {} } },
        },
      },
    ]);
  });

  test("an Activity append takes the Producer of its attempt", async () => {
    const ref: StreamRef<unknown> = { id: "s1" };
    const client = new Client({ AppendRecords: [{}] });
    const info = {
      activityId: "a1",
      attempt: 2,
      currentAttemptScheduledTimestampMs: 1_500,
      workflowExecution: { workflowId: "w1", runId: "r1" },
    };

    await activityContext.run({ info } as Context, () =>
      client.getStream(ref, undefined).append([]),
    );

    expect(client.calls.map((call) => call.input)).toEqual([
      {
        ref: ref,
        records: [],
        producer: {
          id: "act:r1:a1",
          kind: {
            tag: "workflow-activity",
            value: {
              runId: "r1",
              epoch: {
                attemptScheduledTime: {
                  seconds: Long.fromNumber(1),
                  nanos: 500_000_000,
                },
                attempt: 2,
              },
            },
          },
        },
      },
    ]);
  });

  test("the end of an `await using` scope closes its stream", async () => {
    const client = new Client({
      CreateStream: [{ ref: { id: "s1" } }],
      CloseStream: [{}],
    });

    {
      await using stream = await client.createStream({ id: "s1" });
      expect(stream.ref).toEqual({ id: "s1" });
    }

    expect(client.calls.map((call) => [call.operation, call.input])).toEqual([
      ["CreateStream", expect.anything()],
      ["CloseStream", { ref: { id: "s1" }, producer: CLIENT_PRODUCER }],
    ]);
  });

  test("a failed `await using` scope closes its stream", async () => {
    const client = new Client({
      CreateStream: [{ ref: { id: "s1" } }],
      CloseStream: [{}],
    });

    await expect(async () => {
      await using _stream = await client.createStream({ id: "s1" });
      throw new Error("boom");
    }).rejects.toThrow("boom");

    expect(client.calls.map((call) => call.operation)).toEqual([
      "CreateStream",
      "CloseStream",
    ]);
  });

  test("a create takes the fields of the ref as parameters", async () => {
    const created: CreateStreamResult<unknown> = { ref: { id: "s1" } };
    const client = new Client({ CreateStream: [created] });

    await client.createStream({ id: "s1", store: "redis" });

    // The ref takes the id and the store.
    expect(client.calls.map((call) => call.input)).toEqual([
      { streamRef: { id: "s1", store: "redis" }, requestId: expect.any(String) },
    ]);
  });

  test("a stored ref rebuilds the stream", async () => {
    const ref: StreamRef<unknown> = { id: "s2" };
    const described: DescribeStreamResponse = { lastOffset: offset(7) };
    const client = new Client({ DescribeStream: [described] });

    const response = await client.getStream(ref, undefined).describe();

    expect(response.lastOffset).toEqual(offset(7));
    expect(client.calls).toEqual([
      {
        service: STREAM_SERVICE,
        endpoint: SYSTEM_ENDPOINT,
        operation: "DescribeStream",
        input: { ref: ref },
      },
    ]);
  });

  test("a read that doesn't wait stops at an empty page that didn't move", async () => {
    const ref: StreamRef<unknown> = { id: "s1" };
    const pages: ReadRecordsResponse<unknown>[] = [
      { records: [record(2), record(3)], lastOffset: offset(3), done: false },
      { records: [], lastOffset: offset(3), done: false },
    ];
    const client = new Client({ ReadRecords: [...pages] });

    const stream = client.getStream(ref, undefined);

    const read = await readAll(stream.iterRead(offset(1)));

    expect(read).toEqual(pages);
    expect(client.calls.map((call) => call.input)).toEqual([
      { ref: ref, fromOffset: offset(1) },
      { ref: ref, fromOffset: offset(3) },
    ]);
    // The iteration uses one Nexus client, and each page is a new operation.
    expect(client.nexusClients).toBe(1);
    expect(new Set(client.operationIds).size).toBe(pages.length);
  });

  test("a waiting read goes on after an empty page at the same offset", async () => {
    const ref: StreamRef<unknown> = { id: "s1" };
    const pages: ReadRecordsResponse<unknown>[] = [
      { records: [record(2)], lastOffset: offset(2), done: false },
      { records: [], lastOffset: offset(2), done: false },
      { records: [], done: true },
    ];
    const client = new Client({ ReadRecords: [...pages] });

    const stream = client.getStream(ref, undefined);

    const read = await readAll(stream.iterRead(offset(1), undefined, "30s"));

    expect(read).toEqual(pages);
    expect(client.calls.map((call) => call.input)).toMatchObject([
      { ref: ref, fromOffset: offset(1), wait: "30s" },
      { ref: ref, fromOffset: offset(2), wait: "30s" },
      { ref: ref, fromOffset: offset(2), wait: "30s" },
    ]);
  });

  test("a page that covers no Record reads again from the same offset", async () => {
    const ref: StreamRef<unknown> = { id: "s1" };
    const pages: ReadRecordsResponse<unknown>[] = [
      { records: [], done: false },
      { records: [record(1)], lastOffset: offset(1), done: true },
    ];
    const client = new Client({
      CreateStream: [{ ref: ref }],
      ReadRecords: [...pages],
    });
    const stream = await client.createStream({ id: "s1" });

    const read = await readAll(stream.iterRead(undefined, undefined, "30s"));

    expect(read).toEqual(pages);
    expect(client.calls.slice(1).map((call) => call.input)).toMatchObject([
      { ref: ref, wait: "30s" },
      { ref: ref, wait: "30s" },
    ]);
    // The iterator uses the Nexus client that `createStream` gave the handle.
    expect(client.nexusClients).toBe(1);
  });

  test("a done first page ends the read", async () => {
    const ref: StreamRef<unknown> = { id: "s1" };
    const pages: ReadRecordsResponse<unknown>[] = [
      { records: [record(1)], lastOffset: offset(1), done: true },
    ];
    const client = new Client({ ReadRecords: [...pages] });

    const stream = client.getStream(ref, undefined);

    const read = await readAll(stream.iterRead());

    expect(read).toEqual(pages);
    expect(client.calls.map((call) => call.input)).toEqual([{ ref: ref }]);
  });

  test("a changed page doesn't change the next request", async () => {
    const ref: StreamRef<unknown> = { id: "s1" };
    const client = new Client({
      ReadRecords: [
        { records: [record(2)], lastOffset: offset(2), done: false },
        { records: [], done: true },
      ],
    });

    const stream = client.getStream(ref, undefined);

    for await (const page of stream.iterRead(offset(1), undefined, "30s")) {
      page.lastOffset = offset(9);
    }

    expect(client.calls.map((call) => call.input)).toMatchObject([
      { ref: ref, fromOffset: offset(1), wait: "30s" },
      { ref: ref, fromOffset: offset(2), wait: "30s" },
    ]);
  });

  test("the item iterator yields each Record and resumes after it", async () => {
    const ref: StreamRef<unknown> = { id: "s1" };
    const client = new Client({
      ReadRecords: [
        { records: [record(10), record(12)], lastOffset: offset(12), done: false },
        { records: [], lastOffset: offset(12), done: false },
        { records: [record(20)], lastOffset: offset(20), done: true },
      ],
    });
    const stream = client.getStream(ref, undefined);

    const read = stream.iterReadItems(offset(10), undefined, "30s");
    const seen: [Uint8Array, Uint8Array | undefined][] = [];
    for await (const item of read) {
      seen.push([item.offset, read.nextRequest?.fromOffset]);
    }

    // A from-offset is exclusive, so after a Record the read resumes after that Record's own
    // offset. After a page's last Record, it goes on after the page's newest offset.
    expect(seen).toEqual([
      [offset(10), offset(10)],
      [offset(12), offset(12)],
      [offset(20), undefined],
    ]);
    expect(read.nextRequest).toBeUndefined();
    expect(client.calls.map((call) => call.input)).toMatchObject([
      { ref: ref, fromOffset: offset(10), wait: "30s" },
      { ref: ref, fromOffset: offset(12), wait: "30s" },
      { ref: ref, fromOffset: offset(12), wait: "30s" },
    ]);
  });

  test("a new read resumes from the next request of a stopped read", async () => {
    const ref: StreamRef<unknown> = { id: "s1" };
    const client = new Client({
      ReadRecords: [
        { records: [record(1), record(2)], lastOffset: offset(2), done: false },
        { records: [record(2)], lastOffset: offset(2), done: true },
      ],
    });
    const stream = client.getStream(ref, undefined);

    const read = stream.iterReadItems(offset(1));
    const first = await read.next();
    const saved = read.nextRequest;

    expect(first.value?.offset).toEqual(offset(1));
    const resumed: Uint8Array[] = [];
    for await (const item of stream.iterReadItems(saved?.fromOffset)) {
      resumed.push(item.offset);
    }
    expect(resumed).toEqual([offset(2)]);
    expect(client.calls.map((call) => call.input)).toEqual([
      { ref: ref, fromOffset: offset(1) },
      { ref: ref, fromOffset: offset(1) },
    ]);
  });

  test("a changed Record doesn't change the next request", async () => {
    const ref: StreamRef<unknown> = { id: "s1" };
    const client = new Client({
      ReadRecords: [
        { records: [record(1), record(2)], lastOffset: offset(2), done: false },
        { records: [], done: true },
      ],
    });
    const stream = client.getStream(ref, undefined);

    const read = stream.iterReadItems(offset(1), undefined, "30s");
    const nextOffsets: (Uint8Array | undefined)[] = [];
    for await (const item of read) {
      item.offset = offset(9);
      nextOffsets.push(read.nextRequest?.fromOffset);
    }

    expect(nextOffsets).toEqual([offset(1), offset(2)]);
    expect(client.calls.map((call) => call.input)).toMatchObject([
      { ref: ref, fromOffset: offset(1), wait: "30s" },
      { ref: ref, fromOffset: offset(2), wait: "30s" },
    ]);
  });

  test("without a resume helper the next request reads the page again", async () => {
    async function* pages(): AsyncGenerator<number[], void> {
      yield [1, 2];
      yield [3];
    }
    const read = new PollIterator(
      "first",
      pages(),
      (page) => page,
      (request) => (request === "first" ? "second" : undefined),
    );

    const seen: [number, string | undefined][] = [];
    for await (const item of read) {
      seen.push([item, read.nextRequest]);
    }

    expect(seen).toEqual([
      [1, "first"],
      [2, "second"],
      [3, undefined],
    ]);
  });

  test("a handle decodes the bodies of a read with its body type", async () => {
    const client = new Client({
      CreateStream: [{ ref: { id: "s1" } }],
      ReadRecords: [{ records: [], done: true }],
    });
    const stream = await client.createStream({ id: "s1" }, turnType);

    await stream.read();

    expect(stream.bodyType).toBe(turnType);
    // The type info of the started operation decodes the answer with the body type.
    const decoded = client.outputTypes[1]?.transferTypeConverter?.fromTransferType({
      records: [
        {
          body: common.defaultPayloadConverter.toPayload({ role: "model" }),
          offset: offset(1),
        },
      ],
      done: true,
    }) as ReadRecordsResponse<Turn>;
    expect(decoded.records[0].body).toEqual(new Turn("model"));
  });

  test("a client stream converts as its fields", async () => {
    const client = new Client({
      CreateStream: [{ ref: { id: "s1" }, created: false }],
    });
    const stream = await client.createStream({ id: "s1" }, turnType);
    const converter = common.defaultPayloadConverter;

    // The Nexus client and the body type aren't data.
    expect(converter.fromPayload(converter.toPayload(stream))).toStrictEqual({
      ref: { id: "s1" },
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
