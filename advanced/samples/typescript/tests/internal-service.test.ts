import { existsSync } from "node:fs";

import { expect, test, vi } from "vitest";

import { flush } from "../wit/internal-service/operations/flush.ts";

const startOperation = vi.hoisted(() => vi.fn());
vi.mock("@temporalio/workflow", () => ({
  createNexusServiceClient: () => ({ startOperation }),
}));

test("an SDK calls an internal operation through its module", async () => {
  startOperation.mockResolvedValueOnce({});

  await flush({ writerId: "w1" });

  const [, request] = startOperation.mock.lastCall!;
  expect(request).toStrictEqual({ writerId: "w1" });
  // The package has no barrel, since only an SDK calls it.
  expect(existsSync(new URL("../wit/internal-service/index.ts", import.meta.url))).toBe(
    false,
  );
});
