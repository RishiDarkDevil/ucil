"""Tests for python_project.scoring (added per ADR DEC-0017)."""

from __future__ import annotations

import pytest

from python_project.scoring import aggregate_scores, compute_score


class TestComputeScore:
    def test_uniform_weights_returns_arithmetic_mean(self) -> None:
        assert compute_score([10.0, 20.0, 30.0], [1.0, 1.0, 1.0]) == 20.0

    def test_unequal_weights_returns_weighted_mean(self) -> None:
        # (1*3 + 2*1) / (3+1) = 5/4 = 1.25
        assert compute_score([1.0, 2.0], [3.0, 1.0]) == 1.25

    def test_empty_inputs_returns_zero(self) -> None:
        assert compute_score([], []) == 0.0

    def test_zero_total_weight_returns_zero(self) -> None:
        assert compute_score([5.0, 7.0], [0.0, 0.0]) == 0.0

    def test_extra_values_ignored(self) -> None:
        # Extra value (99) is dropped; only first two pairs counted.
        assert compute_score([1.0, 2.0, 99.0], [1.0, 1.0]) == 1.5

    def test_extra_weights_ignored(self) -> None:
        # Extra weight (99) is dropped; only first two pairs counted.
        assert compute_score([1.0, 2.0], [1.0, 1.0, 99.0]) == 1.5

    def test_negative_values_handled(self) -> None:
        assert compute_score([-10.0, 10.0], [1.0, 1.0]) == 0.0


class TestAggregateScores:
    def test_aggregates_pairs(self) -> None:
        result = aggregate_scores([(10.0, 1.0), (20.0, 1.0), (30.0, 1.0)])
        assert result == 20.0

    def test_empty_iterable_returns_zero(self) -> None:
        assert aggregate_scores([]) == 0.0


class TestRoundTripThroughPublicAPI:
    """Sanity-check that scoring is reachable via the package surface."""

    def test_import_via_module_path(self) -> None:
        from python_project import scoring

        assert hasattr(scoring, "compute_score")
        assert hasattr(scoring, "aggregate_scores")

    @pytest.mark.parametrize(
        "values,weights,expected",
        [
            ([1.0, 2.0, 3.0], [1.0, 1.0, 1.0], 2.0),
            ([100.0], [2.5], 100.0),
            ([0.0, 100.0], [1.0, 0.0], 0.0),
        ],
    )
    def test_property_examples(
        self, values: list[float], weights: list[float], expected: float
    ) -> None:
        assert compute_score(values, weights) == expected
