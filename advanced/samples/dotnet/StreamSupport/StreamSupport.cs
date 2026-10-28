using Temporalio.Workflows;

namespace Nexgen.Support
{
    // Sources of the stream sample. They stand in for the ones of Brook's runtime.
    internal static class StreamWorkflowContext
    {
        internal static string StreamId<T>(StreamDefinition<T> definition) =>
            $"{Workflow.Info.WorkflowId}:{Workflow.Info.FirstExecutionRunId}:{definition.Name}";

        internal static Nexgen.StreamService.Producer Owner() =>
            new($"wf:{Workflow.Info.RunId}") { RunId = Workflow.Info.RunId };

        internal static Nexgen.StreamService.Producer Producer() =>
            new($"wf:{Workflow.Info.RunId}") { RunId = Workflow.Info.RunId };
    }
}
