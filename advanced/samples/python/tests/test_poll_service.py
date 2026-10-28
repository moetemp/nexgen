from __future__ import annotations

import collections.abc
import datetime
import typing
import uuid

from nexusrpc.handler import StartOperationContext, service_handler, sync_operation
from temporalio import workflow
from temporalio.testing import WorkflowEnvironment
from temporalio.worker import UnsandboxedWorkflowRunner, Worker
from typing_extensions import override

import wit.poll_service as poll_service
import wit.poll_service.services as poll_services
from wit.poll_service._resources import Feed
from wit.poll_service.client import (
    FeedServiceClientExtension,
    PageServiceClientExtension,
)
from wit.poll_service.models import (
    FeedOrder,
    ListWorkflowsRequest,
    ListWorkflowsResponse,
    OpenFeedRequest,
    WorkflowExecution,
    WorkflowExecutionInfo,
)


def info(workflow_id: str) -> WorkflowExecutionInfo:
    return WorkflowExecutionInfo(
        execution=WorkflowExecution(workflow_id=workflow_id, run_id="r1")
    )


# Each page names the next one, and the last page names none. The middle page is empty.
PAGES = {
    None: ListWorkflowsResponse(
        executions=[info("w1"), info("w2")], next_page_token=b"2"
    ),
    b"2": ListWorkflowsResponse(next_page_token=b"3"),
    b"3": ListWorkflowsResponse(executions=[info("w3")]),
}
ITEMS = [info("w1"), info("w2"), info("w3")]


def page(request: ListWorkflowsRequest) -> ListWorkflowsResponse:
    return PAGES[request.next_page_token or None]


class FakeNexusClient:
    def __init__(self, client: FakeClient, endpoint: str | None) -> None:
        self._client: FakeClient = client
        self._endpoint: str | None = endpoint

    async def start_operation(
        self,
        operation: str,
        input: typing.Any,
        *,
        output_type: type[typing.Any] | None = None,
    ) -> collections.abc.Awaitable[typing.Any]:
        del output_type
        self._client.calls.append((self._endpoint, operation, input))
        answer: object
        if isinstance(input, OpenFeedRequest):
            answer = Feed(namespace=input.namespace, query=input.query)
        else:
            answer = page(typing.cast(ListWorkflowsRequest, input))

        async def result() -> object:
            return answer

        return result()


class FakeClient(PageServiceClientExtension, FeedServiceClientExtension):
    def __init__(self) -> None:
        self.calls: list[tuple[str | None, str, object]] = []

    @override
    def _create_nexus_client(
        self,
        *,
        service: str,
        endpoint: str | None,
        rpc_metadata: collections.abc.Mapping[str, str | bytes] | None,
        rpc_timeout: datetime.timedelta | None,
    ) -> FakeNexusClient:
        del service, rpc_metadata, rpc_timeout
        return FakeNexusClient(self, endpoint)

    def tokens(self, operation: str) -> list[bytes | None]:
        return [
            typing.cast(ListWorkflowsRequest, request).next_page_token
            for _, called, request in self.calls
            if called == operation
        ]


async def test_a_client_polls_an_operation_until_the_helper_stops() -> None:
    client = FakeClient()

    pages = [
        response async for response in client.iter_list_workflows(namespace="default")
    ]

    assert pages == list(PAGES.values())
    assert client.tokens("ListWorkflows") == [None, b"2", b"3"]
    assert {endpoint for endpoint, _, _ in client.calls} == {"pages"}


async def test_a_client_polls_a_service_with_no_endpoint() -> None:
    client = FakeClient()

    read = client.iter_list_feed(namespace="default")
    items = [item async for item in read]

    # The feed's poll yields the executions of each page.
    assert items == ITEMS
    assert read.next_request is None
    assert client.tokens("ListFeed") == [None, b"2", b"3"]
    assert {endpoint for endpoint, _, _ in client.calls} == {None}


async def test_a_client_resource_polls_its_method() -> None:
    client = FakeClient()
    feed = await client.open_feed(
        namespace="default", query="q", order=FeedOrder.Oldest
    )

    items = [item async for item in feed.iter_read(page_size=10)]

    assert items == ITEMS
    assert client.calls[0][2] == OpenFeedRequest(
        namespace="default", query="q", order=FeedOrder.Oldest
    )
    assert client.calls[1][2] == ListWorkflowsRequest(
        namespace="default", page_size=10, query="q"
    )
    assert client.tokens("ListFeed") == [None, b"2", b"3"]


@service_handler(service=poll_services.PageService)
class PageServiceHandler:
    def __init__(self) -> None:
        self.tokens: list[bytes | None] = []

    @sync_operation
    async def list_workflows(
        self, _ctx: StartOperationContext, input: ListWorkflowsRequest
    ) -> ListWorkflowsResponse:
        self.tokens.append(input.next_page_token or None)
        return page(input)


@service_handler(service=poll_services.FeedService)
class FeedServiceHandler:
    def __init__(self) -> None:
        self.orders: list[FeedOrder | None] = []
        self.tokens: list[bytes | None] = []

    @sync_operation
    async def open_feed(
        self, _ctx: StartOperationContext, input: OpenFeedRequest
    ) -> Feed:
        self.orders.append(input.order)
        return Feed(namespace=input.namespace, query=input.query)

    @sync_operation
    async def list_feed(
        self, _ctx: StartOperationContext, input: ListWorkflowsRequest
    ) -> ListWorkflowsResponse:
        self.tokens.append(input.next_page_token or None)
        return page(input)


@workflow.defn
class PollWorkflow:
    @workflow.run
    async def run(self) -> list[int]:
        pages = [p async for p in poll_service.iter_list_workflows(namespace="default")]
        service = poll_service.FeedServiceClient("feeds")
        feed_items = [i async for i in service.iter_list_feed(namespace="default")]
        feed = await service.open_feed(
            namespace="default", query="q", order=FeedOrder.Oldest
        )
        read_items = [i async for i in feed.iter_read()]
        return [len(pages), len(feed_items), len(read_items)]


async def test_workflow_code_polls_each_kind_of_operation(
    env: WorkflowEnvironment,
) -> None:
    task_queue = str(uuid.uuid4())
    pages = PageServiceHandler()
    feeds = FeedServiceHandler()

    async with Worker(
        env.client,
        task_queue=task_queue,
        workflows=[PollWorkflow],
        nexus_service_handlers=[pages, feeds],
        workflow_runner=UnsandboxedWorkflowRunner(),
        # A broken iterator then fails the run at once, not each Workflow Task again.
        workflow_failure_exception_types=[NameError, TypeError, AttributeError],
    ):
        page_endpoint = await env.create_nexus_endpoint("pages", task_queue)
        feed_endpoint = await env.create_nexus_endpoint("feeds", task_queue)
        try:
            counts = await env.client.execute_workflow(
                PollWorkflow.run,
                id=str(uuid.uuid4()),
                task_queue=task_queue,
            )
        finally:
            await env.delete_nexus_endpoint(page_endpoint)
            await env.delete_nexus_endpoint(feed_endpoint)

    assert counts == [3, 3, 3]
    assert pages.tokens == [None, b"2", b"3"]
    assert feeds.orders == [FeedOrder.Oldest]
    assert feeds.tokens == [None, b"2", b"3"] * 2
