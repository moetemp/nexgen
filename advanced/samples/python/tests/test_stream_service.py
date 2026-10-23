from __future__ import annotations

import datetime

import temporalio.converter
from temporalio.api.common.v1 import Payload

import wit.stream_service.client as stream_service_client
import wit.stream_service.models as stream_service_models

CONVERTER = temporalio.converter.DataConverter.default.payload_converter


async def test_client_create_stream_returns_a_bound_stream() -> None:
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

    assert isinstance(result.stream, stream_service_client.Stream)
    assert result.stream.stream.id == "stream-1"
    assert result.created
    assert result.stream._client is client


async def test_iterator_reads_every_page_from_the_next_offset() -> None:
    def page(
        offsets: list[int], next_offset: int, done: bool | None = None
    ) -> stream_service_models.ReadRecordsResponse:
        return stream_service_models.ReadRecordsResponse(
            records=[
                stream_service_models.StreamRecord(
                    kind=stream_service_models.RecordKind.Data, offset=offset
                )
                for offset in offsets
            ],
            next_offset=next_offset,
            done=done,
        )

    pages = [page([1], 2), page([], 2), page([2], 3, done=True)]
    asked: list[int | None] = []

    class Transport:
        async def call(
            self, *, service: str, operation: str, input: Payload
        ) -> Payload:
            assert (service, operation) == ("StreamService", "ReadRecords")
            request = CONVERTER.from_payload(
                input, stream_service_models.ReadRecordsRequest
            )
            asked.append(request.from_offset)
            return CONVERTER.to_payload(pages.pop(0))

    client = stream_service_client.StreamServiceClient(Transport())
    request = stream_service_models.ReadRecordsRequest(
        stream=stream_service_models.StreamRef(id="stream-1")
    )
    offsets = [
        record.offset
        async for record in client.iter_read_records(
            request, deadline=datetime.timedelta(seconds=5)
        )
    ]

    assert offsets == [1, 2]
    assert asked == [None, 2, 2]
