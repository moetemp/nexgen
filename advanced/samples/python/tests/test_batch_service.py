from __future__ import annotations

import pytest
import temporalio.workflow
from typing_extensions import assert_type

from wit.batch_service import batch
from wit.batch_service.models import AddItemsRequest, AddItemsResponse, BatchItem


class FakeNexusClient:
    def __init__(self, response: AddItemsResponse) -> None:
        self.response: AddItemsResponse = response
        self.inputs: list[object] = []

    async def start_operation(
        self, operation: str, input: object, *, output_type: object
    ) -> object:
        del operation, output_type
        self.inputs.append(input)

        async def result() -> AddItemsResponse:
            return self.response

        return result()


async def test_an_add_wraps_each_value_and_returns_one_field(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    nexus_client = FakeNexusClient(AddItemsResponse(last_item_id="i2", count=2))

    def create_nexus_client(**_: object) -> FakeNexusClient:
        return nexus_client

    monkeypatch.setattr(temporalio.workflow, "create_nexus_client", create_nexus_client)

    last_item_id = await batch(batch_id="b1").add_items("x", "y")

    _ = assert_type(last_item_id, str | None)
    assert last_item_id == "i2"
    assert nexus_client.inputs == [
        AddItemsRequest(
            batch_id="b1", items=[BatchItem(value="x"), BatchItem(value="y")]
        )
    ]


async def test_an_add_answers_none_for_a_field_with_presence(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    nexus_client = FakeNexusClient(AddItemsResponse(count=0))

    def create_nexus_client(**_: object) -> FakeNexusClient:
        return nexus_client

    monkeypatch.setattr(temporalio.workflow, "create_nexus_client", create_nexus_client)

    # A JSON field keeps its absence, so an empty add answers `None`, not "".
    assert await batch(batch_id="b1").add_items() is None
