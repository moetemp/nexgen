from __future__ import annotations

import collections.abc
import datetime
import typing

from typing_extensions import override

from wit.client_calls.client import JobServiceClientExtension
from wit.client_calls.models import (
    DescribeJobRequest,
    DescribeJobResponse,
    RunJobRequest,
    RunJobResponse,
)


class FakeNexusClient:
    def __init__(self, client: FakeClient) -> None:
        self._client: FakeClient = client

    async def start_operation(
        self,
        operation: str,
        input: typing.Any,
        *,
        output_type: type[typing.Any] | None = None,
    ) -> collections.abc.Awaitable[typing.Any]:
        del output_type
        self._client.started.append((operation, input))
        answer = self._client.answers[operation]

        async def result() -> object:
            self._client.awaited.append(operation)
            return answer

        return result()


class FakeClient(JobServiceClientExtension):
    def __init__(self, answers: dict[str, object]) -> None:
        self.answers: dict[str, object] = answers
        self.started: list[tuple[str, object]] = []
        self.awaited: list[str] = []
        self.rpc_options: list[
            tuple[
                collections.abc.Mapping[str, str | bytes] | None,
                datetime.timedelta | None,
            ]
        ] = []

    @override
    def _create_nexus_client(
        self,
        *,
        service: str,
        endpoint: str | None,
        rpc_metadata: collections.abc.Mapping[str, str | bytes] | None,
        rpc_timeout: datetime.timedelta | None,
    ) -> FakeNexusClient:
        del service, endpoint
        self.rpc_options.append((rpc_metadata, rpc_timeout))
        return FakeNexusClient(self)


async def test_a_client_starts_an_async_operation_and_waits_later() -> None:
    client = FakeClient({"RunJob": RunJobResponse(output="done")})

    handle = await client.start_run_job(name="nightly")

    # The start gives the handle before the result.
    assert client.started == [("RunJob", RunJobRequest(name="nightly"))]
    assert client.awaited == []
    assert await handle == RunJobResponse(output="done")


async def test_a_client_call_returns_the_result() -> None:
    client = FakeClient({"DescribeJob": DescribeJobResponse(status="running")})

    described = await client.describe_job(name="nightly")

    assert described == DescribeJobResponse(status="running")
    assert client.started == [("DescribeJob", DescribeJobRequest(name="nightly"))]


async def test_a_call_passes_its_rpc_options_to_the_client() -> None:
    client = FakeClient({"RunJob": RunJobResponse(output="done")})

    handle = await client.start_run_job(
        name="nightly",
        rpc_metadata={"authorization": "token"},
        rpc_timeout=datetime.timedelta(seconds=5),
    )
    _ = await handle

    assert client.rpc_options == [
        ({"authorization": "token"}, datetime.timedelta(seconds=5))
    ]


def test_only_an_async_operation_has_a_start_method() -> None:
    assert hasattr(JobServiceClientExtension, "start_run_job")
    assert not hasattr(JobServiceClientExtension, "start_describe_job")
