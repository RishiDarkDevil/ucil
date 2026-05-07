"""Scoring helpers for the python-project test fixture.

Provides ``compute_score`` — a small weighted-mean helper used by the
``refactor-rename-python`` UCIL effectiveness scenario to exercise the
rename-symbol-everywhere refactor flow.

Authorised by ADR DEC-0017 (effectiveness-scenario fixture augmentation).
"""

from __future__ import annotations

from typing import Iterable


def compute_score(values: Iterable[float], weights: Iterable[float]) -> float:
    """Return the weighted mean of ``values`` with corresponding ``weights``.

    Iterables are zipped pairwise; extras on either side are ignored.
    Returns ``0.0`` if the weights sum to zero (or both iterables are empty).

    >>> compute_score([10.0, 20.0, 30.0], [1.0, 1.0, 1.0])
    20.0
    >>> compute_score([1.0, 2.0], [3.0, 1.0])
    1.25
    >>> compute_score([], [])
    0.0
    >>> compute_score([5.0], [0.0])
    0.0
    """
    total = 0.0
    weight_sum = 0.0
    for value, weight in zip(values, weights):
        total += float(value) * float(weight)
        weight_sum += float(weight)
    if weight_sum == 0.0:
        return 0.0
    return total / weight_sum


def aggregate_scores(scored_items: Iterable[tuple[float, float]]) -> float:
    """Convenience wrapper: aggregate (value, weight) pairs into a single score.

    Calls :func:`compute_score` internally.
    """
    values: list[float] = []
    weights: list[float] = []
    for value, weight in scored_items:
        values.append(float(value))
        weights.append(float(weight))
    return compute_score(values, weights)
