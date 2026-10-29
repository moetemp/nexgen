// Sources of the stream sample, in place of those of the Streams SDK.

import type { Info as ActivityInfo } from "@temporalio/activity";
import * as workflow from "@temporalio/workflow";

import type { StreamWriter } from "../wit/stream-service/both/models";

// Workflow code imports this module too, so it can't load `@temporalio/activity`. A client-only
// module sets the provider, as `client.ts` does.
let activityInfoProvider: (() => ActivityInfo | undefined) | undefined;

/** Sets how a client source finds the Activity that it runs in. */
export function setActivityInfoProvider(
  provider: () => ActivityInfo | undefined,
): void {
  activityInfoProvider = provider;
}

/** The run's Workflow Producer. */
export function workflowWriter(): StreamWriter {
  const runId = workflow.workflowInfo().runId;
  return {
    producer: { id: `wf:${runId}`, kind: { tag: "workflow", value: { runId } } },
  };
}

/** A new id for each create call. A Workflow's replay gives the same id. */
export function requestId(): string {
  return workflow.inWorkflowContext() ? workflow.uuid4() : crypto.randomUUID();
}

/** An Activity's attempt, or a client. */
export function clientWriter(): StreamWriter {
  const info = activityInfoProvider?.();
  if (info === undefined) {
    return {
      producer: { id: "client:sample", kind: { tag: "client", value: {} } },
    };
  }
  const runId = info.workflowExecution?.runId;
  return {
    producer: {
      id: `act:${runId}:${info.activityId}`,
      kind: { tag: "activity", value: { runId } },
    },
  };
}
