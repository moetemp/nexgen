"""A test double of the Streams SDK's Workflow read.

It does not read a Store.
"""

from __future__ import annotations

import typing

BodyT = typing.TypeVar("BodyT")

# The generated Workflow `Stream` calls the helper. A test reads each call.
calls: list[tuple[str, typing.Any, dict[str, typing.Any]]] = []

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
    _closed: bool

    def __init__(self, records: list[typing.Any]) -> None:
        self._records = records
        self._closed = False
        self.last_offset = None
        self.stop = None
        self.done = False

    def __aiter__(self) -> StreamReading[BodyT]:
        return self

    async def __anext__(self) -> typing.Any:
        if self._closed or self.stop is not None:
            raise StopAsyncIteration
        if not self._records:
            self.stop = "done"
            self.done = True
            raise StopAsyncIteration
        record = self._records.pop(0)
        self.last_offset = record.offset
        return record

    async def aclose(self) -> None:
        """End the read. A later step gives no Record, and `stop` stays None."""
        self._closed = True


def iter_stream(
    stream: typing.Any, **arguments: typing.Any
) -> StreamReading[typing.Any]:
    """Keep the call, and read the Records that a test queued."""
    calls.append(("iter_stream", stream, arguments))
    records = list(queued)
    queued.clear()
    return StreamReading(records)
