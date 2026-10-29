extern alias Streams;

using System.Linq;
using System.Threading.Tasks;
using Temporalio.Converters;
using Xunit;
using static Nexgen.DotNetExamples.Tests.TransferTypeTestSupport;
using StreamServiceExample = Streams::Temporalio.Workflows;
using WireDescribeStreamResponse = Streams::Temporalio.Api.StreamService.V1.DescribeStreamResponse;
using WireReadRecordsResponse = Streams::Temporalio.Api.StreamService.V1.ReadRecordsResponse;
using WireStreamRecord = Streams::Temporalio.Api.Stream.V1.StreamRecord;

namespace Nexgen.DotNetExamples.Tests
{
    public class StreamServiceChecks
    {
        public sealed record Turn(string Role);

        [Fact]
        public void StreamWithItsOwnFieldNameRoundTrips()
        {
            var converter = DataConverter.Default.PayloadConverter;
            var stream = new StreamServiceExample.Stream<Turn>(
                new StreamServiceExample.StreamRef("stream-1"), null);

            var decoded = converter.ToValue<StreamServiceExample.Stream<Turn>>(
                converter.ToPayload(stream));

            Assert.Equal("stream-1", decoded.StreamValue.Id);
        }

        // A data item's body goes through the payload converter of the sample assembly, which a
        // test can't set. The TypeScript test covers data items.
        [Fact]
        public void ReadItemsRoundTripAsTheirRecords()
        {
            var model = new StreamServiceExample.ReadRecordsResponse<Turn>(new[]
            {
                new StreamServiceExample.StreamRecord<Turn>
                {
                    Item = new StreamServiceExample.StreamItem<Turn>.Superseded(
                        new StreamServiceExample.Supersession("workflow-1", new long[] { 7 })),
                    Offset = 2,
                },
                new StreamServiceExample.StreamRecord<Turn>
                {
                    Item = new StreamServiceExample.StreamItem<Turn>.Close(
                        new StreamServiceExample.StreamClose
                        {
                            Status = StreamServiceExample.StreamCloseStatus
                                .StreamCloseStatusCompleted,
                        }),
                    Offset = 3,
                },
            });

            var wire = Assert.IsType<WireReadRecordsResponse>(
                CreateSdkConverter(typeof(StreamServiceExample.ReadRecordsResponse<Turn>))
                    .ToTransferType(model));
            Assert.Equal(
                new[]
                {
                    WireStreamRecord.ItemOneofCase.Superseded,
                    WireStreamRecord.ItemOneofCase.Close,
                },
                wire.Records.Select(record => record.ItemCase));

            var decoded = FromPayload<StreamServiceExample.ReadRecordsResponse<Turn>>(
                ToPayload(model));
            var superseded = Assert.IsType<StreamServiceExample.StreamItem<Turn>.Superseded>(
                decoded.Records[0].Item);
            Assert.Equal("workflow-1", superseded.Value.ProducerId);
            Assert.Equal(new long[] { 7 }, superseded.Value.FailedTasks);
            var close = Assert.IsType<StreamServiceExample.StreamItem<Turn>.Close>(
                decoded.Records[1].Item);
            Assert.Equal(
                StreamServiceExample.StreamCloseStatus.StreamCloseStatusCompleted,
                close.Value.Status);
            Assert.Equal(3, decoded.Records[1].Offset);
        }

        // A newer server can send a Stream status that this code doesn't know.
        [Fact]
        public void UnknownEnumNumberDecodesAsTheFallbackCase()
        {
            var converter = CreateSdkConverter(typeof(StreamServiceExample.DescribeStreamResponse));
            var wire = new WireDescribeStreamResponse
            {
                Status = (Streams::Temporalio.Api.Enums.V1.StreamStatus)9,
            };

            var decoded = Assert.IsType<StreamServiceExample.DescribeStreamResponse>(
                converter.FromTransferType(wire));
            Assert.Equal(StreamServiceExample.StreamStatus.StreamStatusUnspecified, decoded.Status);
        }

        // The read of a Stream<Turn> returns a ReadRecordsResponse<Turn>, with no type argument.
        private static Task<StreamServiceExample.ReadRecordsResponse<Turn>> ReadTurns(
            StreamServiceExample.Stream<Turn> stream) =>
            stream.ReadAsync(null, null, null, null, null, null);

        // A definition gives the type argument of a create, and its source gives the id.
        private static Task<StreamServiceExample.Stream<Turn>> CreateTurns() =>
            StreamServiceExample.Operations.CreateStreamAsync(
                new Streams::Nexgen.Support.StreamDefinition<Turn>("turns"));

        // A publish takes the values, and answers the offset of the last one.
        private static Task<long> PublishTurns(StreamServiceExample.Stream<Turn> stream) =>
            stream.PublishAsync(new Turn("user"), new Turn("model"));

        // The create takes the backend of the ref as an option, and the ref copies the id.
        private static Task<StreamServiceExample.Stream<Turn>> CreateTurnsOnBackend() =>
            StreamServiceExample.Operations.CreateStreamAsync(
                new Streams::Nexgen.Support.StreamDefinition<Turn>("turns"),
                new StreamServiceExample.CreateStreamOptions { Backend = "redis" });

        [Fact]
        public void EachReasonGivesItsExceptionAndRetryability()
        {
            StreamServiceExample.StreamException From(
                StreamServiceExample.StreamErrorReason reason) =>
                StreamServiceExample.StreamErrors.From(
                    new StreamServiceExample.StreamErrorDetails { Reason = reason }, "failed");

            var notFound = From(StreamServiceExample.StreamErrorReason.StreamErrorReasonNotFound);
            var storage = From(StreamServiceExample.StreamErrorReason.StreamErrorReasonStorage);
            var sequence = From(StreamServiceExample.StreamErrorReason.StreamErrorReasonSequence);
            var unknown = From((StreamServiceExample.StreamErrorReason)99);

            Assert.IsType<StreamServiceExample.StreamNotFoundException>(notFound);
            // The failure type is the same in each SDK.
            Assert.Equal("StreamNotFoundError", notFound.ErrorType);
            Assert.True(notFound.NonRetryable);
            // A storage failure can pass on a retry.
            Assert.IsType<StreamServiceExample.StreamStorageException>(storage);
            Assert.False(storage.NonRetryable);
            // A reason with no class, or one that this code doesn't know, gives the base class.
            Assert.IsType<StreamServiceExample.StreamException>(sequence);
            Assert.IsType<StreamServiceExample.StreamException>(unknown);
            Assert.Equal((StreamServiceExample.StreamErrorReason)99, unknown.Reason);
        }
    }
}
