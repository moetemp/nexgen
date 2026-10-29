using System.Collections.Generic;
using Temporalio.Activities;
using Temporalio.Workflows;

namespace Nexgen.Support
{
    // Sources of the stream sample, in place of those of the Streams SDK.
    internal static class StreamWorkflowContext
    {
        internal static string StreamId<T>(StreamDefinition<T> definition) =>
            $"{Workflow.Info.WorkflowId}:{Workflow.Info.FirstExecutionRunId}:{definition.Name}";

        // The Workflow's run chain, which owns its Streams: its id and its first run id.
        internal static Temporalio.Workflows.Execution HandleOwner() =>
            new()
            {
                Type = Temporalio.Workflows.ExecutionType.ExecutionTypeWorkflow,
                BusinessId = Workflow.Info.WorkflowId,
                RunId = Workflow.Info.FirstExecutionRunId,
            };

        internal static Temporalio.Workflows.Producer Owner() =>
            new($"wf:{Workflow.Info.RunId}") { RunId = Workflow.Info.RunId };

        internal static Temporalio.Workflows.Producer Producer() =>
            new($"wf:{Workflow.Info.RunId}") { RunId = Workflow.Info.RunId };
    }

    // The client sources. An Activity's attempt produces in an Activity, or else the client.
    internal static class StreamActivityContext
    {
        internal static Temporalio.Workflows.Producer Producer()
        {
            if (!ActivityExecutionContext.HasCurrent)
            {
                return new("client:sample");
            }
            var info = ActivityExecutionContext.Current.Info;
            return new($"act:{info.WorkflowRunId}:{info.ActivityId}")
            {
                RunId = info.WorkflowRunId,
            };
        }

        internal static IReadOnlyList<long> LocalActivityFailedTasks() =>
            System.Array.Empty<long>();

        internal static WorkflowTaskEpoch? LocalActivityPreviousCompletedTask() => null;
    }
}
