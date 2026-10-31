"""The client target of the Stream service, on its System Nexus endpoint."""

from __future__ import annotations

import collections.abc
import dataclasses
import datetime
import typing

import pytest
import temporalio.activity
import temporalio.converter
import temporalio.nexus.system
from temporalio.api.common.v1 import Payload
from typing_extensions import assert_type, override

import temporal_support
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
    Producer,
    ReadRecordsRequest,
    ReadRecordsResponse,
    StreamData,
    StreamItemData,
    StreamReadStart,
    StreamRecord,
    StreamRef,
)

STREAM_SERVICE = "temporal.api.streamservice.v1.StreamService"
SYSTEM_ENDPOINT = "__temporal_system"

SYSTEM_CONVERTER = temporalio.nexus.system._SystemNexusPayloadConverter(
    temporalio.converter.PayloadConverter.default,
    temporalio.converter.FailureConverter.default,
)


@dataclasses.dataclass
class Turn:
    role: str


class Turns(temporal_support.StreamDefinition[Turn]):
    name: typing.ClassVar[str] = "turns"


class Texts(temporal_support.StreamDefinition[str]):
    name: typing.ClassVar[str] = "texts"


@pytest.fixture(autouse=True)
def user_payload_converter(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(
        temporalio.nexus.system,
        "_current_user_payload_converter",
        lambda: temporalio.converter.PayloadConverter.default,
    )


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
        rpc_metadata: collections.abc.Mapping[str, str | bytes] | None = None,
        rpc_timeout: datetime.timedelta | None = None,
    ) -> Handle:
        self._client.calls.append((self._service, self._endpoint, operation, arg))
        self._client.operation_ids.append(id)
        self._client.rpc_options.append((rpc_metadata, rpc_timeout))
        answer = self._client.answers[operation].pop(0)
        # A payload answer decodes with the result type, as a real Nexus client does.
        if isinstance(answer, Payload) and result_type is not None:
            answer = typing.cast(
                object, SYSTEM_CONVERTER.from_payload(answer, result_type)
            )
        return Handle(answer)


class Client(StreamServiceClientExtension):
    def __init__(self, answers: dict[str, list[object]]) -> None:
        self.answers: dict[str, list[object]] = answers
        self.calls: list[tuple[str, str, str, object]] = []
        self.operation_ids: list[str] = []
        self.rpc_options: list[
            tuple[
                collections.abc.Mapping[str, str | bytes] | None,
                datetime.timedelta | None,
            ]
        ] = []
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

    stream = await client.create_stream(Texts, id="s1")
    response = await stream.publish(records=[])

    assert isinstance(stream, Stream)
    assert (stream.stream, stream.created) == (ref, True)
    assert response.last_offset == 0
    assert client.calls == [
        (
            STREAM_SERVICE,
            SYSTEM_ENDPOINT,
            "CreateStream",
            # The ref copies the id.
            CreateStreamRequest(id="s1", ref=StreamRef(id="s1")),
        ),
        (
            STREAM_SERVICE,
            SYSTEM_ENDPOINT,
            "AppendRecords",
            # The client fills the producer from its source.
            AppendRecordsRequest(
                stream=ref, records=[], producer=Producer(id="client:sample")
            ),
        ),
    ]


@dataclasses.dataclass
class FakeActivityInfo:
    workflow_run_id: str
    activity_id: str


