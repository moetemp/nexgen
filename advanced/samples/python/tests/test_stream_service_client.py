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
    StreamRef,
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
        answer = self._client.answers[operation]

        async def result() -> object:
            return answer

        return result()


class FakeClient(StreamServiceClientExtension):
    """Stands in for the SDK client that inherits the generated extensions."""

    def __init__(self, answers: dict[str, object]) -> None:
        self.answers: dict[str, object] = answers
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
            "CreateStream": CreateStreamResult(stream=ref, created=True),
            "AppendRecords": AppendRecordsResponse(first_offset=1, last_offset=2),
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
