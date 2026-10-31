"""The sources of the Stream service's Workflow target."""

from __future__ import annotations

import collections.abc
import dataclasses
import datetime
import inspect
import typing

import pytest
from temporalio.api.enums.v1 import stream_pb2
from temporalio.api.stream.v1 import message_pb2
import temporalio.exceptions
import temporalio.workflow

import temporal_support
from typing_extensions import assert_type
import wit.stream_service.both as stream_service
import wit.stream_service.both._resources.stream as stream_module
from wit.stream_service.both._resources import Stream
from wit.stream_service.both.errors import StreamFencedError, StreamUnsupportedError
from wit.stream_service.both.models import (
    AppendRecord,
    AppendRecordsRequest,
    AppendRecordsResponse,
    CreateStreamRequest,
    CreateStreamResult,
    Execution,
    ExecutionType,
    Producer,
    StreamErrorDetails,
    StreamErrorReason,
    StreamHandle,
)
from wit.stream_service.both.operations.create_stream import create_stream


class Turns(temporal_support.StreamDefinition[str]):
    name: typing.ClassVar[str] = "turns"


@dataclasses.dataclass
class FakeWorkflowInfo:
    workflow_id: str
    first_execution_run_id: str
    run_id: str
    attempt: int = 1
    continued_run_id: str | None = None


class FakeNexusClient:
    def __init__(self) -> None:
        self.inputs: list[object] = []

    async def start_operation(
        self, operation: str, input: object, *, output_type: object
    ) -> collections.abc.Awaitable[object]:
        del output_type
        self.inputs.append(input)

        async def result() -> object:
            if operation == "AppendRecords":
                return AppendRecordsResponse(last_offset=1)
            return CreateStreamResult(handle=StreamHandle(id="chat-1:r1:turns"))

        return result()


@pytest.fixture
def nexus_client(monkeypatch: pytest.MonkeyPatch) -> FakeNexusClient:
    nexus_client = FakeNexusClient()
    monkeypatch.setattr(
        temporalio.workflow, "info", lambda: FakeWorkflowInfo("chat-1", "r1", "r2")
    )

    def create_nexus_client(**_: object) -> FakeNexusClient:
        return nexus_client

    monkeypatch.setattr(temporalio.workflow, "create_nexus_client", create_nexus_client)
    return nexus_client


async def test_a_workflow_create_fills_its_sources(
    nexus_client: FakeNexusClient,
) -> None:
    stream = await create_stream(Turns, retention=datetime.timedelta(days=1))

    # The id comes from the definition, and the owner from the run. The handle copies the id,
    # and names the run chain that owns the Stream.
    assert nexus_client.inputs == [
        CreateStreamRequest(
            id="chat-1:r1:turns",
            retention=datetime.timedelta(days=1),
            owner=Producer(id="wf:r2", run_id="r2"),
            stream_handle=StreamHandle(
                id="chat-1:r1:turns",
                owner=Execution(
                    type=ExecutionType.ExecutionTypeWorkflow,
                    business_id="chat-1",
                    run_id="r1",
                ),
            ),
        )
    ]
    assert stream.body_type is str


