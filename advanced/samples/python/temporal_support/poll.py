"""The poll helpers of the stream-service and poll-service samples."""

import dataclasses
import typing


class _Request(typing.Protocol):
    __dataclass_fields__: typing.ClassVar[dict[str, dataclasses.Field[typing.Any]]]


class _ReadRequest(_Request, typing.Protocol):
    @property
    def from_offset(self) -> int | None: ...


class _ReadResponse(typing.Protocol):
    @property
    def next_offset(self) -> int | None: ...

    @property
    def done(self) -> bool | None: ...


class _PageResponse(typing.Protocol):
    @property
    def next_page_token(self) -> bytes | None: ...


_ReadRequestT = typing.TypeVar("_ReadRequestT", bound=_ReadRequest)
_RequestT = typing.TypeVar("_RequestT", bound=_Request)


def next_read_request(
    request: _ReadRequestT, response: _ReadResponse
) -> _ReadRequestT | None:
    if response.done:
        return None
    # An empty page has no next offset, so the next read starts at the same offset.
    from_offset = (
        request.from_offset if response.next_offset is None else response.next_offset
    )
    # `last` reads the newest Records once. The next reads continue from the offset.
    return dataclasses.replace(request, from_offset=from_offset, last=None)


def next_page_request(request: _RequestT, response: _PageResponse) -> _RequestT | None:
    if not response.next_page_token:
        return None
    return dataclasses.replace(request, next_page_token=response.next_page_token)


class _ReadItem(typing.Protocol):
    @property
    def offset(self) -> int | None: ...


def resume_read_request(request: _ReadRequestT, item: _ReadItem) -> _ReadRequestT:
    # A read that resumes after an item starts past its offset, and reads no `last` again.
    return dataclasses.replace(request, from_offset=(item.offset or 0) + 1, last=None)
