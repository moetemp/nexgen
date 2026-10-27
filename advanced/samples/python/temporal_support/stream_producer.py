"""The stream-service sample's writer, from where a call runs."""

from __future__ import annotations

import importlib
import typing

import temporalio.activity
import temporalio.workflow


def workflow_producer() -> typing.Any:
    """The Workflow run that writes."""
    models = _stream_service_models()
    info = temporalio.workflow.info()
    return models.Producer(
        kind=models.ProducerKind.ProducerKindWorkflow,
        id=info.workflow_id,
        run_id=info.run_id,
    )


def client_producer() -> typing.Any:
    """The Activity attempt that writes, or the client when no Activity runs."""
    models = _stream_service_models()
    if temporalio.activity.in_activity():
        info = temporalio.activity.info()
        return models.Producer(
            kind=models.ProducerKind.ProducerKindActivity,
            id=info.activity_id,
            run_id=info.workflow_run_id,
        )
    return models.Producer(kind=models.ProducerKind.ProducerKindClient, id="client")


def _stream_service_models() -> typing.Any:
    # By name on call, since the generated package imports this support package.
    return importlib.import_module("wit.stream_service.models")
