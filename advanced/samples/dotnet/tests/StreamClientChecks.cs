extern alias Streams;

using System;
using System.Collections.Generic;
using System.Linq;
using System.Reflection;
using System.Threading;
using System.Threading.Tasks;
using Streams::Temporalio.Workflows.Client;
using Temporalio.Client;
using Xunit;
using Models = Streams::Temporalio.Workflows;
using StreamClient = Streams::Temporalio.Workflows.Client;

namespace Nexgen.DotNetExamples.Tests
{
    // The client target of the Stream service, on its System Nexus endpoint.
    public class StreamClientChecks
    {
        private const string StreamService = "temporal.api.streamservice.v1.StreamService";
        private const string SystemEndpoint = "__temporal_system";

        [Fact]
        public async Task ACreatedStreamPublishesOnTheSystemEndpoint()
        {
            var streamRef = new Models.StreamRef("s1");
            var client = FakeTemporalClient.Create(
                ("CreateStream", new Models.CreateStreamResult { Stream = streamRef, Created = true }),
                ("AppendRecords", new Models.AppendRecordsResponse { LastOffset = 0 }));
            var records = Array.Empty<Models.AppendRecord>();

            var stream = await client.CreateStreamAsync(new StreamClient.CreateStreamOptions("s1"));
            var response = await stream.PublishAsync(records);

            Assert.Equal((streamRef, (bool?)true), (stream.StreamValue, stream.Created));
            Assert.Equal(0, response.LastOffset);
            var calls = FakeTemporalClient.Calls(client);
            Assert.Equal(
                new[]
                {
                    (SystemEndpoint, StreamService, "CreateStream"),
                    (SystemEndpoint, StreamService, "AppendRecords"),
                },
                calls.Select(call => (call.Endpoint, call.Service, call.Operation)));
            Assert.Equal(new Models.CreateStreamRequest("s1"), calls[0].Input);
            Assert.Equal(new Models.AppendRecordsRequest(streamRef, records), calls[1].Input);
        }

        [Fact]
        public async Task AStoredStreamRefRebuildsTheHandle()
        {
            var streamRef = new Models.StreamRef("s2");
            var client = FakeTemporalClient.Create(
                ("DescribeStream", new Models.DescribeStreamResponse { LastOffset = 7 }));

            var response = await client.GetStream(streamRef, null).DescribeAsync();

            Assert.Equal(7, response.LastOffset);
            Assert.Equal(
                new object?[] { new Models.DescribeStreamRequest(streamRef) },
                FakeTemporalClient.Calls(client).Select(call => call.Input));
        }

        [Fact]
        public async Task AReadThatDoesNotWaitStopsAtAnEmptyPageThatDidNotMove()
        {
            var streamRef = new Models.StreamRef("s1");
            var pages = new[]
            {
                Page(nextOffset: 3, offsets: new long[] { 1, 2 }),
                Page(nextOffset: 3),
            };
            var client = FakeTemporalClient.Create(
                pages.Select(page => ("ReadRecords", (object?)page)).ToArray());

            var read = await ReadAll(
                client.ReadRecordsPagesAsync(new StreamClient.ReadRecordsOptions(streamRef) { Last = 2 }));

            Assert.Equal(pages, read);
            var calls = FakeTemporalClient.Calls(client);
            Assert.Equal(
                new object?[]
                {
                    new Models.ReadRecordsRequest(streamRef) { Last = 2 },
                    new Models.ReadRecordsRequest(streamRef) { FromOffset = 3 },
                },
                calls.Select(call => call.Input));
            // The iteration uses one Nexus client, and each page is a new operation.
            Assert.Equal(1, FakeTemporalClient.NexusClients(client));
            Assert.Equal(pages.Length, calls.Select(call => call.OperationId).Distinct().Count());
        }

