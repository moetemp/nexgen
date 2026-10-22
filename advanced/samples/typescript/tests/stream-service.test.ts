import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";
import * as nexus from "nexus-rpc";

import { Stream } from "../wit/stream-service/resources.ts";
import { streamService } from "../wit/stream-service/services.ts";
import { executeWorkflowWithNexus, withWorkflowEnvironment } from "./helpers.ts";

const workflowsPath = fileURLToPath(
  new URL("./workflows/stream-service.ts", import.meta.url),
);

describe("stream-service generated output", () => {
  test("builds the stream class from the create result", async () => {
    await withWorkflowEnvironment(async (env) => {
      const handler = nexus.serviceHandler(streamService, {
        async createStream(_ctx, input) {
          return { stream: new Stream(input.ref), created: true };
        },
        async appendRecords() {
          return { firstOffset: 0, lastOffset: 0 };
        },
        async closeStream() {
          return {};
        },
        async readRecords() {
          return { records: [] };
        },
        async describeStream() {
          return { status: 0 };
        },
      });

      const result = await executeWorkflowWithNexus<{
        created: boolean;
        isStream: boolean;
        streamId: string;
      }>(env, {
        endpoint: "streams",
        nexusServices: [handler],
        workflowType: "streamServiceCaller",
        workflowsPath,
      });

      expect(result).toEqual({ created: true, isStream: true, streamId: "stream-1" });
    });
  });
});
