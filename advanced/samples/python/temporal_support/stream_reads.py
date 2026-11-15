"""Test doubles of the Streams SDK's reads and of its call guard.

They do not read a Store.
"""

from __future__ import annotations

import typing

BodyT = typing.TypeVar("BodyT")

# The generated `Stream` of each target calls the same helper. A test sets each result, and it
# reads each call.
calls: list[tuple[str, typing.Any, dict[str, typing.Any]]] = []
results: dict[str, typing.Any] = {}


def _call(
    helper: str, stream: typing.Any, arguments: dict[str, typing.Any]
) -> typing.Any:
    calls.append((helper, stream, arguments))
    return results[helper]


async def take_stream(stream: typing.Any, **arguments: typing.Any) -> typing.Any:
    """Keep the call, and return the result that a test set."""
    return _call("take_stream", stream, arguments)


async def filter_stream(stream: typing.Any, **arguments: typing.Any) -> typing.Any:
    """Keep the call, and return the result that a test set."""
    return _call("filter_stream", stream, arguments)


async def reduce_stream(stream: typing.Any, **arguments: typing.Any) -> typing.Any:
    """Keep the call, and return the result that a test set."""
    return _call("reduce_stream", stream, arguments)


# A test queues the Records that the next read gives.
queued: list[typing.Any] = []


class StreamReading(typing.Generic[BodyT]):
    """One read that gives each queued Record, then ends as if the Stream closed.

    After the loop, `last_offset`, `stop` and `done` tell where and why it ended.
    """

    last_offset: bytes | None
    stop: str | None
    done: bool
    _records: list[typing.Any]

    def __init__(self, records: list[typing.Any]) -> None:
        self._records = records
        self.last_offset = None
        self.stop = None
        self.done = False

    def __aiter__(self) -> StreamReading[BodyT]:
        return self

    async def __anext__(self) -> typing.Any:
        if self.stop is not None:
            raise StopAsyncIteration
        if not self._records:
            self.stop = "done"
            self.done = True
            raise StopAsyncIteration
        record = self._records.pop(0)
        self.last_offset = record.offset
        return record

    async def aclose(self) -> None:
        """End the read. A later step of the loop gives no Record."""
        if self.stop is None:
            self.stop = "closed"


def iter_stream(
    stream: typing.Any, **arguments: typing.Any
) -> StreamReading[typing.Any]:
    """Keep the call, and read the Records that a test queued."""
    calls.append(("iter_stream", stream, arguments))
    records = list(queued)
    queued.clear()
    return StreamReading(records)


# The generated `Stream` of each target calls the guard when code takes a method. A test reads
# each call, and it sets the flag while a plain function of a caller runs.
guard_calls: list[tuple[str, str]] = []
in_caller_function = False


def check_stream_call(*, target: str, method: str) -> None:
    """Keep the call, and refuse it while a plain function of a caller runs."""
    guard_calls.append((target, method))
    if in_caller_function:
        raise TypeError(
            f"A plain function of a caller can't call `{method}` of a {target} Stream"
        )
