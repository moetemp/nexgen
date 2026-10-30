import Long from "long";
import { describe, expect, test, vi } from "vitest";

import { Clock, Counter } from "../wit/integer-bindings/resources.ts";
import {
  clockResultTransferTypeConverter,
  nestedRequestTransferTypeConverter,
} from "../wit/integer-bindings/models.ts";
import { getClock } from "../wit/integer-bindings/operations/get-clock.ts";

const startOperation = vi.hoisted(() => vi.fn());
vi.mock("@temporalio/workflow", () => ({
  createNexusServiceClient: () => ({ startOperation }),
}));

describe("binding-aware 64-bit resource values", () => {
  test("uses Long for constructor fields and nested proto parameters", async () => {
    startOperation.mockResolvedValue({ result: async () => undefined });
    const seconds = Long.fromString("9007199254740993");
    const clock = new Clock(seconds, 42);
    await clock.touch();
    await clock.setTime(seconds.add(1), 43);

    const direct = startOperation.mock.calls[0][1];
    const nested = startOperation.mock.calls[1][1].at;
    expect(direct.seconds).toBe(seconds);
    expect(nested.seconds.toString()).toBe("9007199254740994");
    for (const model of [direct, nested]) {
      const proto = clockResultTransferTypeConverter.toTransferType(model);
      expect(
        clockResultTransferTypeConverter
          .fromTransferType(proto)
          .seconds?.equals(model.seconds),
      ).toBe(true);
    }
    const nestedProto = nestedRequestTransferTypeConverter.toTransferType({
      at: nested,
    });
    expect(nestedProto.createTime?.seconds?.toString()).toBe("9007199254740994");
  });

  test("keeps JSON-only constructor fields and parameters as numbers", async () => {
    startOperation.mockClear();
    startOperation.mockResolvedValue({ result: async () => undefined });
    const counter = new Counter(10);
    await counter.add(5);
    expect(counter.total).toBe(10);
    expect(startOperation.mock.calls[0][1]).toEqual({ total: 10, amount: 5 });
  });

  test("constructs a resource from a proto-backed operation result", async () => {
    const seconds = Long.fromString("9007199254740993");
    startOperation.mockResolvedValue({
      result: async () => ({ seconds, nanos: 42 }),
    });
    const clock = await getClock({});
    expect(clock).toBeInstanceOf(Clock);
    expect(clock.seconds?.toString()).toBe("9007199254740993");
  });
});
