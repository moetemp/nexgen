"""The sources of the Stream service's Workflow target."""

from __future__ import annotations

import collections.abc
import dataclasses
import datetime
import inspect
import typing

import pytest
import temporalio.workflow

import temporal_support
from typing_extensions import assert_type
import wit.stream_service.both as stream_service
import wit.stream_service.both._resources.stream as stream_module
from wit.stream_service.both._resources import Stream
from wit.stream_service.both.models import (
    AppendRecord,
    AppendRecordsRequest,
    AppendRecordsResponse,
    CreateStreamRequest,
    CreateStreamResult,
    Execution,
    ExecutionType,
    Producer,
    StreamRef,
)
from wit.stream_service.both.operations.create_stream import create_stream


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
        del output_type
        self.inputs.append(input)

        async def result() -> object:
            if operation == "AppendRecords":
                return AppendRecordsResponse(last_offset=1)
            return CreateStreamResult(stream=StreamRef(id="chat-1:r1:turns"))

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

    # The id comes from the definition, and the owner from the run, before the request is built.
    # The ref copies the id, and names the run chain that owns the Stream.
    assert nexus_client.inputs == [
        CreateStreamRequest(
            id="chat-1:r1:turns",
            retention=datetime.timedelta(days=1),
            owner=Producer(id="wf:r2", run_id="r2"),
            ref=StreamRef(
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


async def test_a_workflow_publish_fills_the_workflow_producer(
    nexus_client: FakeNexusClient,
) -> None:
    ref = StreamRef(id="s1")
    stream = Stream(body_type=str, stream=ref, created=None)

    appended = await stream.publish(records=[AppendRecord(body="hi")])

    assert appended.last_offset == 1
    assert nexus_client.inputs == [
        AppendRecordsRequest(
            stream=ref,
            records=[AppendRecord(body="hi")],
            producer=Producer(id="wf:r2", run_id="r2"),
        )
    ]


def test_a_workflow_read_takes_no_last() -> None:
    # Replay needs a fixed range, so the WIT omits `last` from the Workflow API.
    assert "last" not in inspect.signature(stream_module.read_records).parameters


def test_a_workflow_factory_builds_a_handle_on_the_run_operations() -> None:
    handle = stream_service.stream(Turns, id="chat-1:r1:turns", backend="redis")

    _ = assert_type(handle, Stream[str])
    assert handle == Stream(
        body_type=str, stream=StreamRef(id="chat-1:r1:turns", backend="redis")
    )
