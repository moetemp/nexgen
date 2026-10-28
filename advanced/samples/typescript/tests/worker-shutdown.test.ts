import { expect, test, vi } from "vitest";

import { shutdownWorker } from "../wit/worker-shutdown/operations/shutdown-worker.ts";

const startOperation = vi.hoisted(() => vi.fn());
vi.mock("@temporalio/workflow", () => ({
  createNexusServiceClient: () => ({ startOperation }),
  workflowInfo: () => ({ namespace: "default" }),
}));

test("fills the heartbeat of a shutdown from the request", async () => {
  startOperation.mockResolvedValue({});

  await shutdownWorker({
    identity: "worker-1",
    workerInstanceKey: "key-1",
    taskQueue: "queue",
    sdkName: "temporal-typescript",
  });

  // The heartbeat takes the SDK name, and copies the worker fields of the request.
  const [, request] = startOperation.mock.lastCall!;
  expect(request).toStrictEqual({
    identity: "worker-1",
    workerInstanceKey: "key-1",
    taskQueue: "queue",
    namespace: "default",
    workerHeartbeat: {
      sdkName: "temporal-typescript",
      sdkVersion: undefined,
      workerInstanceKey: "key-1",
      workerIdentity: "worker-1",
      taskQueue: "queue",
    },
  });
});
