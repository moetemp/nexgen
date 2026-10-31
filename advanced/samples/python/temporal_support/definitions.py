"""Definitions, which name a Stream and the type of its values."""

from __future__ import annotations

import typing

ValueT = typing.TypeVar("ValueT")


class StreamDefinition(typing.Generic[ValueT]):
    """A subclass names the value type, as in ``class Turns(StreamDefinition[Turn])``."""

    name: typing.ClassVar[str] = ""


def definition_type_arguments(
    definition: type[typing.Any],
) -> tuple[type[typing.Any], ...]:
    """The type arguments that a definition class gives its definition base, in order."""
    for cls in definition.__mro__:
        bases = typing.cast(tuple[object, ...], cls.__dict__.get("__orig_bases__", ()))
        for base in bases:
            if typing.get_origin(base) is not StreamDefinition:
                continue
            arguments = typing.get_args(base)
            if any(isinstance(argument, typing.TypeVar) for argument in arguments):
                break
            return typing.cast(tuple[type[typing.Any], ...], arguments)
    raise TypeError(f"{definition.__qualname__} gives no type argument to a definition")
