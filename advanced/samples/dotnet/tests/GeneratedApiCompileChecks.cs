using System;
using System.Threading.Tasks;
using Temporalio.Workflows;
using BatchServiceExample = Nexgen.BatchService;
using InternalServiceExample = Nexgen.InternalService;
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

        // An add takes the values, and answers the id of the last item, which can be absent.
        internal static Task<string?> AddItemsAsync() =>
            new BatchServiceExample.Batch("b1").AddItemsAsync("x", "y");

        // The package is internal, and the tests compile their own copy of it.
        internal static Task<InternalServiceExample.FlushResponse> FlushAsync() =>
            InternalServiceExample.Operations.FlushAsync(
                new InternalServiceExample.FlushOptions("writer-1"));

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
