using System.Linq;
using System.Threading.Tasks;
using Temporalio.Converters;
using Xunit;
using static Nexgen.DotNetExamples.Tests.TransferTypeTestSupport;
using StreamServiceExample = Nexgen.StreamService;
using WireDescribeStreamResponse = Temporalio.Api.StreamService.V1.DescribeStreamResponse;
using WireReadRecordsResponse = Temporalio.Api.StreamService.V1.ReadRecordsResponse;
using WireStreamRecord = Temporalio.Api.Stream.V1.StreamRecord;

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
                new StreamServiceExample.StreamRef("stream-1"), null, null);

            var decoded = converter.ToValue<StreamServiceExample.Stream<Turn>>(converter.ToPayload(stream));

            Assert.Equal("stream-1", decoded.StreamValue.Id);
        }

        [Fact]
        public void ReadItemsRoundTripAsTheirRecords()
        {
            using var converterContext = PushConverterContext();
            var model = new StreamServiceExample.ReadRecordsResponse<Turn>(new[]
            {
                new StreamServiceExample.StreamRecord<Turn>(
                    new StreamServiceExample.StreamItem<Turn>.Data(
                        new StreamServiceExample.StreamData<Turn>(new Turn("model"))))
                { Offset = 1 },
                new StreamServiceExample.StreamRecord<Turn>(
                    new StreamServiceExample.StreamItem<Turn>.Superseded(
                        new StreamServiceExample.Supersession("workflow-1", new long[] { 7 })))
                { Offset = 2 },
                new StreamServiceExample.StreamRecord<Turn>(
                    new StreamServiceExample.StreamItem<Turn>.Close(new StreamServiceExample.StreamClose
                    {
                        Status = StreamServiceExample.StreamCloseStatus.StreamCloseStatusCompleted,
                    }))
                { Offset = 3 },
            });

            var wire = Assert.IsType<WireReadRecordsResponse>(
                CreateSdkConverter(typeof(StreamServiceExample.ReadRecordsResponse<Turn>))
                    .ToTransferType(model));
            Assert.Equal(
                new[]
                {
                    WireStreamRecord.ItemOneofCase.Data,
                    WireStreamRecord.ItemOneofCase.Superseded,
                    WireStreamRecord.ItemOneofCase.Close,
                },
                wire.Records.Select(record => record.ItemCase));

            var decoded = FromPayload<StreamServiceExample.ReadRecordsResponse<Turn>>(ToPayload(model));
            // The body decodes as the type argument of the response.
            var data = Assert.IsType<StreamServiceExample.StreamItem<Turn>.Data>(decoded.Records[0].Item);
            Assert.Equal(new Turn("model"), data.Value.Body);
            var superseded = Assert.IsType<StreamServiceExample.StreamItem<Turn>.Superseded>(
                decoded.Records[1].Item);
            Assert.Equal("workflow-1", superseded.Value.ProducerId);
            Assert.Equal(new long[] { 7 }, superseded.Value.FailedTasks);
            var close = Assert.IsType<StreamServiceExample.StreamItem<Turn>.Close>(
                decoded.Records[2].Item);
            Assert.Equal(
                StreamServiceExample.StreamCloseStatus.StreamCloseStatusCompleted,
                close.Value.Status);
            Assert.Equal(3, decoded.Records[2].Offset);
        }

        // The read of a Stream<Turn> returns a ReadRecordsResponse<Turn>, with no type argument.
        private static Task<StreamServiceExample.ReadRecordsResponse<Turn>> ReadTurns(
            StreamServiceExample.Stream<Turn> stream) =>
            stream.ReadAsync(null, null, null, null, null);

        // A newer server can send a Stream status that this code doesn't know.
        [Fact]
        public void UnknownEnumNumberPassesThrough()
        {
            var converter = CreateSdkConverter(typeof(StreamServiceExample.DescribeStreamResponse));
            var wire = new WireDescribeStreamResponse
            {
                Status = (Temporalio.Api.Enums.V1.StreamStatus)9,
            };

            var decoded = Assert.IsType<StreamServiceExample.DescribeStreamResponse>(
                converter.FromTransferType(wire));
            Assert.Equal((StreamServiceExample.StreamStatus)9, decoded.Status);
            var encoded = Assert.IsType<WireDescribeStreamResponse>(converter.ToTransferType(decoded));
            Assert.Equal((Temporalio.Api.Enums.V1.StreamStatus)9, encoded.Status);
        }
    }
}
