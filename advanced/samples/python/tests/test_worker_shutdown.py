from __future__ import annotations

import dataclasses
import json

import pytest
import temporalio.converter
import temporalio.workflow

from wit.worker_shutdown.models import (
    ShutdownWorkerRequest,
    ShutdownWorkerResponse,
    WorkerHeartbeat,
)
from wit.worker_shutdown.operations.shutdown_worker import shutdown_worker


@dataclasses.dataclass
class FakeWorkflowInfo:
    namespace: str


class FakeNexusClient:
    def __init__(self) -> None:
        self.inputs: list[object] = []

    async def start_operation(
        self, operation: str, input: object, *, output_type: object
    ) -> ShutdownWorkerResponse:
        del operation, output_type
        self.inputs.append(input)
        return ShutdownWorkerResponse()


async def test_a_shutdown_fills_the_heartbeat_from_the_request(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    nexus_client = FakeNexusClient()
    monkeypatch.setattr(
        temporalio.workflow, "info", lambda: FakeWorkflowInfo("default")
    )

    def create_nexus_client(**_: object) -> FakeNexusClient:
        return nexus_client

    monkeypatch.setattr(temporalio.workflow, "create_nexus_client", create_nexus_client)

    _ = await shutdown_worker(
        identity="worker-1",
        worker_instance_key="key-1",
        task_queue="queue",
        sdk_name="temporal-python",
    )

    # The heartbeat takes the SDK name, and copies the worker fields of the request.
    heartbeat = WorkerHeartbeat(
        worker_instance_key="key-1",
        worker_identity="worker-1",
        task_queue="queue",
        sdk_name="temporal-python",
    )
    request = ShutdownWorkerRequest(
        namespace="default",
        identity="worker-1",
        worker_instance_key="key-1",
        task_queue="queue",
        worker_heartbeat=heartbeat,
    )
    assert nexus_client.inputs == [request]
    payload = (await temporalio.converter.DataConverter.default.encode([request]))[0]
    wire = json.loads(payload.data)
    assert wire["workerHeartbeat"] == {
        "workerInstanceKey": "key-1",
        "workerIdentity": "worker-1",
        "taskQueue": "queue",
        "sdkName": "temporal-python",
    }
