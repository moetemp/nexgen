"""The sample's close policy of a resource at the end of `async with`. The SDK supplies the
real one.
"""

from __future__ import annotations

import asyncio

import temporalio.workflow

# The resources that Workflow code leaves open at the end of a block, by run id. The sample only
# records them, and the SDK closes them at the run's end or keeps them open for a retry run.
_deferred_closes: dict[str, list[object]] = {}


def exit_close(resource: object, exc: BaseException | None) -> bool:
    """Whether the end of a block closes the resource now."""
    if temporalio.workflow.in_workflow():
        run_id = temporalio.workflow.info().run_id
        _deferred_closes.setdefault(run_id, []).append(resource)
        return False
    # An interrupt isn't a failure of the work, so the resource stays open for a retry.
    return exc is None or isinstance(exc, (Exception, asyncio.CancelledError))


def deferred_closes(run_id: str) -> list[object]:
    """The resources that the run's blocks left open."""
    return _deferred_closes.get(run_id, [])
