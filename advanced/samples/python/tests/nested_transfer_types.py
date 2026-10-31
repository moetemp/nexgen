"""A data converter that applies transfer types inside other values too, as sdk-python's
Streams fork does.

PyPI's ``temporalio`` applies the transfer type of a value at the top level of a payload only.
"""

from __future__ import annotations

import dataclasses
import inspect
import typing

import google.protobuf.json_format
import google.protobuf.message
import temporalio.converter
from temporalio.converter._payload_converter import _get_transfer_type_converter
from typing_extensions import override


class NestedTransferEncoder(temporalio.converter.AdvancedJSONEncoder):
    """Writes a value inside another value as its transfer type."""

    @override
    def default(self, o: typing.Any) -> typing.Any:
        converter = _get_transfer_type_converter(typing.cast(object, type(o)))
        if converter is None:
            return super().default(o)
        value = converter.to_transfer_type(o)
        if isinstance(value, google.protobuf.message.Message):
            return google.protobuf.json_format.MessageToDict(value)
        return typing.cast(object, value)


class NestedTransferTypeConverter(temporalio.converter.JSONTypeConverter):
    """Reads a value inside another value from its transfer type. A dataclass is written as its
    fields, so it has none there.
    """

    @override
    def to_typed_value(self, hint: type, value: typing.Any) -> typing.Any:
        origin = typing.get_origin(hint) or hint
        if not inspect.isclass(origin) or dataclasses.is_dataclass(origin):
            return temporalio.converter.JSONTypeConverter.Unhandled
        converter = _get_transfer_type_converter(typing.cast(object, origin))
        if converter is None:
            return temporalio.converter.JSONTypeConverter.Unhandled
        transfer_type = converter.transfer_type
        if inspect.isclass(transfer_type) and issubclass(
            transfer_type, google.protobuf.message.Message
        ):
            value = google.protobuf.json_format.ParseDict(
                value, transfer_type(), ignore_unknown_fields=True
            )
        return typing.cast(object, converter.from_transfer_type(value, hint))


class NestedPayloadConverter(temporalio.converter.CompositePayloadConverter):
    def __init__(self) -> None:
        json_converter = temporalio.converter.JSONPlainPayloadConverter(
            encoder=NestedTransferEncoder,
            custom_type_converters=[NestedTransferTypeConverter()],
        )
        super().__init__(
            *(
                json_converter
                if isinstance(converter, temporalio.converter.JSONPlainPayloadConverter)
                else converter
                for converter in temporalio.converter.DefaultPayloadConverter.default_encoding_payload_converters
            )
        )


NESTED_DATA_CONVERTER = dataclasses.replace(
    temporalio.converter.DataConverter.default,
    payload_converter_class=NestedPayloadConverter,
)
