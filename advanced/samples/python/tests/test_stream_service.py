from __future__ import annotations

import datetime

from streamsample.v1 import stream_service_pb2
from temporalio.api.common.v1 import Payload

import wit.stream_service.client as stream_service_client
import wit.stream_service.models as stream_service_models


def _proto_payload(message: stream_service_pb2.ReadRecordsResponse) -> Payload:
    return Payload(
        metadata={"encoding": b"binary/protobuf"},
        data=message.SerializeToString(),
    )


async def test_iterator_reads_every_page_from_the_next_offset() -> None:
    pages = [
        stream_service_pb2.ReadRecordsResponse(
            records=[stream_service_pb2.StreamRecord(offset=1)], next_offset=2
        ),
        stream_service_pb2.ReadRecordsResponse(next_offset=2),
        stream_service_pb2.ReadRecordsResponse(
            records=[stream_service_pb2.StreamRecord(offset=2)],
            next_offset=3,
            done=True,
        ),
    ]
    asked: list[int] = []
    waits: list[bool] = []

    class Transport:
        async def call(
            self, *, service: str, operation: str, input: Payload
        ) -> Payload:
            assert (service, operation) == (
                "streamsample.v1.StreamService",
                "ReadRecords",
            )
            request = stream_service_pb2.ReadRecordsRequest.FromString(input.data)
            asked.append(request.from_offset)
            waits.append(request.HasField("wait"))
            return _proto_payload(pages.pop(0))

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
    assert asked == [0, 2, 2]
    assert all(waits)
