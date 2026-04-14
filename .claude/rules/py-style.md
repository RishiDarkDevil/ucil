# Python style rules (UCIL)

## Toolchain
- `uv` for env + deps management (`uv add`, `uv sync`, `uv run`).
- Python 3.11+.
- `pyproject.toml` per package; no `setup.py`.

## Lint & format
- `ruff format` (replaces black) and `ruff check --fix`.
- `mypy --strict` is the baseline.

## Tests
- `pytest` + `hypothesis` for property-based tests.
- `pytest-asyncio` for async.
- No mocks of ONNX Runtime / embedding models in the positive path; use a smaller test model instead.

## Types
- Every public function has type hints.
- `from __future__ import annotations` in every module.
- `Any` only at I/O boundaries with a narrowing comment.

## Errors
- Domain-specific `Exception` subclasses, never bare `Exception`.
- `raise ... from ...` to preserve cause chains.

## Logging
- `logging` with structured adapters (e.g., `structlog` if justified). No `print` in library code.

## Async
- `asyncio` with `TaskGroup` (3.11+).
- Timeouts on every network/IO await via `asyncio.timeout`.
