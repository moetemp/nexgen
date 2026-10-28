from __future__ import annotations

import collections.abc
import dataclasses
import datetime
import types
import typing

import pytest
from streamprotos.temporal.api.streamservice.v1 import request_response_pb2
from temporalio.api.common.v1 import Payload
import temporalio.activity
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
    Producer,
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
        self.rpc_options: list[
            tuple[
                collections.abc.Mapping[str, str | bytes] | None,
                datetime.timedelta | None,
            ]
        ] = []

    @override
    def _create_nexus_client(
        self,
        *,
        service: str,
        endpoint: str | None,
        rpc_metadata: collections.abc.Mapping[str, str | bytes] | None,
        rpc_timeout: datetime.timedelta | None,
    ) -> FakeNexusClient:
        self.rpc_options.append((rpc_metadata, rpc_timeout))
        return FakeNexusClient(self, service, endpoint)


async def test_a_created_stream_calls_its_operations_through_the_client() -> None:
    ref = StreamRef(id="s1")
    client = FakeClient(
        {
            "CreateStream": [CreateStreamResult(stream=ref, created=False)],
            "AppendRecords": [AppendRecordsResponse(first_offset=1, last_offset=2)],
        }
    )

    stream = await client.create_stream(Texts, id="s1")
    last_offset = await stream.publish("hi")

    # The response sets `created` to False, and the handle keeps it.
    assert stream.created is False
    assert last_offset == 2
    assert client.calls == [
        Call(
            STREAM_SERVICE,
            "__temporal_system",
            "CreateStream",
            CreateStreamRequest(id="s1", ref=StreamRef(id="s1")),
        ),
        Call(
            STREAM_SERVICE,
            "__temporal_system",
            "AppendRecords",
            AppendRecordsRequest(
                stream=ref,
                records=[AppendRecord(body="hi")],
                # The client fills the producer from its source.
                producer=Producer(id="client:sample"),
            ),
        ),
    ]


async def test_a_create_takes_the_ref_fields_as_parameters() -> None:
    client = FakeClient(
        {"CreateStream": [CreateStreamResult(stream=StreamRef(id="s1"))]}
    )

    _ = await client.create_stream(Texts, id="s1", backend="redis")

    request = client.calls[0].input
    # The ref copies the id, and takes the backend.
    assert request == CreateStreamRequest(
        id="s1", ref=StreamRef(id="s1", backend="redis")
    )
    wire = request_response_pb2.CreateStreamRequest.FromString(
        SYSTEM_CONVERTER.to_payload(request).data
    )
    assert wire.stream_id == "s1"
    assert (wire.ref.id, wire.ref.backend) == ("s1", "redis")


async def test_a_factory_builds_a_handle_with_no_call() -> None:
    client = FakeClient({"DescribeStream": [DescribeStreamResponse(last_offset=5)]})

    by_id = client.stream(Turns, id="s1", backend="redis")
    by_ref = client.stream(Turns, stream=StreamRef(id="s2"))

    _ = assert_type(by_id, Stream[Turn])
    assert by_id == Stream(body_type=Turn, stream=StreamRef(id="s1", backend="redis"))
    assert by_id.body_type is Turn
    assert by_ref.stream == StreamRef(id="s2")
    assert client.calls == []
    # The handle calls through the client that built it.
    _ = await by_id.describe()
    assert [call.operation for call in client.calls] == ["DescribeStream"]


def test_a_factory_takes_the_ref_fields_or_the_ref() -> None:
    client = FakeClient({})

    with pytest.raises(TypeError, match="pass id or stream"):
        _ = client.stream(Turns)
    with pytest.raises(TypeError, match="not both"):
        _ = client.stream(Turns, id="s1", stream=StreamRef(id="s1"))


async def test_a_handle_passes_as_its_ref() -> None:
    converter = temporalio.converter.DataConverter.default
    stream = Stream(body_type=Turn, stream=StreamRef(id="s1"), created=True)

    (payload,) = await converter.encode([stream])
    (decoded,) = await converter.decode([payload], [Stream[Turn]])

    # The wire form is the ref, and the parameter type gives the body type. Only a create's
    # handle has `created`.
    assert payload.metadata["messageType"] == b"temporal.api.stream.v1.StreamRef"
    assert decoded == Stream(body_type=Turn, stream=StreamRef(id="s1"))
    assert decoded.body_type is Turn


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

    read = [item async for item in client.iter_read_records(str, stream=ref, last=2)]

    assert read == []
    assert [call.input for call in client.calls] == [
        ReadRecordsRequest(stream=ref, last=2),
        ReadRecordsRequest(stream=ref, from_offset=3),
        # The empty page has no next offset, so the read stays at offset 3.
        ReadRecordsRequest(stream=ref, from_offset=3),
    ]


