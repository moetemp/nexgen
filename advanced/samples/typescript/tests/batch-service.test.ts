import { expect, expectTypeOf, test, vi } from "vitest";

import { Batch } from "../wit/batch-service/resources.ts";

const startOperation = vi.hoisted(() => vi.fn());
vi.mock("@temporalio/workflow", () => ({
  createNexusServiceClient: () => ({ startOperation }),
}));

test("wraps each added value, and returns one field of the response", async () => {
  startOperation
    .mockResolvedValueOnce({ result: async () => ({ lastItemId: "i2", count: 2 }) })
    .mockResolvedValueOnce({ result: async () => ({ count: 0 }) });
  const batch = new Batch("b1");

  const lastItemId = await batch.addItems("x", "y");
  const [, request] = startOperation.mock.calls.at(-1)!;
  const empty = await batch.addItems();

  expectTypeOf(lastItemId).toEqualTypeOf<string | undefined>();
  expect(lastItemId).toBe("i2");
  expect(request).toStrictEqual({
    batchId: "b1",
    items: [{ value: "x" }, { value: "y" }],
  });
  // A JSON field keeps its absence, so an empty add answers `undefined`.
  expect(empty).toBeUndefined();
});
