from __future__ import annotations

import collections.abc
import dataclasses
import typing

from typing_extensions import override

from wit.stream_service.client import StreamServiceClientExtension
from wit.stream_service.models import (
    AppendRecord,
    AppendRecordsRequest,
    AppendRecordsResponse,
    CreateStreamRequest,
    CreateStreamResult,
    ReadRecordsRequest,
    ReadRecordsResponse,
    StreamData,
    StreamItemData,
    StreamRecord,
    StreamRef,
)
from wit.stream_writer.client import StreamWriterClientExtension
from wit.stream_writer.models import (
    CloseWriterRequest,
    CloseWriterResponse,
    CloseWritersRequest,
    CloseWritersResponse,
)


@dataclasses.dataclass
class Call:
    service: str
    endpoint: str | None
    operation: str
    input: object


class FakeNexusClient:
    def __init__(self, client: FakeClient, service: str, endpoint: str | None) -> None:
        self._client: FakeClient = client
        self._service: str = service
        self._endpoint: str | None = endpoint

    async def start_operation(
        self,
        operation: str,
        input: typing.Any,
        *,
        output_type: type[typing.Any] | None = None,
    ) -> collections.abc.Awaitable[typing.Any]:
        del output_type
        self._client.calls.append(Call(self._service, self._endpoint, operation, input))
        answer = self._client.answers[operation].pop(0)

        async def result() -> object:
            return answer

        return result()


class FakeClient(StreamServiceClientExtension, StreamWriterClientExtension):
    """Stands in for the SDK client that inherits the generated extensions."""

    def __init__(self, answers: dict[str, list[object]]) -> None:
        self.answers: dict[str, list[object]] = answers
        self.calls: list[Call] = []

    @override
    def _create_nexus_client(
        self,
        *,
        service: str,
        endpoint: str | None,
    ) -> FakeNexusClient:
        return FakeNexusClient(self, service, endpoint)


async def test_a_created_stream_calls_its_operations_through_the_client() -> None:
    ref = StreamRef(id="s1")
    client = FakeClient(
        {
            "CreateStream": [CreateStreamResult(stream=ref, created=True)],
            "AppendRecords": [AppendRecordsResponse(first_offset=1, last_offset=2)],
        }
    )

    stream = await client.create_stream(stream_id="s1")
    appended = await stream.publish(records=[AppendRecord(body="hi")])

    assert stream.created is True
    assert appended.last_offset == 2
    assert client.calls == [
        Call(
            "temporal.api.streamservice.v1.StreamService",
            "__temporal_system",
            "CreateStream",
            CreateStreamRequest(stream_id="s1"),
        ),
        Call(
            "temporal.api.streamservice.v1.StreamService",
            "__temporal_system",
            "AppendRecords",
            AppendRecordsRequest(stream=ref, records=[AppendRecord(body="hi")]),
        ),
    ]


async def test_a_poll_yields_each_page_until_the_helper_stops() -> None:
    ref = StreamRef(id="s1")
    first = ReadRecordsResponse(
        records=[StreamRecord(item=StreamItemData(StreamData(body="a")))],
        next_offset=2,
    )
    last = ReadRecordsResponse(
        records=[StreamRecord(item=StreamItemData(StreamData(body="b")))],
        next_offset=3,
        done=True,
    )
    client = FakeClient(
        {
            "CreateStream": [CreateStreamResult(stream=ref)],
            "ReadRecords": [first, last],
        }
    )
    stream = await client.create_stream(stream_id="s1")

    pages = [page async for page in stream.iter_read(from_offset=1)]

    assert pages == [first, last]
    assert [call.input for call in client.calls[1:]] == [
        ReadRecordsRequest(stream=ref, from_offset=1),
        ReadRecordsRequest(stream=ref, from_offset=2),
    ]


async def test_a_service_with_no_endpoint_is_left_to_the_client() -> None:
    client = FakeClient({"CloseWriter": [CloseWriterResponse(last_offset=7)]})

    closed = await (await client.close_writer(writer_id=3))

    assert closed.last_offset == 7
    assert client.calls == [
        Call(
            "temporal.sdk.streams.v1.StreamWriter",
            None,
            "CloseWriter",
            CloseWriterRequest(writer_id=3),
        )
    ]


async def test_an_empty_request_takes_no_arguments() -> None:
    client = FakeClient({"CloseWriters": [CloseWritersResponse(failed=0)]})

    closed = await (await client.close_writers())

    assert closed.failed == 0
    assert [call.input for call in client.calls] == [CloseWritersRequest()]
