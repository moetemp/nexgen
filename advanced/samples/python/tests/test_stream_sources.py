from __future__ import annotations

import collections.abc
import dataclasses
import datetime
import typing

import pytest
import temporalio.workflow

import temporal_support
from typing_extensions import assert_type
import wit.stream_service as stream_service
from wit.stream_service._resources import Stream
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
