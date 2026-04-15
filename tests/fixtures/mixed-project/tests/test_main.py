"""Tests for the mixed-project Python component."""

from __future__ import annotations

import pytest

from src.main import describe_value, slugify, chunk


def test_describe_value_string() -> None:
    """describe_value handles string input."""
    result = describe_value("hello")
    assert result == 'string("hello")'


def test_slugify_basic() -> None:
    """slugify converts text to a URL-safe slug."""
    assert slugify("Hello World!") == "hello-world"
    assert slugify("  leading spaces  ") == "leading-spaces"


def test_chunk_splits_list() -> None:
    """chunk splits a list into fixed-size pieces."""
    result = chunk([1, 2, 3, 4, 5], 2)
    assert result == [[1, 2], [3, 4], [5]]


# INTENTIONALLY FAILING — pytest.mark.skip so CI does not run it.
# This test represents a "known broken" scenario the fixture documents.
@pytest.mark.skip(reason="Intentionally failing — fixture defect demonstration")
def test_intentionally_failing() -> None:
    """This test is intentionally failing."""
    raise AssertionError(
        "This test is intentionally failing. "
        "The mixed-project fixture contains broken tests by design."
    )
