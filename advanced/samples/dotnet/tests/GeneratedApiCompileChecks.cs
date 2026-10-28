using System;
using System.Threading.Tasks;
using Temporalio.Workflows;
using StartWorkflowExample = Nexgen.StartWorkflowService;
using WorkerShutdownExample = Nexgen.WorkerShutdownService;

namespace Nexgen.DotNetExamples.Tests
{

    [Workflow]
    internal class ExampleWorkflow
    {
        [WorkflowRun]
        internal Task<string> RunAsync(string input) => Task.FromResult(input);

        [WorkflowSignal]
        internal Task NotifyAsync(string message) => Task.CompletedTask;
    }

    internal static class GeneratedApiCompileChecks
    {
        internal static Task<StartWorkflowExample.StartedWorkflow> StartWorkflowAsync() =>
            StartWorkflowExample.Operations.StartWorkflowAsync(
                new StartWorkflowExample.StartWorkflowOptions(
                    nameof(ExampleWorkflow),
                    "workflow-id",
                    "task-queue")
                {
                    WorkflowStartDelay = TimeSpan.FromSeconds(1),
                });

        // The shutdown takes the SDK fields of the heartbeat. The heartbeat copies the worker
        // fields of the request.
        internal static Task<WorkerShutdownExample.ShutdownWorkerResponse> ShutdownWorkerAsync() =>
            WorkerShutdownExample.Operations.ShutdownWorkerAsync(
                new WorkerShutdownExample.ShutdownWorkerOptions("worker-1", "key-1", "queue")
                {
                    SdkName = "temporal-dotnet",
                });
    }
}
