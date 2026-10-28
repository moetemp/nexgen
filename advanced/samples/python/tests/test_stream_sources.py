from __future__ import annotations

import collections.abc
import dataclasses
import datetime
import typing

import pytest
import temporalio.exceptions
import temporalio.workflow
from streamprotos.temporal.api.enums.v1 import stream_pb2
from streamprotos.temporal.api.stream.v1 import message_pb2

import temporal_support
from typing_extensions import assert_type
import wit.stream_service as stream_service
from wit.stream_service._resources import Stream
from wit.stream_service._resources.stream import (
    WorkflowStreamService,
    bind_stream_operations,
)
from wit.stream_service.errors import StreamFencedError
from wit.stream_service.models import (
    CreateStreamRequest,
    CreateStreamResult,
    Producer,
    StreamRef,
)
from wit.stream_service.operations.create_stream import create_stream


class Turns(temporal_support.StreamDefinition[str]):
    name: typing.ClassVar[str] = "turns"


@dataclasses.dataclass
class FakeWorkflowInfo:
    workflow_id: str
    first_execution_run_id: str
    run_id: str


class FakeNexusClient:
    def __init__(self) -> None:
        self.inputs: list[object] = []

    async def start_operation(
        self, operation: str, input: object, *, output_type: object
    ) -> collections.abc.Awaitable[object]:
        del operation, output_type
        self.inputs.append(input)

        async def result() -> object:
            return CreateStreamResult(stream=StreamRef(id="chat-1:r1:turns"))

        return result()


async def test_a_workflow_create_fills_its_sources(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    nexus_client = FakeNexusClient()
    monkeypatch.setattr(
        temporalio.workflow, "info", lambda: FakeWorkflowInfo("chat-1", "r1", "r2")
    )

    def create_nexus_client(**_: object) -> FakeNexusClient:
        return nexus_client

    monkeypatch.setattr(temporalio.workflow, "create_nexus_client", create_nexus_client)

    stream = await create_stream(Turns, retention=datetime.timedelta(days=1))

    # The id comes from the definition, and the owner from the run, before the request is built.
    # The ref copies the id.
    assert nexus_client.inputs == [
        CreateStreamRequest(
            id="chat-1:r1:turns",
            retention=datetime.timedelta(days=1),
            owner=Producer(id="wf:r2", run_id="r2"),
            ref=StreamRef(id="chat-1:r1:turns"),
        )
    ]
    assert stream.body_type is str


def test_a_workflow_factory_builds_a_handle_on_the_run_operations() -> None:
    handle = stream_service.stream(Turns, id="chat-1:r1:turns", backend="redis")

    _ = assert_type(handle, Stream[str])
    assert handle == Stream(
        body_type=str, stream=StreamRef(id="chat-1:r1:turns", backend="redis")
    )
    # A handle with no operations of its own calls those of the code that runs it.
    assert handle._operations is None


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
    stream = bind_stream_operations(
        Stream(body_type=str, stream=StreamRef(id="s1")), WorkflowStreamService()
    )

    with pytest.raises(StreamFencedError) as raised:
        _ = await stream.publish("a")

    assert raised.value.__cause__ is error
