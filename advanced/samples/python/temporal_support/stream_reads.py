"""Test doubles of the Streams SDK's reads of many pages. They do not read a Store."""

from __future__ import annotations

import typing

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
