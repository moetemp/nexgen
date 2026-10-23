using Temporalio.Converters;
using UserServiceExample = Nexgen.UserService;
using Xunit;

namespace Nexgen.DotNetExamples.Tests
{
    public class UserServiceSessionChecks
    {
        [Fact]
        public void SessionWithItsOwnFieldNameRoundTrips()
        {
            var converter = DataConverter.Default.PayloadConverter;
            var session = new UserServiceExample.Session("session-1");

            var decoded = converter.ToValue<UserServiceExample.Session>(converter.ToPayload(session));

            Assert.Equal("session-1", decoded.SessionValue);
        }
    }
}