        [Fact]
        public async Task AWaitingReadGoesOnAfterAnEmptyPageAtTheSameOffset()
        {
            var streamRef = new Models.StreamRef("s1");
            var wait = TimeSpan.FromSeconds(30);
            var pages = new[]
            {
                Page(nextOffset: 2, offsets: new long[] { 1 }),
                Page(nextOffset: 2),
                Page(done: true),
            };
            var client = FakeTemporalClient.Create(
                pages.Select(page => ("ReadRecords", (object?)page)).ToArray());

            var read = await ReadAll(client.ReadRecordsPagesAsync(
                new StreamClient.ReadRecordsOptions(streamRef) { FromOffset = 1, Wait = wait }));

            Assert.Equal(pages, read);
            Assert.Equal(
                new object?[]
                {
                    new Models.ReadRecordsRequest(streamRef) { FromOffset = 1, Wait = wait },
                    new Models.ReadRecordsRequest(streamRef) { FromOffset = 2, Wait = wait },
                    new Models.ReadRecordsRequest(streamRef) { FromOffset = 2, Wait = wait },
                },
                FakeTemporalClient.Calls(client).Select(call => call.Input));
        }

        [Fact]
        public async Task AStreamReadFromTheEndStaysAtTheFirstNextOffset()
        {
            var streamRef = new Models.StreamRef("s1");
            var wait = TimeSpan.FromSeconds(30);
            var pages = new[]
            {
                Page(nextOffset: 5),
                Page(nextOffset: 6, offsets: new long[] { 5 }),
                Page(done: true),
            };
            var client = FakeTemporalClient.Create(
                new[] { ("CreateStream", (object?)new Models.CreateStreamResult { Stream = streamRef }) }
                    .Concat(pages.Select(page => ("ReadRecords", (object?)page)))
                    .ToArray());
            var stream = await client.CreateStreamAsync(new StreamClient.CreateStreamOptions("s1"));

            var read = await ReadAll(stream.ReadPagesAsync(
                null, Models.StreamReadStart.StreamReadStartEnd, null, wait, null, null));

            Assert.Equal(pages, read);
            Assert.Equal(
                new object?[]
                {
                    new Models.ReadRecordsRequest(streamRef)
                    {
                        Start = Models.StreamReadStart.StreamReadStartEnd,
                        Wait = wait,
                    },
                    new Models.ReadRecordsRequest(streamRef) { FromOffset = 5, Wait = wait },
                    new Models.ReadRecordsRequest(streamRef) { FromOffset = 6, Wait = wait },
                },
                FakeTemporalClient.Calls(client).Skip(1).Select(call => call.Input));
            // The iterator uses the Nexus client that `CreateStreamAsync` gave the handle.
            Assert.Equal(1, FakeTemporalClient.NexusClients(client));
        }

        [Fact]
        public async Task ADoneFirstPageEndsTheRead()
        {
            var streamRef = new Models.StreamRef("s1");
            var pages = new[] { Page(nextOffset: 1, done: true, offsets: new long[] { 0 }) };
            var client = FakeTemporalClient.Create(("ReadRecords", pages[0]));

            var read = await ReadAll(
                client.ReadRecordsPagesAsync(new StreamClient.ReadRecordsOptions(streamRef)));

            Assert.Equal(pages, read);
            Assert.Equal(
                new object?[] { new Models.ReadRecordsRequest(streamRef) },
                FakeTemporalClient.Calls(client).Select(call => call.Input));
        }

        [Fact]
        public async Task APollPassesItsCancellationTokenToTheRead()
        {
            using var cancellation = new CancellationTokenSource();
            var streamRef = new Models.StreamRef("s1");
            var client = FakeTemporalClient.Create(
                ("ReadRecords", Page(done: true)),
                ("ReadRecords", Page(done: true)));

            await ReadAll(client.ReadRecordsPagesAsync(
                new StreamClient.ReadRecordsOptions(streamRef), cancellation.Token));
            await ReadAll(client.GetStream(streamRef, null)
                .ReadPagesAsync(null, null, null, null, null, null, cancellation.Token));

            Assert.All(
                FakeTemporalClient.Calls(client),
                call => Assert.Equal(cancellation.Token, call.CancellationToken));
        }

