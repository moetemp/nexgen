from __future__ import annotations

import uuid

from nexusrpc.handler import StartOperationContext, service_handler, sync_operation
from temporalio import workflow
from temporalio.testing import WorkflowEnvironment
from temporalio.worker import UnsandboxedWorkflowRunner, Worker

import wit.stream_writer as stream_writer
import wit.stream_writer.models as stream_writer_models
import wit.stream_writer.services as stream_writer_services


@service_handler(service=stream_writer_services.StreamWriter)
class StreamWriterHandler:
    def __init__(self) -> None:
        self.calls: list[tuple[str, object]] = []

    @sync_operation
    async def open_writer(
        self,
        _ctx: StartOperationContext,
        input: stream_writer_models.OpenWriterRequest,
    ) -> stream_writer_models.WriterResult:
        self.calls.append(("OpenWriter", input))
        return stream_writer_models.WriterResult(writer_id=7)

    @sync_operation
    async def close_writers(
        self,
        _ctx: StartOperationContext,
        input: stream_writer_models.CloseWritersRequest,
    ) -> stream_writer_models.CloseWritersResponse:
        self.calls.append(("CloseWriters", input))
        return stream_writer_models.CloseWritersResponse(failed=0)

    @sync_operation
    async def close_writer(
        self,
        _ctx: StartOperationContext,
        input: stream_writer_models.CloseWriterRequest,
    ) -> stream_writer_models.CloseWriterResponse:
        self.calls.append(("CloseWriter", input))
        return stream_writer_models.CloseWriterResponse(last_offset=3)


@workflow.defn
class StreamWriterCallerWorkflow:
    @workflow.run
    async def run(self) -> int | None:
        service = stream_writer.StreamWriterClient("stream-writer")
        writer = await service.open_writer(
            stream=stream_writer_models.StreamRef(id="s1")
        )
        closed = await writer.close()
        return closed.last_offset


async def test_a_client_with_no_endpoint_builds_the_returned_resource(
    env: WorkflowEnvironment,
) -> None:
    task_queue = str(uuid.uuid4())
    handler = StreamWriterHandler()

    async with Worker(
        env.client,
        task_queue=task_queue,
        workflows=[StreamWriterCallerWorkflow],
        nexus_service_handlers=[handler],
        workflow_runner=UnsandboxedWorkflowRunner(),
        # A missing import then fails the run at once, not each Workflow Task again.
        workflow_failure_exception_types=[NameError],
    ):
        endpoint = await env.create_nexus_endpoint("stream-writer", task_queue)
        try:
            last_offset = await env.client.execute_workflow(
                StreamWriterCallerWorkflow.run,
                id=str(uuid.uuid4()),
                task_queue=task_queue,
            )
        finally:
            await env.delete_nexus_endpoint(endpoint)

    assert last_offset == 3
    [(open_operation, open_request), (close_operation, close_request)] = handler.calls
    assert open_operation == "OpenWriter"
    assert isinstance(open_request, stream_writer_models.OpenWriterRequest)
    assert open_request.stream is not None
    assert open_request.stream.id == "s1"
    assert (close_operation, close_request) == (
        "CloseWriter",
        stream_writer_models.CloseWriterRequest(writer_id=7),
    )
