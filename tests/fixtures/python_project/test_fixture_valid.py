"""Validates that the python-project fixture is well-formed."""

from __future__ import annotations

import ast
import pathlib
import tomllib


FIXTURE_DIR = pathlib.Path(__file__).parent.parent / "python-project"


def test_fixture_directory_exists() -> None:
    assert FIXTURE_DIR.exists(), f"Fixture directory not found: {FIXTURE_DIR}"


def test_fixture_pyproject_valid_toml() -> None:
    pyproject = FIXTURE_DIR / "pyproject.toml"
    assert pyproject.exists(), "pyproject.toml missing from fixture"
    with open(pyproject, "rb") as f:
        data = tomllib.load(f)
    assert "project" in data
    assert "name" in data["project"]


def test_fixture_source_has_type_annotations() -> None:
    src_dir = FIXTURE_DIR / "src" / "python_project"
    assert src_dir.exists(), f"src/python_project not found at {src_dir}"
    py_files = list(src_dir.glob("*.py"))
    assert len(py_files) >= 1, "No Python files found in src/python_project"
    annotated_count = 0
    for py_file in py_files:
        source = py_file.read_text()
        try:
            tree = ast.parse(source)
        except SyntaxError:
            continue
        for node in ast.walk(tree):
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                if node.returns is not None or any(
                    a.annotation for a in node.args.args
                ):
                    annotated_count += 1
                    break
    assert annotated_count > 0, "No type annotations found in src/python_project/*.py"


def test_fixture_has_enough_source_lines() -> None:
    src_dir = FIXTURE_DIR / "src" / "python_project"
    total_lines = sum(
        len(py_file.read_text().splitlines()) for py_file in src_dir.glob("*.py")
    )
    assert total_lines >= 2000, f"Expected ≥2000 lines, found {total_lines}"
