import Long from "long";
import { expect, test, vi } from "vitest";

import { StreamWriterClient } from "../wit/stream-writer/index.ts";
import { Writer } from "../wit/stream-writer/resources.ts";

const startOperation = vi.hoisted(() => vi.fn());
vi.mock("@temporalio/workflow", () => ({
  createNexusServiceClient: () => ({ startOperation }),
}));

test("a client with no endpoint builds the resource that an operation returns", async () => {
  // A `u64` writer id above 2^53, which a `number` can't hold.
  const writerId = Long.fromString("18446744073709551615", true);
  startOperation.mockResolvedValue({ result: async () => ({ writerId }) });

  const writer = await new StreamWriterClient("stream-writer").openWriter({
    stream: { id: "stream-1" },
  });

  expect(writer).toBeInstanceOf(Writer);
  expect(writer.writerId).toBe(writerId);
});
