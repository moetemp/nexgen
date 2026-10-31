// Sources of the stream sample, in place of those of the Streams SDK.

import type { Info as ActivityInfo } from "@temporalio/activity";
import * as workflow from "@temporalio/workflow";
import type Long from "long";

import {
  type Execution,
  ExecutionType,
  type Producer,
  type WorkflowTaskEpoch,
} from "../wit/stream-service/both/models";
import type { StreamDefinition } from "./temporal_model_converters";

// Workflow code imports this module too, so it can't load `@temporalio/activity`. A client-only
// module sets the provider, as `client.ts` does.
let activityInfoProvider: (() => ActivityInfo | undefined) | undefined;

/** Sets how a client source finds the Activity that it runs in. */
export function setActivityInfoProvider(
  provider: () => ActivityInfo | undefined,
): void {
  activityInfoProvider = provider;
}

/** The id of the Workflow's own Stream of this definition. */
export function workflowStreamId(definition: StreamDefinition<unknown>): string {
  const info = workflow.workflowInfo();
  return `${info.workflowId}:${info.firstExecutionRunId}:${definition.name}`;
}

/** The Workflow's run chain, which owns its Streams: its id and its first run id. */
export function workflowHandleOwner(): Execution {
  const info = workflow.workflowInfo();
  return {
    type: ExecutionType.ExecutionTypeWorkflow,
    businessId: info.workflowId,
    runId: info.firstExecutionRunId,
  };
}

export function workflowOwner(): Producer {
  const runId = workflow.workflowInfo().runId;
  return { id: `wf:${runId}`, runId };
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
