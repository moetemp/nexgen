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
    }
}
