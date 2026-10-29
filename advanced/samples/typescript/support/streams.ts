// Sources of the stream sample, in place of those of the Streams SDK.

import type { Info as ActivityInfo } from "@temporalio/activity";
import * as workflow from "@temporalio/workflow";
import type Long from "long";

import type { Producer, WorkflowTaskEpoch } from "../wit/stream-service/both/models";

// Workflow code imports this module too, so it can't load `@temporalio/activity`. A client-only
// module sets the provider, as `client.ts` does.
let activityInfoProvider: (() => ActivityInfo | undefined) | undefined;

/** Sets how a client source finds the Activity that it runs in. */
export function setActivityInfoProvider(
  provider: () => ActivityInfo | undefined,
): void {
  activityInfoProvider = provider;
}

export function workflowProducer(): Producer {
  const runId = workflow.workflowInfo().runId;
  return { id: `wf:${runId}`, runId };
}

/** The run that a retry run's Workflow retries. Undefined elsewhere. */
export function withdrawRun(): string | undefined {
  if (!workflow.inWorkflowContext()) {
    return undefined;
  }
  const info = workflow.workflowInfo();
  return info.attempt > 1 ? info.continuedFromExecutionRunId : undefined;
}

/** A new id for each create call. A Workflow's replay gives the same id. */
export function requestId(): string {
  return workflow.inWorkflowContext() ? workflow.uuid4() : crypto.randomUUID();
}

/** An Activity's attempt, or a client. */
export function clientProducer(): Producer {
  const info = activityInfoProvider?.();
  if (info === undefined) {
    return { id: "client:sample" };
  }
  const runId = info.workflowExecution?.runId;
  return { id: `act:${runId}:${info.activityId}`, runId };
}

export function localActivityFailedTasks(): Long[] {
  return [];
}

export function localActivityPreviousCompletedTask(): WorkflowTaskEpoch | undefined {
  return undefined;
}
