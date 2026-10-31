"""The sources of the Stream service's Workflow target."""

from __future__ import annotations

import collections.abc
import dataclasses
import datetime
import inspect
import uuid

import pytest
from temporalio.api.enums.v1 import stream_pb2
from temporalio.api.stream.v1 import message_pb2
import temporalio.exceptions
import temporalio.workflow
from typing_extensions import assert_type

import temporal_support
import wit.stream_service.both.workflow as stream_service
import wit.stream_service.both.workflow._resources.stream as stream_module
from wit.stream_service.both.workflow._resources import Stream
from wit.stream_service.both.errors import StreamFencedError, StreamUnsupportedError
from wit.stream_service.both.models import (
    AppendRecord,
    AppendRecordsRequest,
    AppendRecordsResponse,
    CreateStreamRequest,
    CreateStreamResult,
    Producer,
    StreamErrorDetails,
    StreamErrorReason,
    StreamRef,
    StreamWriter,
)
from wit.stream_service.both.workflow.operations.create_stream import create_stream


REQUEST_ID = uuid.UUID(int=7)


@dataclasses.dataclass
class FakeWorkflowInfo:
    run_id: str
    # A first attempt, so `withdraw_run` gives nothing.
    attempt: int = 1


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
            return CreateStreamResult(ref=StreamRef(id="turns"))

        return result()


@pytest.fixture
def nexus_client(monkeypatch: pytest.MonkeyPatch) -> FakeNexusClient:
    nexus_client = FakeNexusClient()
    monkeypatch.setattr(temporalio.workflow, "in_workflow", lambda: True)
    monkeypatch.setattr(temporalio.workflow, "info", lambda: FakeWorkflowInfo("r2"))
    monkeypatch.setattr(temporalio.workflow, "uuid4", lambda: REQUEST_ID)

    def create_nexus_client(**_: object) -> FakeNexusClient:
        return nexus_client

    monkeypatch.setattr(temporalio.workflow, "create_nexus_client", create_nexus_client)
    return nexus_client


async def test_a_workflow_create_fills_its_sources(
    nexus_client: FakeNexusClient,
) -> None:
    stream = await create_stream(
        str, id="turns", backend="redis", retention=datetime.timedelta(days=1)
    )

    # The request id comes from the Workflow, so a replay sends the same one. The ref takes the id
    # and the backend.
    assert nexus_client.inputs == [
        CreateStreamRequest(
            stream_ref=StreamRef(id="turns", backend="redis"),
            retention=datetime.timedelta(days=1),
            request_id=str(REQUEST_ID),
        )
    ]
    assert stream.body_type is str


async def test_a_workflow_publish_fills_the_workflow_writer(
    nexus_client: FakeNexusClient,
) -> None:
    ref = StreamRef(id="s1")
    stream = Stream(body_type=str, ref=ref, created=None)

    last_offset = await stream.publish("hi")

    assert last_offset == 1
    assert nexus_client.inputs == [
        AppendRecordsRequest(
            ref=ref,
            records=[AppendRecord(body="hi")],
            writer=StreamWriter(
                producer=Producer(id="wf:r2", run_id="r2"), failed_tasks=[]
            ),
        )
    ]


async def test_a_sync_start_places_the_operation_at_the_call(
    nexus_client: FakeNexusClient,
) -> None:
    ref = StreamRef(id="s1")

    task = stream_module.start_append_records(
        ref=ref, records=[AppendRecord(body="hi")]
    )

    # Sync Workflow code awaits nothing, so the start can't wait for the caller's await.
    assert nexus_client.inputs == [
        AppendRecordsRequest(
            ref=ref,
            records=[AppendRecord(body="hi")],
            writer=StreamWriter(
                producer=Producer(id="wf:r2", run_id="r2"), failed_tasks=[]
            ),
        )
    ]
    handle = await task
    assert await handle == AppendRecordsResponse(last_offset=1)


async def test_a_sync_create_gives_the_stream(nexus_client: FakeNexusClient) -> None:
    task = stream_service.start_create_stream(str, id="turns")

    assert len(nexus_client.inputs) == 1
    stream = await task
    assert stream.ref == StreamRef(id="turns")
    _ = assert_type(stream, Stream[str])


def test_a_workflow_read_takes_no_last() -> None:
    # Replay needs a fixed range, so the WIT omits `last` from the Workflow API.
    assert "last" not in inspect.signature(stream_module.read_records).parameters


async def test_a_workflow_block_leaves_the_close_of_its_stream_to_the_run(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    nexus_client = FakeNexusClient()
    monkeypatch.setattr(temporalio.workflow, "in_workflow", lambda: True)
    monkeypatch.setattr(temporalio.workflow, "info", lambda: FakeWorkflowInfo("r3"))
    monkeypatch.setattr(temporalio.workflow, "uuid4", lambda: REQUEST_ID)

    def create_nexus_client(**_: object) -> FakeNexusClient:
        return nexus_client

    monkeypatch.setattr(temporalio.workflow, "create_nexus_client", create_nexus_client)

    async with create_stream(str, id="turns") as stream:
        _ = assert_type(stream, Stream[str])

    # The block sends only the create, and the sample's policy records the Stream for the run.
    assert len(nexus_client.inputs) == 1
    assert temporal_support.deferred_closes("r3") == [stream]


def test_a_workflow_factory_builds_a_handle_on_the_run_operations() -> None:
    handle = stream_service.get_stream(id="turns", backend="redis", body_type=str)

    _ = assert_type(handle, Stream[str])
    assert handle.ref == StreamRef(id="turns", backend="redis")
    assert handle.created is None
    assert handle.body_type is str


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
    monkeypatch.setattr(temporalio.workflow, "info", lambda: FakeWorkflowInfo("r2"))
    stream = Stream(ref=StreamRef(id="s1"), created=None, body_type=str)

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
    monkeypatch.setattr(temporalio.workflow, "info", lambda: FakeWorkflowInfo("r2"))

    # The operation function converts too, not only the calls of a handle.
    with pytest.raises(StreamUnsupportedError, match="no such backend") as raised:
        _ = await create_stream(str, id="turns", backend="nowhere")

    assert raised.value.__cause__ is failure
