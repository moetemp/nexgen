"""The client target of the Stream service, on its System Nexus endpoint."""

from __future__ import annotations

import typing

from typing_extensions import override

import wit.stream_service.both as workflow_package
import wit.stream_service.both._resources.stream as workflow_stream_module
from wit.stream_service.both.client import Stream, StreamServiceClientExtension
from wit.stream_service.both.models import (
    AppendRecordsRequest,
    AppendRecordsResponse,
    CreateStreamRequest,
    CreateStreamResult,
    DescribeStreamRequest,
    DescribeStreamResponse,
    ReadRecordsRequest,
    ReadRecordsResponse,
    StreamRef,
)

STREAM_SERVICE = "temporal.api.streamservice.v1.StreamService"
SYSTEM_ENDPOINT = "__temporal_system"


class Handle:
    def __init__(self, answer: object) -> None:
        self._answer: object = answer

    async def result(self) -> typing.Any:
        return self._answer


class NexusClient:
    def __init__(self, client: Client, service: str, endpoint: str) -> None:
        self._client: Client = client
        self._service: str = service
        self._endpoint: str = endpoint

    async def start_operation(
        self,
        operation: str,
        arg: typing.Any,
        *,
        id: str,
        result_type: type[typing.Any] | None = None,
    ) -> Handle:
        del result_type
        self._client.calls.append((self._service, self._endpoint, operation, arg))
        self._client.operation_ids.append(id)
        return Handle(self._client.answers[operation].pop(0))


class Client(StreamServiceClientExtension):
    def __init__(self, answers: dict[str, list[object]]) -> None:
        self.answers: dict[str, list[object]] = answers
        self.calls: list[tuple[str, str, str, object]] = []
        self.operation_ids: list[str] = []
        self.nexus_clients: int = 0

    @override
    def _create_nexus_client(self, *, service: str, endpoint: str) -> NexusClient:
        self.nexus_clients += 1
        return NexusClient(self, service, endpoint)


async def test_a_created_stream_publishes_on_the_system_endpoint() -> None:
    ref = StreamRef(id="s1")
    client = Client(
        {
            "CreateStream": [CreateStreamResult(stream=ref, created=True)],
            "AppendRecords": [AppendRecordsResponse(first_offset=0, last_offset=0)],
        }
    )

    stream = await client.create_stream(id="s1")
    response = await stream.publish(records=[])

    assert isinstance(stream, Stream)
    assert (stream.stream, stream.created) == (ref, True)
    assert response.last_offset == 0
    assert client.calls == [
        (STREAM_SERVICE, SYSTEM_ENDPOINT, "CreateStream", CreateStreamRequest(id="s1")),
        (
            STREAM_SERVICE,
            SYSTEM_ENDPOINT,
            "AppendRecords",
            AppendRecordsRequest(stream=ref, records=[]),
        ),
    ]


async def test_a_stored_stream_ref_rebuilds_the_handle() -> None:
    ref = StreamRef(id="s2")
    client = Client({"DescribeStream": [DescribeStreamResponse(last_offset=7)]})

    response = await client.get_stream(stream=ref, created=None).describe()

    assert response.last_offset == 7
    assert client.calls == [
        (
            STREAM_SERVICE,
            SYSTEM_ENDPOINT,
            "DescribeStream",
            DescribeStreamRequest(stream=ref),
        )
    ]


async def test_the_client_reads_every_page_and_keeps_the_offset_on_an_empty_page() -> (
    None
):
    ref = StreamRef(id="s1")
    pages = [
        ReadRecordsResponse(next_offset=3),
        ReadRecordsResponse(),
        ReadRecordsResponse(done=True),
    ]
    client = Client({"ReadRecords": list(pages)})

    read = [page async for page in client.iter_read_records(stream=ref, last=2)]

    assert read == pages
    assert [call[3] for call in client.calls] == [
        ReadRecordsRequest(stream=ref, last=2),
        ReadRecordsRequest(stream=ref, from_offset=3),
        # The empty page has no next offset, so the read stays at offset 3.
        ReadRecordsRequest(stream=ref, from_offset=3),
    ]
    # One Nexus client serves the iteration, and each page is a new operation.
    assert client.nexus_clients == 1
    assert len(set(client.operation_ids)) == len(pages)


async def test_a_stream_reads_every_page_on_its_own_nexus_client() -> None:
    ref = StreamRef(id="s1")
    pages = [
        ReadRecordsResponse(next_offset=2),
        ReadRecordsResponse(),
        ReadRecordsResponse(done=True),
    ]
    client = Client(
        {"CreateStream": [CreateStreamResult(stream=ref)], "ReadRecords": list(pages)}
    )
    stream = await client.create_stream(id="s1")

    read = [page async for page in stream.iter_read(from_offset=1)]

    assert read == pages
    assert [call[3] for call in client.calls[1:]] == [
        ReadRecordsRequest(stream=ref, from_offset=1),
        ReadRecordsRequest(stream=ref, from_offset=2),
        ReadRecordsRequest(stream=ref, from_offset=2),
    ]
    assert client.nexus_clients == 1


def test_only_the_client_target_has_the_read_poll() -> None:
    assert hasattr(workflow_stream_module, "read_records")
    assert not hasattr(workflow_stream_module, "iter_read_records")
    assert not hasattr(workflow_package, "iter_read_records")
    assert not hasattr(workflow_stream_module.Stream, "iter_read")
    assert hasattr(Stream, "iter_read")
