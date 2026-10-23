from __future__ import annotations

from temporalio.api.common.v1 import Payload

import wit.stream_service.client as stream_service_client
import wit.stream_service.models as stream_service_models
from wit.stream_service._resources import Stream


async def test_client_create_stream_returns_the_stream_class() -> None:
    class Transport:
        async def call(
            self, *, service: str, operation: str, input: Payload
        ) -> Payload:
            assert (service, operation) == ("StreamService", "CreateStream")
            assert b"stream-1" in input.data
            return Payload(
                metadata={"encoding": b"json/plain"},
                data=b'{"stream": {"stream": {"id": "stream-1"}}, "created": true}',
            )

    client = stream_service_client.StreamServiceClient(Transport())
    result = await client.create_stream(
        stream_service_models.CreateStreamRequest(
            ref=stream_service_models.StreamRef(id="stream-1")
        )
    )

    assert isinstance(result.stream, Stream)
    assert result.stream.stream.id == "stream-1"
    assert result.created
