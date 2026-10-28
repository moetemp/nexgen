"""The close policy of a resource at the end of `async with`. It stands in for the SDK's."""

from __future__ import annotations

import asyncio
import collections.abc
import types
import typing

import temporalio.exceptions
import temporalio.workflow

# The resources that Workflow code leaves open at the end of a block, by run id. The SDK closes
# them at the run's end, or keeps them open for a retry run.
_deferred_closes: dict[str, list[object]] = {}


def exit_close(
    resource: object, exc: BaseException | None
) -> tuple[bool, BaseException | None]:
    """Whether the end of a block closes the resource now, and with which failure."""
    if temporalio.workflow.in_workflow():
        run_id = temporalio.workflow.info().run_id
        _deferred_closes.setdefault(run_id, []).append(resource)
        return False, None
    if exc is None:
        return True, None
    # The Store records a canceled close, so a cancel becomes Temporal's CancelledError.
    if isinstance(exc, asyncio.CancelledError):
        return True, temporalio.exceptions.CancelledError(str(exc) or "canceled")
    if isinstance(exc, Exception):
        return True, exc
    # A `KeyboardInterrupt` or a `SystemExit` leaves the resource open.
    return False, None


def deferred_closes(run_id: str) -> list[object]:
    """The resources that the run's blocks left open."""
    return _deferred_closes.get(run_id, [])


class _Closable(typing.Protocol):
    async def __aenter__(self) -> object: ...

    async def __aexit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        tb: types.TracebackType | None,
    ) -> None: ...


_ResourceT = typing.TypeVar("_ResourceT", bound=_Closable)


@typing.final
class ResourceCall(typing.Generic[_ResourceT]):
    """A call that returns a resource. Await it for the resource, or enter it with `async with`,
    which closes the resource at the block's end.
    """

    def __init__(self, call: collections.abc.Awaitable[_ResourceT]) -> None:
        self._call: collections.abc.Awaitable[_ResourceT] = call
        self._resource: _ResourceT | None = None

    def __await__(self) -> collections.abc.Generator[typing.Any, None, _ResourceT]:
        return self._call.__await__()

    async def __aenter__(self) -> _ResourceT:
        resource = await self._call
        _ = await resource.__aenter__()
        self._resource = resource
        return resource

    async def __aexit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        tb: types.TracebackType | None,
    ) -> None:
        if self._resource is not None:
            await self._resource.__aexit__(exc_type, exc, tb)
