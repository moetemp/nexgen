using Temporalio.Activities;
using Temporalio.Workflows;

namespace Nexgen.Support
{
    // Sources of the stream sample, in place of those of the Streams SDK.
    internal static class StreamWorkflowContext
    {
        // The run's Workflow Producer.
        internal static Temporalio.Workflows.StreamWriter Writer() =>
            new()
            {
                Producer = new($"wf:{Workflow.Info.RunId}")
                {
                    Kind = new ProducerKind.Workflow(
                        new WorkflowProducer { RunId = Workflow.Info.RunId }),
                },
            };

        // A new id for each create call. A Workflow's replay gives the same id.
        internal static string RequestId() =>
            (Workflow.InWorkflow ? Workflow.NewGuid() : System.Guid.NewGuid()).ToString();
    }

    // The client sources. An Activity's attempt produces in an Activity, or else the client.
    internal static class StreamActivityContext
    {
        internal static Temporalio.Workflows.StreamWriter Writer() =>
            new() { Producer = Producer() };

        private static Temporalio.Workflows.Producer Producer()
        {
            if (!ActivityExecutionContext.HasCurrent)
            {
                return new("client:sample")
                {
                    Kind = new ProducerKind.Client(new ClientProducer()),
                };
            }
            var info = ActivityExecutionContext.Current.Info;
            return new($"act:{info.WorkflowRunId}:{info.ActivityId}")
            {
                Kind = new ProducerKind.Activity(
                    new ActivityProducer { RunId = info.WorkflowRunId }),
            };
        }
    }
}
