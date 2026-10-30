using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using NexusRpc;
using NexusRpc.Handlers;
using Temporalio.Client;
using Temporalio.Testing;
using Temporalio.Worker;
using Temporalio.Workflows;
using Sources = Nexgen.SourceService;
using Xunit;

namespace Nexgen.DotNetExamples.Tests
{
    public class TargetSourcesChecks
    {
        [Workflow]
        public class TargetSourcesCaller
        {
            [WorkflowRun]
            public async Task RunAsync()
            {
                // The options have a parameter for the client-only source and none for the others.
                await Sources.Operations.SubmitAsync(new Sources.SubmitOptions("id", "supplied"));
                await new Sources.Job("job").RestartAsync();
            }
        }

        [Fact]
        public async Task ClientOnlySourceIsAParameterAndTheOtherSourcesAreSupplied()
        {
            await using var env = await WorkflowEnvironment.StartLocalAsync(new());
            var taskQueue = Guid.NewGuid().ToString();
            var submits = new List<Sources.SubmitRequest>();
            var restarts = new List<Sources.RestartRequest>();
            var definition = new ServiceDefinition("SourceService", new[]
            {
                new OperationDefinition("Submit", typeof(Sources.SubmitRequest), typeof(void)),
                new OperationDefinition("Restart", typeof(Sources.RestartRequest), typeof(void)),
            });
            var handlers = new Dictionary<string, IOperationHandler<object?, object?>>
            {
                ["Submit"] = OperationHandler.WrapAsGenericHandler(
                    OperationHandler.Sync<Sources.SubmitRequest, NoValue>((_ctx, input) =>
                    {
                        submits.Add(input);
                        return default(NoValue);
                    })),
                ["Restart"] = OperationHandler.WrapAsGenericHandler(
                    OperationHandler.Sync<Sources.RestartRequest, NoValue>((_ctx, input) =>
                    {
                        restarts.Add(input);
                        return default(NoValue);
                    })),
            };
            using var worker = new TemporalWorker(env.Client, new TemporalWorkerOptions(taskQueue)
                .AddWorkflow<TargetSourcesCaller>()
                .AddNexusService(new ServiceHandlerInstance(definition, handlers)));
            await worker.ExecuteAsync(async () =>
            {
                var endpoint = await env.CreateNexusEndpointAsync("sources", taskQueue);
                try
                {
                    await env.Client.ExecuteWorkflowAsync(
                        (TargetSourcesCaller workflow) => workflow.RunAsync(),
                        new WorkflowOptions(Guid.NewGuid().ToString(), taskQueue));
                }
                finally
                {
                    await env.DeleteNexusEndpointAsync(endpoint);
                }
            }, CancellationToken.None);

            var submit = Assert.Single(submits);
            Assert.Equal("id", submit.Id);
            Assert.Equal("supplied", submit.Client);
            Assert.Equal("shared", submit.Shared);
            Assert.Equal("workflow", submit.Workflow);
            var restart = Assert.Single(restarts);
            Assert.Equal("job", restart.JobId);
            Assert.Equal("shared", restart.Shared);
        }
    }
}
