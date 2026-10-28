from __future__ import annotations

import collections.abc
import dataclasses
import types
import typing

import pytest
import temporalio.converter
import temporalio.workflow
from typing_extensions import override

import temporal_support
import wit.workflow_handle as workflow_handle
from wit.stream_service._resources import Stream
from wit.stream_service._resources.stream import WorkflowStreamService
from wit.stream_service.client import StreamServiceClientExtension
from wit.stream_service.models import (
    AppendRecord,
    AppendRecordsRequest,
    AppendRecordsResponse,
    CreateStreamRequest,
    CreateStreamResult,
    DescribeStreamRequest,
    DescribeStreamResponse,
    StreamRef,
)
from wit.user_service._resources import User
from wit.user_service.client import UserServiceClientExtension
from wit.user_service.models import (
    GetUserRequest,
    NotifyAllRequest,
    UpdateEmailRequest,
)
from wit.workflow_handle._resources import WorkflowHandle
from wit.workflow_handle.client import WorkflowHandleServiceClientExtension
from wit.workflow_handle.models import StartWorkflowRequest, StartWorkflowResult

STREAM_SERVICE = "temporal.api.streamservice.v1.StreamService"
USER_SERVICE = "UserService"


@dataclasses.dataclass
class Call:
    service: str
    endpoint: str | None
    operation: str
    input: object


class FakeNexusClient:
    def __init__(
        self, client: FakeClient | FakeUserClient, service: str, endpoint: str | None
    ) -> None:
        self._client: FakeClient | FakeUserClient = client
        self._service: str = service
        self._endpoint: str | None = endpoint

    async def start_operation(
        self,
        operation: str,
        input: typing.Any,
        *,
        output_type: type[typing.Any] | None = None,
    ) -> collections.abc.Awaitable[typing.Any]:
        del output_type
        self._client.calls.append(Call(self._service, self._endpoint, operation, input))
        answer = self._client.answers[operation].pop(0)

        async def result() -> object:
            return answer

        return result()


class FakeClient(
    StreamServiceClientExtension,
    WorkflowHandleServiceClientExtension,
):
    """Stands in for the SDK client, which inherits the extensions of several services."""

    def __init__(self, answers: dict[str, list[object]]) -> None:
        self.answers: dict[str, list[object]] = answers
        self.calls: list[Call] = []

    @override
    def _create_nexus_client(
        self,
        *,
        service: str,
        endpoint: str | None,
    ) -> FakeNexusClient:
        return FakeNexusClient(self, service, endpoint)


class FakeUserClient(UserServiceClientExtension):
    """Stands in for the SDK client of a service with no endpoint."""

    def __init__(self, answers: dict[str, list[object]]) -> None:
        self.answers: dict[str, list[object]] = answers
        self.calls: list[Call] = []

    @override
    def _create_nexus_client(
        self,
        *,
        service: str,
        endpoint: str | None,
    ) -> FakeNexusClient:
        return FakeNexusClient(self, service, endpoint)


async def test_a_created_stream_calls_its_operations_through_the_client() -> None:
    ref = StreamRef(id="s1")
    client = FakeClient(
        {
            "CreateStream": [CreateStreamResult(stream=ref, created=False)],
            "AppendRecords": [AppendRecordsResponse(first_offset=1, last_offset=2)],
        }
    )

    stream = await client.create_stream(stream_id="s1")
    appended = await stream.publish(records=[AppendRecord(body="hi")])

    # The response sets `created` to False, and the handle keeps it.
    assert stream.created is False
    assert appended.last_offset == 2
    assert client.calls == [
        Call(
            STREAM_SERVICE,
            "__temporal_system",
            "CreateStream",
            CreateStreamRequest(stream_id="s1"),
        ),
        Call(
            STREAM_SERVICE,
            "__temporal_system",
            "AppendRecords",
            AppendRecordsRequest(stream=ref, records=[AppendRecord(body="hi")]),
        ),
    ]


async def test_a_client_call_returns_the_result() -> None:
    client = FakeClient({"DescribeStream": [DescribeStreamResponse(last_offset=5)]})

    described = await client.describe_stream(stream=StreamRef(id="s1"))

    assert described.last_offset == 5


async def test_a_handle_built_by_hand_takes_the_current_client() -> None:
    ref = StreamRef(id="s1")
    client = FakeClient({"DescribeStream": [DescribeStreamResponse(last_offset=5)]})
    stream = Stream(stream=ref, created=None, status=None)

    with temporal_support.current_client(client):
        described = await stream.describe()

    assert described.last_offset == 5
    assert [call.input for call in client.calls] == [DescribeStreamRequest(stream=ref)]


def test_a_handle_in_workflow_code_takes_the_workflow_operations(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(temporalio.workflow, "in_workflow", lambda: True)
    stream = Stream(stream=StreamRef(id="s1"), created=None, status=None)

    assert isinstance(stream._ops(), WorkflowStreamService)


async def test_a_service_with_no_endpoint_is_left_to_the_client() -> None:
    client = FakeUserClient(
        {
            "GetUser": [User(user_id="u1", email="old@example.com")],
            "UpdateEmail": [User(user_id="u1", email="new@example.com")],
        }
    )

    user = await client.get_user(user_id="u1")
    updated = await user.update_email("new@example.com")

    assert updated.email == "new@example.com"
    assert client.calls == [
        Call(USER_SERVICE, None, "GetUser", GetUserRequest(user_id="u1")),
        Call(
            USER_SERVICE,
            None,
            "UpdateEmail",
            UpdateEmailRequest(user_id="u1", email="new@example.com"),
        ),
    ]


async def test_a_bound_resource_round_trips_through_the_default_converter() -> None:
    client = FakeUserClient({"GetUser": [User(user_id="u1", email="a@example.com")]})
    user = await client.get_user(user_id="u1")
    converter = temporalio.converter.PayloadConverter.default

    decoded = converter.from_payload(converter.to_payload(user), User)

    assert decoded == user
    assert user._operations is client
    assert decoded._operations is None


async def test_an_empty_request_takes_no_arguments() -> None:
    client = FakeUserClient({"NotifyAll": [None]})

    await client.notify_all()

    assert [call.input for call in client.calls] == [NotifyAllRequest()]


def test_a_client_class_cant_get_an_operation_from_two_bases() -> None:
    class Base:
        def create_stream(self) -> None: ...

    with pytest.raises(TypeError, match="gets create_stream from .*Base and from"):
        _ = types.new_class("Client", (Base, FakeClient))


async def test_a_package_with_only_the_client_target_takes_a_sourced_field() -> None:
    client = FakeClient({"StartWorkflow": [StartWorkflowResult(run_id="r1")]})

    # Outside a Workflow, the field that Workflow code fills from a source is a parameter.
    handle = await client.start_workflow(
        workflow="Greet", workflow_id="w1", task_queue="q", namespace="default"
    )

    assert handle == WorkflowHandle(namespace="default", workflow_id="w1", run_id="r1")
    assert [call.input for call in client.calls] == [
        StartWorkflowRequest(
            workflow="Greet", workflow_id="w1", task_queue="q", namespace="default"
        )
    ]


def test_a_package_with_only_the_client_target_has_its_serialization_contexts() -> None:
    info = workflow_handle.__nexus_operation_registry__[
        ("WorkflowHandleService", "StartWorkflow")
    ]
    request = StartWorkflowRequest(
        workflow="Greet", workflow_id="w1", task_queue="q", namespace="default"
    )

    assert info.serialization_context is not None
    assert info.serialization_context(
        request
    ) == temporalio.converter.WorkflowSerializationContext(
        namespace="default", workflow_id="w1"
    )
