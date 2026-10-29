"""The sample's version of the SDK helper that reads the error details of a failed call."""

from __future__ import annotations

import typing

import temporalio.exceptions

_DetailsT = typing.TypeVar("_DetailsT")


def error_details(
    err: BaseException, details_type: type[_DetailsT]
) -> tuple[_DetailsT, str] | None:
    """The details of a failed call, and the message of the error that holds them.

    A Nexus operation failure or an Activity error has the call's application error as its
    cause. Its details hold the model, or the proto message that the model converts from.
    """
    current: BaseException | None = err
    while current is not None:
        if isinstance(current, temporalio.exceptions.ApplicationError):
            for detail in current.details:
                if isinstance(detail, details_type):
                    return detail, current.message
                converter = getattr(
                    details_type, "__temporal_transfer_type_converter", None
                )
                transfer_type = getattr(converter, "transfer_type", None)
                if converter is not None and transfer_type is not None:
                    if isinstance(detail, transfer_type):
                        details = typing.cast(
                            _DetailsT,
                            converter.from_transfer_type(detail, details_type),
                        )
                        return details, current.message
        current = current.__cause__
    return None
