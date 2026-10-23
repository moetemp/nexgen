using StreamServiceExample = Nexgen.StreamService;
using Temporalio.Converters;
using Xunit;

namespace Nexgen.DotNetExamples.Tests
{
    public class StreamServiceChecks
    {
        [Fact]
        public void StreamWithItsOwnFieldNameRoundTrips()
        {
            var converter = DataConverter.Default.PayloadConverter;
            var stream = new StreamServiceExample.Stream(new StreamServiceExample.StreamRef("stream-1"));

            var decoded = converter.ToValue<StreamServiceExample.Stream>(converter.ToPayload(stream));

            Assert.Equal("stream-1", decoded.StreamValue.Id);
        }

        [Fact]
        public void WaitTravelsAsAProto3JsonDuration()
        {
            var converter = DataConverter.Default.PayloadConverter;
            var request = new StreamServiceExample.ReadRecordsRequest(
                new StreamServiceExample.StreamRef("stream-1"), System.Array.Empty<string>())
            {
                Wait = System.TimeSpan.FromMilliseconds(250),
            };

            var payload = converter.ToPayload(request);
            var decoded = converter.ToValue<StreamServiceExample.ReadRecordsRequest>(payload);

            Assert.Contains("\"0.25s\"", payload.Data.ToStringUtf8());
            Assert.Equal(System.TimeSpan.FromMilliseconds(250), decoded.Wait);
        }
    }
}
