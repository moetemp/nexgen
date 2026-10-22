from __future__ import annotations

import datetime

import temporalio.converter
from temporalio.api.common.v1 import Payload

import wit.log_service.client as log_client
import wit.log_service.models as log_models

CONVERTER = temporalio.converter.DataConverter.default.payload_converter


async def test_iterator_reads_every_page_from_the_next_offset() -> None:
    pages = [
        log_models.ReadResponse(
            entries=[log_models.Entry(offset=1, text="a")], next_offset=2
        ),
        log_models.ReadResponse(entries=[], next_offset=2),
        log_models.ReadResponse(
            entries=[log_models.Entry(offset=2, text="b")], next_offset=3, done=True
        ),
    ]
    asked: list[int | None] = []

    class Transport:
        async def call(
            self, *, service: str, operation: str, input: Payload
        ) -> Payload:
            assert (service, operation) == ("LogService", "Read")
            request = CONVERTER.from_payload(input, log_models.ReadRequest)
            asked.append(request.from_offset)
            return CONVERTER.to_payload(pages.pop(0))

    client = log_client.LogServiceClient(Transport())
    request = log_models.ReadRequest(log_id="log")
    texts = [
        entry.text
        async for entry in client.iter_read(
            request, deadline=datetime.timedelta(seconds=5)
        )
    ]

    assert texts == ["a", "b"]
    assert asked == [None, 2, 2]
