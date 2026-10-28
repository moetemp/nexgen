from __future__ import annotations

import pytest
import temporalio.workflow

import wit.internal_service as internal_service
from wit.internal_service.models import FlushRequest
from wit.internal_service.operations.flush import flush


class FakeNexusClient:
    def __init__(self) -> None:
        self.inputs: list[object] = []

    async def start_operation(
        self, operation: str, input: object, *, output_type: object
    ) -> object:
        del operation, output_type
        self.inputs.append(input)
        return None


def test_an_internal_package_exports_nothing() -> None:
    # Only an SDK calls the package, through its modules.
    assert internal_service.__all__ == []
    assert not hasattr(internal_service, "flush")
    assert not hasattr(internal_service, "FlushRequest")


async def test_an_sdk_calls_an_internal_operation_through_its_module(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    nexus_client = FakeNexusClient()

    def create_nexus_client(**_: object) -> FakeNexusClient:
        return nexus_client

    monkeypatch.setattr(temporalio.workflow, "create_nexus_client", create_nexus_client)

    _ = await flush(writer_id="w1")

    assert nexus_client.inputs == [FlushRequest(writer_id="w1")]
