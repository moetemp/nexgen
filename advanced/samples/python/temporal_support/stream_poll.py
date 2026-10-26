"""The poll helper of the stream-service sample."""

from __future__ import annotations

import dataclasses
import typing


class _ReadRequest(typing.Protocol):
    __dataclass_fields__: typing.ClassVar[dict[str, dataclasses.Field[typing.Any]]]

    @property
    def from_offset(self) -> int | None: ...


class _ReadResponse(typing.Protocol):
    @property
    def next_offset(self) -> int | None: ...

    @property
    def done(self) -> bool | None: ...


_RequestT = typing.TypeVar("_RequestT", bound=_ReadRequest)


def next_read_request(request: _RequestT, response: _ReadResponse) -> _RequestT | None:
    """Continue a read where the last page ended, until a page says the stream is done."""
    if response.done:
        return None
    return dataclasses.replace(request, from_offset=response.next_offset)
