import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";
import * as nexus from "nexus-rpc";

import type {
  RestartRequest,
  SubmitRequest,
} from "../wit/target-sources/workflow/models.ts";
import { sourceService } from "../wit/target-sources/workflow/services.ts";
import { executeWorkflowWithNexus, withWorkflowEnvironment } from "./helpers.ts";

const workflowsPath = fileURLToPath(
  new URL("./workflows/target-sources.ts", import.meta.url),
);

describe("target-sources generated output", () => {
  test("supplies the sources for the workflow target and takes the client-only one", async () => {
    await withWorkflowEnvironment(async (env) => {
      const submits: SubmitRequest[] = [];
      const restarts: RestartRequest[] = [];
      const unused = async (): Promise<never> => {
        throw new Error("the caller does not use this operation");
      };
      const handler = nexus.serviceHandler(sourceService, {
        async submit(_ctx, input) {
          submits.push(input);
        },
        async restart(_ctx, input) {
          restarts.push(input);
        },
        createJob: unused,
        refresh: unused,
        createMarker: unused,
        createRun: unused,
        createRunWithStatus: unused,
        cancelRun: unused,
      });

      await executeWorkflowWithNexus<void>(env, {
        endpoint: "sources",
        nexusServices: [handler],
        workflowType: "targetSourcesCaller",
        workflowsPath,
      });

      expect(submits).toEqual([
        { id: "id", client: "supplied", shared: "shared", workflow: "workflow" },
      ]);
      expect(restarts).toEqual([{ jobId: "job", shared: "shared" }]);
    });
  });
});
