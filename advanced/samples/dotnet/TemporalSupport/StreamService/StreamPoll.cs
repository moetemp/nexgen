using System;
using Temporalio.Workflows;

namespace Nexgen.Support
{
    // The poll helper of the stream-service sample. Only its project compiles it, since it names
    // the sample's generated requests.
    internal static class StreamPoll
    {
        internal static ReadRecordsRequest? NextReadRequest<TBody>(
            ReadRecordsRequest request, ReadRecordsResponse<TBody> response)
        {
            if (response.Done == true)
            {
                return null;
            }
            var nextOffset = response.NextOffset;
            // A read that doesn't wait ends at the newest Record.
            if (response.Records.Count == 0
                && (request.Wait ?? TimeSpan.Zero) == TimeSpan.Zero
                && (nextOffset == null || nextOffset == request.FromOffset))
            {
                return null;
            }
            if (nextOffset == null)
            {
                // `Last` reads the newest Records only once.
                return request with { Last = null };
            }
            // `Last` takes no `FromOffset`, and a `FromOffset` overrides `Start`.
            return request with { FromOffset = nextOffset, Start = null, Last = null };
        }
    }
}