async def test_an_activity_publish_takes_the_producer_of_its_attempt(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(temporalio.activity, "in_activity", lambda: True)
    monkeypatch.setattr(
        temporalio.activity, "info", lambda: FakeActivityInfo("r1", "a1")
    )
    ref = StreamRef(id="s1")
    client = Client({"AppendRecords": [AppendRecordsResponse(last_offset=1)]})

    _ = await client.get_stream(stream=ref, created=None).publish(records=[])

    assert client.calls == [
        (
            STREAM_SERVICE,
            SYSTEM_ENDPOINT,
            "AppendRecords",
            AppendRecordsRequest(
                stream=ref, records=[], producer=Producer(id="act:r1:a1", run_id="r1")
            ),
        )
    ]


async def test_a_create_takes_the_ref_fields_as_parameters() -> None:
    client = Client({"CreateStream": [CreateStreamResult(stream=StreamRef(id="s1"))]})

    _ = await client.create_stream(Texts, id="s1", backend="redis")

    request = client.calls[0][3]
    # The ref copies the id, and takes the backend.
    assert request == CreateStreamRequest(
        id="s1", ref=StreamRef(id="s1", backend="redis")
    )
    # The proto keeps the ref.
    payload = SYSTEM_CONVERTER.to_payload(request)
    assert SYSTEM_CONVERTER.from_payload(payload, CreateStreamRequest) == request


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


async def test_a_read_that_does_not_wait_stops_at_an_empty_page_that_did_not_move() -> (
    None
):
    ref = StreamRef(id="s1")
    pages = [
        ReadRecordsResponse[str](
            records=[StreamRecord[str](offset=1), StreamRecord[str](offset=2)],
            next_offset=3,
        ),
        ReadRecordsResponse[str](next_offset=3),
    ]
    client = Client({"ReadRecords": list(pages)})

    read = [page async for page in client.iter_read_records(str, stream=ref, last=2)]

    assert read == pages
    assert [call[3] for call in client.calls] == [
        ReadRecordsRequest(stream=ref, last=2),
        ReadRecordsRequest(stream=ref, from_offset=3),
    ]
    # The iteration uses one Nexus client, and each page is a new operation.
    assert client.nexus_clients == 1
    assert len(set(client.operation_ids)) == len(pages)


async def test_a_waiting_read_goes_on_after_an_empty_page_at_the_same_offset() -> None:
    ref = StreamRef(id="s1")
    wait = datetime.timedelta(seconds=30)
    pages = [
        ReadRecordsResponse[str](records=[StreamRecord[str](offset=1)], next_offset=2),
        ReadRecordsResponse[str](next_offset=2),
        ReadRecordsResponse[str](done=True),
    ]
    client = Client({"ReadRecords": list(pages)})

    read = [
        page
        async for page in client.iter_read_records(
            str, stream=ref, from_offset=1, wait=wait
        )
    ]

    assert read == pages
    assert [call[3] for call in client.calls] == [
        ReadRecordsRequest(stream=ref, from_offset=1, wait=wait),
        ReadRecordsRequest(stream=ref, from_offset=2, wait=wait),
        ReadRecordsRequest(stream=ref, from_offset=2, wait=wait),
    ]


async def test_a_stream_read_from_the_end_stays_at_the_first_next_offset() -> None:
    ref = StreamRef(id="s1")
    wait = datetime.timedelta(seconds=30)
    pages = [
        ReadRecordsResponse[str](next_offset=5),
        ReadRecordsResponse[str](records=[StreamRecord[str](offset=5)], next_offset=6),
        ReadRecordsResponse[str](done=True),
    ]
    client = Client(
        {"CreateStream": [CreateStreamResult(stream=ref)], "ReadRecords": list(pages)}
    )
    stream = await client.create_stream(Texts, id="s1")

    read = [
        page
        async for page in stream.iter_read(
            start=StreamReadStart.StreamReadStartEnd, wait=wait
        )
    ]

    assert read == pages
    assert [call[3] for call in client.calls[1:]] == [
        ReadRecordsRequest(
            stream=ref, start=StreamReadStart.StreamReadStartEnd, wait=wait
        ),
        ReadRecordsRequest(stream=ref, from_offset=5, wait=wait),
        ReadRecordsRequest(stream=ref, from_offset=6, wait=wait),
    ]
    # The iterator uses the Nexus client that `create_stream` gave the handle.
    assert client.nexus_clients == 1


async def test_a_done_first_page_ends_the_read() -> None:
    ref = StreamRef(id="s1")
    pages = [
        ReadRecordsResponse[str](
            records=[StreamRecord[str](offset=0)], next_offset=1, done=True
        )
    ]
    client = Client({"ReadRecords": list(pages)})

    read = [page async for page in client.iter_read_records(str, stream=ref)]

    assert read == pages
    assert [call[3] for call in client.calls] == [ReadRecordsRequest(stream=ref)]


async def test_a_changed_page_does_not_change_the_next_request() -> None:
    ref = StreamRef(id="s1")
    wait = datetime.timedelta(seconds=30)
    client = Client(
        {
            "ReadRecords": [
                ReadRecordsResponse[str](
                    records=[StreamRecord[str](offset=1)], next_offset=2
                ),
                ReadRecordsResponse[str](done=True),
            ]
        }
    )

    async for page in client.iter_read_records(
        str, stream=ref, from_offset=1, wait=wait
    ):
        page.next_offset = 9

    assert [call[3] for call in client.calls] == [
        ReadRecordsRequest(stream=ref, from_offset=1, wait=wait),
        ReadRecordsRequest(stream=ref, from_offset=2, wait=wait),
    ]


def test_only_the_client_target_has_the_read_poll() -> None:
    assert hasattr(workflow_stream_module, "read_records")
    assert not hasattr(workflow_stream_module, "iter_read_records")
    assert not hasattr(workflow_package, "iter_read_records")
    assert not hasattr(workflow_stream_module.Stream, "iter_read")
    assert hasattr(Stream, "iter_read")


async def test_a_handle_decodes_its_bodies_with_its_body_type() -> None:
    ref = StreamRef(id="s1")
    record = StreamRecord(
        item=StreamItemData(StreamData(body=Turn(role="model"))), offset=1
    )
    client = Client(
        {
            "CreateStream": [CreateStreamResult(stream=ref)],
            "ReadRecords": [
                SYSTEM_CONVERTER.to_payload(ReadRecordsResponse(records=[record]))
            ],
        }
    )

    stream = await client.create_stream(Turns, id="s1")
    page = await stream.read()

    _ = assert_type(stream, Stream[Turn])
    _ = assert_type(page, ReadRecordsResponse[Turn])
    # The body arrives as a payload, and the handle's body type decodes it.
    assert page.records == [record]


async def test_a_call_and_a_resource_method_pass_their_rpc_options() -> None:
    client = Client(
        {
            "CreateStream": [CreateStreamResult(stream=StreamRef(id="s1"))],
            "DescribeStream": [DescribeStreamResponse(last_offset=5)],
        }
    )
    timeout = datetime.timedelta(seconds=3)

    stream = await client.create_stream(Texts, id="s1", rpc_timeout=timeout)
    _ = await stream.describe(rpc_metadata={"authorization": "token"})

    assert client.rpc_options == [
        ({}, timeout),
        ({"authorization": "token"}, None),
    ]
