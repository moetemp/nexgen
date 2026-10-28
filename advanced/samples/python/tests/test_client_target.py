from __future__ import annotations

import collections.abc
import dataclasses
import types
import typing

import pytest
from temporalio.api.common.v1 import Payload
import temporalio.converter
import temporalio.nexus.system
import temporalio.workflow
from typing_extensions import assert_type, override

import temporal_support
import wit.stream_service._resources.stream as stream_module
import wit.workflow_handle as workflow_handle
from wit.stream_service._resources import Stream
from wit.stream_service._resources.stream import WorkflowStreamService
from wit.stream_service.client import StreamServiceClientExtension
from wit.stream_service.models import (
    AppendRecord,
    AppendRecordsRequest,
    AppendRecordsResponse,
    CreateStreamRequest,
    CreateStreamResult,
    DescribeStreamRequest,
    DescribeStreamResponse,
    ReadRecordsRequest,
    ReadRecordsResponse,
    StreamData,
    StreamItemData,
    StreamRecord,
    StreamRef,
)
from wit.stream_writer.client import StreamWriterClientExtension
from wit.stream_writer import models as writer_models
from wit.stream_writer.models import (
    CloseWriterRequest,
    CloseWriterResponse,
    CloseWritersRequest,
    CloseWritersResponse,
    OpenWriterRequest,
    WriterResult,
)
from wit.workflow_handle._resources import WorkflowHandle
from wit.workflow_handle.client import WorkflowHandleServiceClientExtension
from wit.workflow_handle.models import StartWorkflowRequest, StartWorkflowResult

STREAM_SERVICE = "temporal.api.streamservice.v1.StreamService"
STREAM_WRITER = "temporal.sdk.streams.v1.StreamWriter"


@dataclasses.dataclass
class Turn:
    role: str


class Turns(temporal_support.StreamDefinition[Turn]):
    name: typing.ClassVar[str] = "turns"


class Texts(temporal_support.StreamDefinition[str]):
    name: typing.ClassVar[str] = "texts"


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
        self._client.calls.append(Call(self._service, self._endpoint, operation, input))
        answer = self._client.answers[operation].pop(0)
        # A payload answer decodes with the output type, as a real Nexus client does.
        if isinstance(answer, Payload) and output_type is not None:
            answer = typing.cast(
                object, SYSTEM_CONVERTER.from_payload(answer, output_type)
            )

        async def result() -> object:
            return answer

        return result()


SYSTEM_CONVERTER = temporalio.nexus.system._SystemNexusPayloadConverter(
    temporalio.converter.PayloadConverter.default,
    temporalio.converter.FailureConverter.default,
)


@pytest.fixture(autouse=True)
def user_payload_converter(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(
        temporalio.nexus.system,
        "_current_user_payload_converter",
        lambda: temporalio.converter.PayloadConverter.default,
    )


class FakeClient(
    StreamServiceClientExtension,
    StreamWriterClientExtension,
    WorkflowHandleServiceClientExtension,
):
    """Stands in for the SDK client, which inherits the extensions of several services."""

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
            "CreateStream": [CreateStreamResult(stream=ref, created=False)],
            "AppendRecords": [AppendRecordsResponse(first_offset=1, last_offset=2)],
        }
    )

    stream = await client.create_stream(Texts, stream_id="s1")
    appended = await stream.publish(records=[AppendRecord(body="hi")])

    # The response sets `created` to False, and the handle keeps it.
    assert stream.created is False
    assert appended.last_offset == 2
    assert client.calls == [
        Call(
            STREAM_SERVICE,
            "__temporal_system",
            "CreateStream",
            CreateStreamRequest(stream_id="s1"),
        ),
        Call(
            STREAM_SERVICE,
            "__temporal_system",
            "AppendRecords",
            AppendRecordsRequest(stream=ref, records=[AppendRecord(body="hi")]),
        ),
    ]


async def test_a_client_call_returns_the_result() -> None:
    client = FakeClient({"DescribeStream": [DescribeStreamResponse(last_offset=5)]})

    described = await client.describe_stream(stream=StreamRef(id="s1"))

    assert described.last_offset == 5


async def test_a_handle_built_by_hand_takes_the_current_client() -> None:
    ref = StreamRef(id="s1")
    client = FakeClient({"DescribeStream": [DescribeStreamResponse(last_offset=5)]})
    stream = Stream(body_type=str, stream=ref, created=None, status=None)

    with temporal_support.current_client(client):
        described = await stream.describe()

    assert described.last_offset == 5
    assert [call.input for call in client.calls] == [DescribeStreamRequest(stream=ref)]


