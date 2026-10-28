"""Stream definitions, which name a Stream and the type of its values."""

from __future__ import annotations

import typing

ValueT = typing.TypeVar("ValueT")


class StreamDefinition(typing.Generic[ValueT]):
    """A subclass names the value type, as in ``class Turns(StreamDefinition[Turn])``."""

    name: typing.ClassVar[str] = ""


def definition_type_arguments(
    definition: type[typing.Any],
) -> tuple[type[typing.Any], ...]:
    """The type arguments that a definition class gives its generic base."""
    for base in typing.cast(
        tuple[object, ...], getattr(definition, "__orig_bases__", ())
    ):
        arguments = typing.get_args(base)
        if arguments:
            return typing.cast(tuple[type[typing.Any], ...], arguments)
    raise TypeError(f"{definition.__qualname__} gives no type argument to its base")
