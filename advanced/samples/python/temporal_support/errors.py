"""The error details of a failed call. They stand in for the SDK's."""

from __future__ import annotations

import typing

import temporalio.exceptions

_DetailsT = typing.TypeVar("_DetailsT")


def error_details(
    err: BaseException, details_type: type[_DetailsT]
) -> _DetailsT | None:
    """The details of a failed call, from the error that the transport raised.

    A Nexus operation failure or an Activity error has the call's application error as its
    cause. Its details hold the model, or the proto message that the model converts from.
    """
    current: BaseException | None = err
    while current is not None:
        if isinstance(current, temporalio.exceptions.ApplicationError):
            for detail in current.details:
                if isinstance(detail, details_type):
                    return detail
                converter = getattr(
                    details_type, "__temporal_transfer_type_converter", None
                )
                transfer_type = getattr(converter, "transfer_type", None)
                if converter is not None and transfer_type is not None:
                    if isinstance(detail, transfer_type):
                        return typing.cast(
                            _DetailsT,
                            converter.from_transfer_type(detail, details_type),
                        )
        current = current.__cause__
    return None