def test_a_handle_in_workflow_code_takes_the_workflow_operations(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(temporalio.workflow, "in_workflow", lambda: True)
    stream = Stream(body_type=str, stream=StreamRef(id="s1"), created=None, status=None)

    assert isinstance(stream._ops(), WorkflowStreamService)


async def test_a_read_poll_keeps_its_offset_on_an_empty_page() -> None:
    ref = StreamRef(id="s1")
    pages = [
        ReadRecordsResponse[str](next_offset=3),
        ReadRecordsResponse[str](),
        ReadRecordsResponse[str](done=True),
    ]
    client = FakeClient({"ReadRecords": list(pages)})

    read = [page async for page in client.iter_read_records(str, stream=ref, last=2)]

    assert read == pages
    assert [call.input for call in client.calls] == [
        ReadRecordsRequest(stream=ref, last=2),
        ReadRecordsRequest(stream=ref, from_offset=3),
        # The empty page has no next offset, so the read stays at offset 3.
        ReadRecordsRequest(stream=ref, from_offset=3),
    ]


async def test_a_handle_reads_every_page() -> None:
    ref = StreamRef(id="s1")
    pages = [
        ReadRecordsResponse[str](next_offset=2),
        ReadRecordsResponse[str](done=True),
    ]
    client = FakeClient(
        {"CreateStream": [CreateStreamResult(stream=ref)], "ReadRecords": list(pages)}
    )
    stream = await client.create_stream(Texts, stream_id="s1")

    read = [page async for page in stream.iter_read(from_offset=1)]

    assert read == pages
    assert [call.input for call in client.calls[1:]] == [
        ReadRecordsRequest(stream=ref, from_offset=1),
        ReadRecordsRequest(stream=ref, from_offset=2),
    ]


async def test_a_handle_decodes_its_bodies_with_its_body_type() -> None:
    ref = StreamRef(id="s1")
    record = StreamRecord(
        item=StreamItemData(StreamData(body=Turn(role="model"))), offset=1
    )
    client = FakeClient(
        {
            "CreateStream": [CreateStreamResult(stream=ref)],
            "ReadRecords": [
                SYSTEM_CONVERTER.to_payload(ReadRecordsResponse(records=[record]))
            ],
        }
    )

    stream = await client.create_stream(Turns, stream_id="s1")
    page = await stream.read()

    _ = assert_type(stream, Stream[Turn])
    _ = assert_type(page, ReadRecordsResponse[Turn])
    # The body arrives as a payload, and the handle's body type decodes it.
    assert page.records == [record]


def test_only_the_client_target_has_the_read_poll() -> None:
    # The WIT names the client target, since Workflow code reads from a Local Activity.
    assert hasattr(stream_module, "read_records")
    assert not hasattr(stream_module, "iter_read_records")


async def test_a_service_with_no_endpoint_is_left_to_the_client() -> None:
    ref = writer_models.StreamRef(id="s1")
    client = FakeClient(
        {
            "OpenWriter": [WriterResult(writer_id=3)],
            "CloseWriter": [CloseWriterResponse(last_offset=7)],
        }
    )

    writer = await client.open_writer(str, stream=ref)
    closed = await writer.close()

    assert closed.last_offset == 7
    assert client.calls == [
        Call(STREAM_WRITER, None, "OpenWriter", OpenWriterRequest(stream=ref)),
        Call(STREAM_WRITER, None, "CloseWriter", CloseWriterRequest(writer_id=3)),
    ]


async def test_an_empty_request_takes_no_arguments() -> None:
    client = FakeClient({"CloseWriters": [CloseWritersResponse(failed=0)]})

    closed = await client.close_writers()

    assert closed.failed == 0
    assert [call.input for call in client.calls] == [CloseWritersRequest()]


def test_a_client_class_cant_get_an_operation_from_two_bases() -> None:
    class Base:
        def create_stream(self) -> None: ...

    with pytest.raises(TypeError, match="gets create_stream from .*Base and from"):
        _ = types.new_class("Client", (Base, FakeClient))


async def test_a_package_with_only_the_client_target_takes_a_sourced_field() -> None:
    client = FakeClient({"StartWorkflow": [StartWorkflowResult(run_id="r1")]})

    # Outside a Workflow, the field that Workflow code fills from a source is a parameter.
    handle = await client.start_workflow(
        workflow="Greet", workflow_id="w1", task_queue="q", namespace="default"
    )

    assert handle == WorkflowHandle(namespace="default", workflow_id="w1", run_id="r1")
    assert [call.input for call in client.calls] == [
        StartWorkflowRequest(
            workflow="Greet", workflow_id="w1", task_queue="q", namespace="default"
        )
    ]


def test_a_package_with_only_the_client_target_has_its_serialization_contexts() -> None:
    info = workflow_handle.__nexus_operation_registry__[
        ("WorkflowHandleService", "StartWorkflow")
    ]
    request = StartWorkflowRequest(
        workflow="Greet", workflow_id="w1", task_queue="q", namespace="default"
    )

    assert info.serialization_context is not None
    assert info.serialization_context(
        request
    ) == temporalio.converter.WorkflowSerializationContext(
        namespace="default", workflow_id="w1"
    )