async def test_a_retry_run_withdraws_the_records_of_the_run_that_it_retries(
    nexus_client: FakeNexusClient, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(temporalio.workflow, "in_workflow", lambda: True)
    monkeypatch.setattr(
        temporalio.workflow,
        "info",
        lambda: FakeWorkflowInfo(
            "chat-1", "r1", "r3", attempt=2, continued_run_id="r2"
        ),
    )

    _ = await create_stream(Turns)

    request = typing.cast(CreateStreamRequest, nexus_client.inputs[0])
    assert request.withdraw_run == "r2"


async def test_a_workflow_publish_fills_the_workflow_producer(
    nexus_client: FakeNexusClient,
) -> None:
    ref = StreamHandle(id="s1")
    stream = Stream(body_type=str, handle=ref, created=None)

    last_offset = await stream.publish("hi")

    assert last_offset == 1
    assert nexus_client.inputs == [
        AppendRecordsRequest(
            handle=ref,
            records=[AppendRecord(body="hi")],
            producer=Producer(id="wf:r2", run_id="r2"),
        )
    ]


async def test_a_sync_start_places_the_operation_at_the_call(
    nexus_client: FakeNexusClient,
) -> None:
    ref = StreamHandle(id="s1")

    task = stream_module.start_append_records(
        handle=ref, records=[AppendRecord(body="hi")]
    )

    # Sync Workflow code awaits nothing, so the start can't wait for the caller's await.
    assert nexus_client.inputs == [
        AppendRecordsRequest(
            handle=ref,
            records=[AppendRecord(body="hi")],
            producer=Producer(id="wf:r2", run_id="r2"),
        )
    ]
    handle = await task
    assert await handle == AppendRecordsResponse(last_offset=1)


async def test_a_sync_create_gives_the_stream(nexus_client: FakeNexusClient) -> None:
    task = stream_service.start_create_stream(Turns)

    assert len(nexus_client.inputs) == 1
    stream = await task
    assert stream.handle == StreamHandle(id="chat-1:r1:turns")
    _ = assert_type(stream, Stream[str])


def test_a_workflow_read_takes_no_last() -> None:
    # Replay needs a fixed range, so the WIT omits `last` from the Workflow API.
    assert "last" not in inspect.signature(stream_module.read_records).parameters


async def test_a_workflow_block_leaves_the_close_of_its_stream_to_the_run(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    nexus_client = FakeNexusClient()
    monkeypatch.setattr(temporalio.workflow, "in_workflow", lambda: True)
    monkeypatch.setattr(
        temporalio.workflow, "info", lambda: FakeWorkflowInfo("chat-1", "r1", "r3")
    )

    def create_nexus_client(**_: object) -> FakeNexusClient:
        return nexus_client

    monkeypatch.setattr(temporalio.workflow, "create_nexus_client", create_nexus_client)

    async with create_stream(Turns) as stream:
        _ = assert_type(stream, Stream[str])

    # The block sends only the create, and the sample's policy records the Stream for the run.
    assert len(nexus_client.inputs) == 1
    assert temporal_support.deferred_closes("r3") == [stream]


def test_a_workflow_factory_builds_a_handle_on_the_run_operations() -> None:
    handle = stream_service.stream(Turns, id="chat-1:r1:turns", backend="redis")

    _ = assert_type(handle, Stream[str])
    assert handle == Stream(
        body_type=str, handle=StreamHandle(id="chat-1:r1:turns", backend="redis")
    )


async def test_workflow_code_raises_the_class_of_a_local_activity_failure(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    error = temporalio.exceptions.ActivityError(
        "failed",
        scheduled_event_id=1,
        started_event_id=2,
        identity="worker",
        activity_type="append",
        activity_id="1",
        retry_state=None,
    )
    error.__cause__ = temporalio.exceptions.ApplicationError(
        "fenced",
        message_pb2.StreamErrorDetails(reason=stream_pb2.STREAM_ERROR_REASON_FENCED),
        type="StreamFencedError",
    )

    class FailingNexusClient:
        async def start_operation(
            self, operation: str, input: object, *, output_type: object
        ) -> collections.abc.Awaitable[object]:
            del operation, input, output_type

            async def result() -> object:
                raise error

            return result()

    def create_nexus_client(**_: object) -> FailingNexusClient:
        return FailingNexusClient()

    monkeypatch.setattr(temporalio.workflow, "create_nexus_client", create_nexus_client)
    monkeypatch.setattr(
        temporalio.workflow, "info", lambda: FakeWorkflowInfo("chat-1", "r1", "r2")
    )
    stream = Stream(body_type=str, handle=StreamHandle(id="s1"))

    # The handle's result fails, after the start, and the method converts that too.
    with pytest.raises(StreamFencedError) as raised:
        _ = await stream.publish("a")

    assert raised.value.__cause__ is error
    assert raised.value.message == "fenced"


async def test_a_workflow_create_raises_the_class_of_its_failure(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    details = StreamErrorDetails(reason=StreamErrorReason.StreamErrorReasonUnsupported)
    failure = temporalio.exceptions.ApplicationError("no such backend", details)

    class FailingNexusClient:
        async def start_operation(
            self, operation: str, input: object, *, output_type: object
        ) -> collections.abc.Awaitable[object]:
            del operation, input, output_type
            raise failure

    def create_nexus_client(**_: object) -> FailingNexusClient:
        return FailingNexusClient()

    monkeypatch.setattr(temporalio.workflow, "create_nexus_client", create_nexus_client)
    monkeypatch.setattr(
        temporalio.workflow, "info", lambda: FakeWorkflowInfo("chat-1", "r1", "r2")
    )

    # The operation function converts too, not only the calls of a handle.
    with pytest.raises(StreamUnsupportedError, match="no such backend") as raised:
        _ = await create_stream(Turns, backend="nowhere")

    assert raised.value.__cause__ is failure