        [Fact]
        public void OnlyTheClientTargetHasTheReadPoll()
        {
            Assert.NotNull(typeof(Models.Operations).GetMethod(
                "ReadRecordsAsync", new[] { typeof(Models.ReadRecordsOptions) }));
            Assert.Null(typeof(Models.Operations).GetMethod("ReadRecordsPagesAsync"));
            Assert.Null(typeof(Models.Stream).GetMethod("ReadPagesAsync"));
            Assert.NotNull(typeof(StreamClient.Stream).GetMethod("ReadPagesAsync"));
        }

        private static Models.ReadRecordsResponse Page(
            long? nextOffset = null, bool? done = null, long[]? offsets = null) =>
            new((offsets ?? Array.Empty<long>())
                .Select(offset => new Models.StreamRecord { Offset = offset })
                .ToArray())
            {
                NextOffset = nextOffset,
                Done = done,
            };

        private static async Task<List<T>> ReadAll<T>(IAsyncEnumerable<T> pages)
        {
            var read = new List<T>();
            await foreach (var page in pages)
            {
                read.Add(page);
            }
            return read;
        }
    }

    internal sealed record NexusCall(
        string Endpoint,
        string Service,
        string Operation,
        object? Input,
        string? OperationId,
        CancellationToken? CancellationToken);

    // A fake SDK client. It answers each Nexus operation with the next answer of its name.
    public class FakeTemporalClient : DispatchProxy
    {
        private readonly List<NexusCall> calls = new();
        private readonly Dictionary<string, Queue<object?>> answers = new();
        private int nexusClients;

        internal static ITemporalClient Create(
            params (string Operation, object? Answer)[] answers)
        {
            var client = DispatchProxy.Create<ITemporalClient, FakeTemporalClient>();
            var fake = (FakeTemporalClient)(object)client;
            foreach (var (operation, answer) in answers)
            {
                if (!fake.answers.TryGetValue(operation, out var queue))
                {
                    queue = new Queue<object?>();
                    fake.answers[operation] = queue;
                }
                queue.Enqueue(answer);
            }
            return client;
        }

        internal static IReadOnlyList<NexusCall> Calls(ITemporalClient client) =>
            ((FakeTemporalClient)(object)client).calls;

        internal static int NexusClients(ITemporalClient client) =>
            ((FakeTemporalClient)(object)client).nexusClients;

        protected override object? Invoke(MethodInfo? targetMethod, object?[]? args)
        {
            var createsNexusClient = targetMethod is
            {
                Name: nameof(ITemporalClient.CreateNexusClient),
                IsGenericMethod: false,
            };
            if (createsNexusClient && args is { Length: 2 })
            {
                nexusClients++;
                return new FakeNexusClient(this, (string)args[0]!, (string)args[1]!);
            }
            throw new NotSupportedException(targetMethod?.Name);
        }

        private sealed class FakeNexusClient : NexusClient
        {
            private readonly FakeTemporalClient fake;

            internal FakeNexusClient(FakeTemporalClient fake, string endpoint, string service)
            {
                this.fake = fake;
                Endpoint = endpoint;
                Service = service;
            }

            public override string Service { get; }

            public override string Endpoint { get; }

            public override Task<NexusOperationHandle<TResult>> StartNexusOperationAsync<TResult>(
                string operationName, object? arg, NexusOperationOptions? options = null)
            {
                fake.calls.Add(new NexusCall(
                    Endpoint,
                    Service,
                    operationName,
                    arg,
                    options?.Id,
                    options?.Rpc?.CancellationToken));
                var answer = fake.answers[operationName].Dequeue();
                NexusOperationHandle<TResult> handle = new FakeHandle<TResult>(answer);
                return Task.FromResult(handle);
            }
        }

        private sealed record FakeHandle<TResult>(object? Answer)
            : NexusOperationHandle<TResult>(null!, "fake")
        {
            public override Task<T> GetResultAsync<T>(RpcOptions? rpcOptions = null) =>
                Task.FromResult((T)Answer!);
        }
    }
}
