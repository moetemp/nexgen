"""Sources of the stream sample. They stand in for the ones of Brook's runtime."""

from __future__ import annotations

import importlib
import typing

import temporalio.activity
import temporalio.workflow


def _producer(producer_id: str, run_id: str | None = None) -> typing.Any:
    # The sample's models import the support package, so this finds them when it runs.
    models = importlib.import_module("wit.stream_service.models")
    return models.Producer(id=producer_id, run_id=run_id)


def workflow_stream_id(definition: type[typing.Any]) -> str:
    """The id of the Workflow's own Stream of this definition."""
    info = temporalio.workflow.info()
    name = typing.cast(str, getattr(definition, "name", ""))
    return f"{info.workflow_id}:{info.first_execution_run_id}:{name}"


def workflow_owner() -> typing.Any:
    run_id = temporalio.workflow.info().run_id
    return _producer(f"wf:{run_id}", run_id)


def workflow_producer() -> typing.Any:
    run_id = temporalio.workflow.info().run_id
    return _producer(f"wf:{run_id}", run_id)


def client_producer() -> typing.Any:
    """An Activity's attempt, or a client."""
    if temporalio.activity.in_activity():
        info = temporalio.activity.info()
        return _producer(
            f"act:{info.workflow_run_id}:{info.activity_id}", info.workflow_run_id
        )
    return _producer("client:sample")


def local_activity_failed_tasks() -> list[int]:
    return []


def local_activity_previous_completed_task() -> typing.Any:
    return None
