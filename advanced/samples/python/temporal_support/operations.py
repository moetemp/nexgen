"""The operations of a resource handle that has none, for the client target.

The SDK's helper also gives the client of an Activity. The sample sets the client in a context.
"""

import collections.abc
import contextlib
import contextvars
import typing

import temporalio.workflow

_current_client: contextvars.ContextVar[object | None] = contextvars.ContextVar(
    "current_client", default=None
)


def current_operations(
    *, workflow: collections.abc.Callable[[], object] | None
) -> typing.Any:
    if workflow is not None and temporalio.workflow.in_workflow():
        return workflow()
    client = _current_client.get()
    if client is None:
        raise RuntimeError("no Temporal client for a handle with no operations")
    return client


@contextlib.contextmanager
def current_client(client: object) -> collections.abc.Iterator[None]:
    token = _current_client.set(client)
    try:
        yield
    finally:
        _current_client.reset(token)
