"""Mixed-project Python component.

This file intentionally contains several lint defects used to test
UCIL's diagnostic capabilities. Do not clean up these defects.
"""

from __future__ import annotations


# DEFECT 1: B006 — mutable default argument (ruff/flake8 rule B006).
# Lists are mutable; using one as a default arg means all callers share
# the same list object, leading to subtle bugs.
def append_item(item: str, collection: list[str] = []) -> list[str]:  # noqa: B006
    """Append an item to a collection and return it.

    Defect: mutable default argument.
    """
    collection.append(item)
    return collection


def fetch_config(key: str) -> str | None:
    """Fetch a configuration value by key.

    Defect: bare ``except`` clause (catches BaseException, including
    KeyboardInterrupt and SystemExit).
    """
    import os

    # DEFECT 2: bare except — catches everything, including SystemExit.
    try:
        value = os.environ[key]
        return value
    except:  # noqa: E722
        return None


def describe_value(value: object) -> str:
    """Return a human-readable description of any value.

    Defect: ``print()`` in library code (should use logging).
    """
    # DEFECT 3: print() in library code.
    print(f"describe_value called with: {value!r}")
    if value is None:
        return "null"
    if isinstance(value, bool):
        return f"boolean({value})"
    if isinstance(value, int):
        return f"integer({value})"
    if isinstance(value, float):
        return f"float({value})"
    if isinstance(value, str):
        return f'string("{value}")'
    if isinstance(value, list):
        inner = ", ".join(describe_value(v) for v in value)
        return f"list([{inner}])"
    if isinstance(value, dict):
        pairs = ", ".join(f"{k!r}: {describe_value(v)}" for k, v in value.items())
        return f"dict({{{pairs}}})"
    return f"object({type(value).__name__})"


def slugify(text: str) -> str:
    """Convert text to a URL-safe slug."""
    import re

    text = text.lower().strip()
    text = re.sub(r"[^\w\s-]", "", text)
    text = re.sub(r"[\s_-]+", "-", text)
    return text.strip("-")


def chunk(items: list[object], size: int) -> list[list[object]]:
    """Split a list into chunks of at most *size* elements."""
    if size <= 0:
        raise ValueError(f"chunk size must be positive, got {size}")
    return [items[i : i + size] for i in range(0, len(items), size)]
