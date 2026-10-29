from __future__ import annotations

import typing

import pytest

import temporal_support

T = typing.TypeVar("T")


class Turn:
    pass


class Mixin(typing.Generic[T]):
    pass


class Turns(Mixin[int], temporal_support.StreamDefinition[Turn]):
    pass


class MoreTurns(Turns):
    pass


class OpenTurns(temporal_support.StreamDefinition[T]):
    pass


def test_a_definition_gives_the_type_arguments_of_its_definition_base() -> None:
    # The other generic base gives `int`, which isn't the value type.
    assert temporal_support.definition_type_arguments(Turns) == (Turn,)
    assert temporal_support.definition_type_arguments(MoreTurns) == (Turn,)


@pytest.mark.parametrize("definition", [temporal_support.StreamDefinition, OpenTurns])
def test_a_definition_with_no_type_argument_is_refused(
    definition: type[typing.Any],
) -> None:
    with pytest.raises(TypeError, match="gives no type argument"):
        _ = temporal_support.definition_type_arguments(definition)