def stream_record(body: str, offset: int) -> StreamRecord[str]:
    return StreamRecord(item=StreamItemData(StreamData(body=body)), offset=offset)


async def test_a_handle_yields_items_and_resumes_after_each() -> None:
    ref = StreamRef(id="s1")
    pages = [
        ReadRecordsResponse(
            records=[stream_record("a", 10), stream_record("b", 12)], next_offset=15
        ),
        ReadRecordsResponse[str](next_offset=15),
        ReadRecordsResponse(
            records=[stream_record("c", 20)], next_offset=21, done=True
        ),
    ]
    client = FakeClient(
        {"CreateStream": [CreateStreamResult(stream=ref)], "ReadRecords": list(pages)}
    )
    stream = await client.create_stream(Texts, id="s1")

    read = stream.iter_read(from_offset=10)
    seen: list[tuple[int | None, int | None]] = []
    async for record in read:
        seen.append(
            (record.offset, read.next_request and read.next_request.from_offset)
        )

    # After an item, the read resumes past it, and after a page's last item, where the page ends.
    assert seen == [(10, 11), (12, 15), (20, None)]
    assert read.next_request is None
    assert [call.input for call in client.calls[1:]] == [
        ReadRecordsRequest(stream=ref, from_offset=10),
        ReadRecordsRequest(stream=ref, from_offset=15),
        ReadRecordsRequest(stream=ref, from_offset=15),
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

    stream = await client.create_stream(Turns, id="s1")
    page = await stream.read()

    _ = assert_type(stream, Stream[Turn])
    _ = assert_type(page, ReadRecordsResponse[Turn])
    # The body arrives as a payload, and the handle's body type decodes it.
    assert page.records == [record]


async def test_a_resource_method_passes_its_rpc_options_to_the_client() -> None:
    client = FakeClient({"DescribeStream": [DescribeStreamResponse(last_offset=5)]})
    stream = Stream(
        body_type=str,
        stream=StreamRef(id="s1"),
        created=None,
        status=None,
        _operations=client,
    )

    _ = await stream.describe(
        rpc_metadata={"authorization": "token"},
        rpc_timeout=datetime.timedelta(seconds=3),
    )

    assert client.rpc_options == [
        ({"authorization": "token"}, datetime.timedelta(seconds=3))
    ]


async def test_workflow_operations_refuse_rpc_options() -> None:
    request = DescribeStreamRequest(stream=StreamRef(id="s1"))

    with pytest.raises(ValueError, match="outside a Workflow"):
        _ = await WorkflowStreamService()._stream_service_describe_stream(
            request, rpc_timeout=datetime.timedelta(seconds=3)
        )


@dataclasses.dataclass
class FakeActivityInfo:
    workflow_run_id: str
    activity_id: str


async def test_an_activity_append_takes_the_producer_of_its_attempt(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(temporalio.activity, "in_activity", lambda: True)
    monkeypatch.setattr(
        temporalio.activity, "info", lambda: FakeActivityInfo("r1", "a1")
    )
    ref = StreamRef(id="s1")
    client = FakeClient({"AppendRecords": [AppendRecordsResponse(last_offset=1)]})
    stream = Stream(
        body_type=str, stream=ref, created=None, status=None, _operations=client
    )

    _ = await stream.publish("hi")

    # The resource method leaves the producer unset, so the client's source fills it.
    assert [call.input for call in client.calls] == [
        AppendRecordsRequest(
            stream=ref,
            records=[AppendRecord(body="hi")],
            producer=Producer(id="act:r1:a1", run_id="r1"),
        )
    ]


async def test_a_publish_wraps_each_value_and_returns_the_last_offset() -> None:
    ref = StreamRef(id="s1")
    client = FakeClient(
        {
            "AppendRecords": [
                AppendRecordsResponse(first_offset=4, last_offset=5),
                AppendRecordsResponse(),
            ]
        }
    )
    stream = Stream(body_type=str, stream=ref, _operations=client)

    last_offset = await stream.publish("a", "b")
    empty = await stream.publish()

    _ = assert_type(last_offset, int)
    assert last_offset == 5
    assert [call.input for call in client.calls][0] == AppendRecordsRequest(
        stream=ref,
        records=[AppendRecord(body="a"), AppendRecord(body="b")],
        producer=Producer(id="client:sample"),
    )
    # `last_offset` has no presence on the wire, so an empty publish answers 0.
    assert empty == 0


async def test_an_append_keeps_the_producer_that_sdk_code_sets() -> None:
    ref = StreamRef(id="s1")
    client = FakeClient({"AppendRecords": [AppendRecordsResponse(last_offset=1)]})
    request = AppendRecordsRequest[str](
        stream=ref, producer=Producer(id="writer-7"), sequence=3
    )

    _ = await client._stream_service_append_records(request)

    assert [call.input for call in client.calls] == [request]


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
