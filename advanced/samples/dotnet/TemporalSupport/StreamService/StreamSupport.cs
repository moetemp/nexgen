using Temporalio.Activities;
using Temporalio.Workflows;

namespace Nexgen.Support
{
    // Sources of the stream sample, in place of those of the Streams SDK.
    internal static class StreamWorkflowContext
    {
        // The run's Workflow Producer, with the run that a retry run retries.
        internal static Temporalio.Workflows.StreamWriter Writer() =>
            new(System.Array.Empty<long>())
            {
                Producer = new($"wf:{Workflow.Info.RunId}") { RunId = Workflow.Info.RunId },
                WithdrawRun = WithdrawRun(),
            };

        // The run that a retry run's Workflow retries. Null elsewhere.
        internal static string? WithdrawRun() =>
            Workflow.InWorkflow && Workflow.Info.Attempt > 1 ? Workflow.Info.ContinuedRunId : null;

        // A new id for each create call. A Workflow's replay gives the same id.
        internal static string RequestId() =>
            (Workflow.InWorkflow ? Workflow.NewGuid() : System.Guid.NewGuid()).ToString();
    }

    // The client sources. An Activity's attempt produces in an Activity, or else the client.
    internal static class StreamActivityContext
    {
        // The sample has no Local Activity context, so a client writer withdraws nothing.
        internal static Temporalio.Workflows.StreamWriter Writer() =>
            new(System.Array.Empty<long>()) { Producer = Producer() };

        private static Temporalio.Workflows.Producer Producer()
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
    }
}
