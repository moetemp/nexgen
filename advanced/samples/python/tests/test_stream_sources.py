"""The sources of the Stream service's Workflow target."""

from __future__ import annotations

import collections.abc
import dataclasses
import datetime
import inspect
import uuid

import pytest
import temporalio.workflow

import wit.stream_service.both.workflow._resources.stream as stream_module
from wit.stream_service.both.workflow._resources import Stream
from wit.stream_service.both.models import (
    AppendRecord,
    AppendRecordsRequest,
    AppendRecordsResponse,
    CreateStreamRequest,
    CreateStreamResult,
    Producer,
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
        str,
        stream_ref=StreamRef(id="turns"),
        retention=datetime.timedelta(days=1),
    )

    # The request id comes from the Workflow, so a replay sends the same one.
    assert nexus_client.inputs == [
        CreateStreamRequest(
            stream_ref=StreamRef(id="turns"),
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

    appended = await stream.publish(records=[AppendRecord(body="hi")])

    assert appended.last_offset == 1
    assert nexus_client.inputs == [
        AppendRecordsRequest(
            ref=ref,
            records=[AppendRecord(body="hi")],
            writer=StreamWriter(
                producer=Producer(id="wf:r2", run_id="r2"), failed_tasks=[]
            ),
        )
    ]


def test_a_workflow_read_takes_no_last() -> None:
    # Replay needs a fixed range, so the WIT omits `last` from the Workflow API.
    assert "last" not in inspect.signature(stream_module.read_records).parameters
