"""
test_evaluator.py — pytest tests for the python_project Evaluator.

All tests exercise real evaluation behaviour.  No mocks or placeholders.
"""

from __future__ import annotations

import math

import pytest

from python_project.evaluator import Evaluator
from python_project.types import EvalError


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def ev(source: str) -> object:
    """Lex, parse, and evaluate ``source``, returning the result."""
    return Evaluator().eval_source(source)


# ---------------------------------------------------------------------------
# Arithmetic tests
# ---------------------------------------------------------------------------


def test_evaluate_arithmetic() -> None:
    """Evaluator correctly computes 2 + 3 * 4 = 14."""
    assert ev("2 + 3 * 4") == 14


def test_evaluate_subtraction() -> None:
    """Evaluator correctly computes 10 - 3 - 2 = 5 (left-associative)."""
    assert ev("10 - 3 - 2") == 5


def test_evaluate_division() -> None:
    """Evaluator performs true division and returns a float."""
    result = ev("7 / 2")
    assert isinstance(result, float)
    assert result == 3.5


def test_evaluate_modulo() -> None:
    """Evaluator computes modulo correctly."""
    assert ev("10 % 3") == 1
    assert ev("7 % 2") == 1


def test_evaluate_exponentiation() -> None:
    """Evaluator computes 2 ^ 10 = 1024."""
    assert ev("2 ^ 10") == 1024


def test_evaluate_right_associative_power() -> None:
    """Evaluator evaluates 2 ^ 3 ^ 2 as 2 ^ (3 ^ 2) = 512."""
    assert ev("2 ^ 3 ^ 2") == 512


def test_evaluate_unary_minus() -> None:
    """Evaluator handles unary negation."""
    assert ev("-5") == -5
    assert ev("--3") == 3
    assert ev("-3.14") == pytest.approx(-3.14)


def test_evaluate_parentheses_change_precedence() -> None:
    """Evaluator respects parentheses for grouping."""
    assert ev("(2 + 3) * 4") == 20
    assert ev("2 + (3 * 4)") == 14  # same as default precedence


def test_evaluate_division_by_zero_raises() -> None:
    """Evaluator raises EvalError on division by zero."""
    with pytest.raises(EvalError, match="Division by zero"):
        ev("1 / 0")


def test_evaluate_modulo_by_zero_raises() -> None:
    """Evaluator raises EvalError on modulo by zero."""
    with pytest.raises(EvalError, match="Modulo by zero"):
        ev("5 % 0")


def test_evaluate_float_arithmetic() -> None:
    """Evaluator handles mixed int/float arithmetic."""
    result = ev("1.5 + 2.5")
    assert result == pytest.approx(4.0)

    result2 = ev("3 * 1.5")
    assert result2 == pytest.approx(4.5)


# ---------------------------------------------------------------------------
# String operation tests
# ---------------------------------------------------------------------------


def test_evaluate_string_operations() -> None:
    """Evaluator evaluates upper, lower, and join on strings."""
    assert ev('upper("hello")') == "HELLO"
    assert ev('lower("WORLD")') == "world"
    assert ev('join(", ", ["a", "b", "c"])') == "a, b, c"


def test_evaluate_string_concatenation() -> None:
    """Evaluator concatenates strings with + operator."""
    assert ev('"hello" + " " + "world"') == "hello world"


def test_evaluate_string_length() -> None:
    """Evaluator computes the length of a string."""
    assert ev('len("hello")') == 5
    assert ev('len("")') == 0


def test_evaluate_string_split() -> None:
    """Evaluator splits a string into a list."""
    result = ev('split("a,b,c", ",")')
    assert result == ["a", "b", "c"]


def test_evaluate_string_strip() -> None:
    """Evaluator strips whitespace from strings."""
    assert ev('strip("  hello  ")') == "hello"
    assert ev('lstrip("  hi")') == "hi"
    assert ev('rstrip("hi  ")') == "hi"


def test_evaluate_string_replace() -> None:
    """Evaluator replaces substrings."""
    assert ev('replace("hello world", "world", "there")') == "hello there"


def test_evaluate_string_contains() -> None:
    """Evaluator checks substring membership."""
    assert ev('contains("hello world", "world")') is True
    assert ev('contains("hello world", "xyz")') is False


def test_evaluate_string_startswith_endswith() -> None:
    """Evaluator checks string prefix and suffix."""
    assert ev('startswith("hello", "he")') is True
    assert ev('endswith("hello", "lo")') is True
    assert ev('startswith("hello", "lo")') is False


def test_evaluate_string_find() -> None:
    """Evaluator finds the index of a substring."""
    assert ev('find("hello", "ll")') == 2
    assert ev('find("hello", "xyz")') == -1


def test_evaluate_string_repeat() -> None:
    """Evaluator repeats a string n times."""
    assert ev('repeat("ab", 3)') == "ababab"


def test_evaluate_string_count() -> None:
    """Evaluator counts occurrences of a substring."""
    assert ev('count("banana", "a")') == 3


# ---------------------------------------------------------------------------
# Conditional tests
# ---------------------------------------------------------------------------


def test_evaluate_conditionals() -> None:
    """Evaluator correctly evaluates if-then-else expressions."""
    assert ev("if true then 1 else 2") == 1
    assert ev("if false then 1 else 2") == 2
    assert ev("if 0 then 1 else 2") == 2  # 0 is falsy
    assert ev("if 1 then 1 else 2") == 1  # 1 is truthy


def test_evaluate_conditional_with_expression_condition() -> None:
    """Evaluator evaluates a condition that is itself an expression."""
    assert ev("if 3 > 2 then 100 else 0") == 100
    assert ev("if 1 > 2 then 100 else 0") == 0


def test_evaluate_nested_if() -> None:
    """Evaluator handles nested if expressions."""
    source = "if true then if false then 1 else 2 else 3"
    assert ev(source) == 2


def test_evaluate_if_short_circuits() -> None:
    """Evaluator only evaluates the taken branch of an if expression."""
    # The else branch divides by zero — should not be evaluated
    assert ev("if true then 42 else 1 / 0") == 42


# ---------------------------------------------------------------------------
# Let binding tests
# ---------------------------------------------------------------------------


def test_evaluate_let_binding() -> None:
    """Evaluator evaluates let x = 5 in x * 2 = 10."""
    assert ev("let x = 5 in x * 2") == 10


def test_evaluate_nested_let() -> None:
    """Evaluator handles nested let bindings with correct scoping."""
    assert ev("let x = 3 in let y = 4 in x * x + y * y") == 25


def test_evaluate_let_shadowing() -> None:
    """Inner let shadows outer binding within its body."""
    result = ev("let x = 1 in let x = 2 in x")
    assert result == 2


def test_evaluate_let_does_not_leak() -> None:
    """Let binding does not leak the variable outside its scope."""
    with pytest.raises(EvalError, match="Undefined variable"):
        ev("let x = 5 in x; x")


def test_evaluate_let_with_function_call() -> None:
    """Let binding can bind the result of a function call."""
    assert ev("let m = max(3, 7) in m + 1") == 8


def test_evaluate_let_complex_expression() -> None:
    """Let binding works with complex binding expressions."""
    assert ev("let v = 2 ^ 8 in v - 6") == 250


# ---------------------------------------------------------------------------
# Built-in function tests
# ---------------------------------------------------------------------------


def test_evaluate_builtin_functions() -> None:
    """Evaluator invokes abs, max, min, and len correctly."""
    assert ev("abs(-5)") == 5
    assert ev("abs(3.14)") == pytest.approx(3.14)
    assert ev("max(1, 2, 3)") == 3
    assert ev("min(10, 5, 8)") == 5
    assert ev('len("hello")') == 5
    assert ev("len([1, 2, 3])") == 3


def test_evaluate_builtin_round() -> None:
    """Evaluator rounds numbers correctly."""
    assert ev("round(3.7)") == 4
    assert ev("round(3.14159, 2)") == pytest.approx(3.14)


def test_evaluate_builtin_floor_ceil() -> None:
    """Evaluator computes floor and ceil correctly."""
    assert ev("floor(3.9)") == 3
    assert ev("ceil(3.1)") == 4
    assert ev("floor(-1.1)") == -2
    assert ev("ceil(-1.9)") == -1


def test_evaluate_builtin_sqrt() -> None:
    """Evaluator computes square roots."""
    assert ev("sqrt(9)") == pytest.approx(3.0)
    assert ev("sqrt(2)") == pytest.approx(math.sqrt(2))


def test_evaluate_builtin_sqrt_negative_raises() -> None:
    """Evaluator raises EvalError for sqrt of a negative number."""
    with pytest.raises(EvalError, match="non-negative"):
        ev("sqrt(-1)")


def test_evaluate_builtin_range() -> None:
    """Evaluator produces a list from range()."""
    assert ev("range(5)") == [0, 1, 2, 3, 4]
    assert ev("range(2, 6)") == [2, 3, 4, 5]
    assert ev("range(0, 10, 2)") == [0, 2, 4, 6, 8]


def test_evaluate_builtin_sum() -> None:
    """Evaluator sums a list of numbers."""
    assert ev("sum([1, 2, 3, 4, 5])") == 15
    assert ev("sum(range(5))") == 10


def test_evaluate_builtin_type() -> None:
    """Evaluator returns the type name of a value."""
    assert ev("type(42)") == "number"
    assert ev('type("hi")') == "string"
    assert ev("type(true)") == "boolean"
    assert ev("type(null)") == "null"
    assert ev("type([1, 2])") == "list"


def test_evaluate_builtin_bool_conversion() -> None:
    """Evaluator converts values to boolean via bool()."""
    assert ev("bool(1)") is True
    assert ev("bool(0)") is False
    assert ev('bool("")') is False
    assert ev('bool("x")') is True


def test_evaluate_builtin_int_conversion() -> None:
    """Evaluator converts values to int."""
    assert ev("int(3.9)") == 3
    assert ev('int("42")') == 42
    assert ev("int(true)") == 1
    assert ev("int(false)") == 0


def test_evaluate_builtin_float_conversion() -> None:
    """Evaluator converts values to float."""
    result = ev("float(3)")
    assert isinstance(result, float)
    assert result == 3.0


def test_evaluate_builtin_str_conversion() -> None:
    """Evaluator converts values to string."""
    assert ev("str(42)") == "42"
    assert ev("str(true)") == "true"
    assert ev("str(null)") == "null"


def test_evaluate_builtin_sorted() -> None:
    """Evaluator sorts a list."""
    assert ev("sorted([3, 1, 2])") == [1, 2, 3]
    assert ev("sorted([3, 1, 2], true)") == [3, 2, 1]


def test_evaluate_builtin_reversed() -> None:
    """Evaluator reverses a list."""
    assert ev("reversed([1, 2, 3])") == [3, 2, 1]


def test_evaluate_builtin_any_all() -> None:
    """Evaluator evaluates any() and all() on lists."""
    assert ev("any([false, false, true])") is True
    assert ev("any([false, false, false])") is False
    assert ev("all([true, true, true])") is True
    assert ev("all([true, false, true])") is False


def test_evaluate_builtin_enumerate() -> None:
    """Evaluator produces index-value pairs from enumerate()."""
    result = ev('enumerate(["a", "b", "c"])')
    assert result == [[0, "a"], [1, "b"], [2, "c"]]

    result2 = ev('enumerate(["x", "y"], 1)')
    assert result2 == [[1, "x"], [2, "y"]]


def test_evaluate_builtin_zip() -> None:
    """Evaluator zips two lists element-wise."""
    result = ev("zip([1, 2, 3], [4, 5, 6])")
    assert result == [[1, 4], [2, 5], [3, 6]]


def test_evaluate_builtin_index() -> None:
    """Evaluator finds the index of an item in a list."""
    assert ev("index([10, 20, 30], 20)") == 1


def test_evaluate_builtin_contains_list() -> None:
    """Evaluator checks membership in a list."""
    assert ev("contains([1, 2, 3], 2)") is True
    assert ev("contains([1, 2, 3], 9)") is False


def test_evaluate_builtin_count_in_list() -> None:
    """Evaluator counts occurrences of a value in a list."""
    assert ev("count([1, 2, 2, 3, 2], 2)") == 3


# ---------------------------------------------------------------------------
# Logical operator tests
# ---------------------------------------------------------------------------


def test_evaluate_logical_and_short_circuit() -> None:
    """Evaluator short-circuits 'and': if left is falsy, right not evaluated."""
    # right side would cause division by zero if evaluated
    result = ev("false and 1 / 0")
    assert result is False


def test_evaluate_logical_or_short_circuit() -> None:
    """Evaluator short-circuits 'or': if left is truthy, right not evaluated."""
    result = ev("true or 1 / 0")
    assert result is True


def test_evaluate_logical_not() -> None:
    """Evaluator inverts truthiness with 'not'."""
    assert ev("not true") is False
    assert ev("not false") is True
    assert ev("not 0") is True
    assert ev("not 1") is False


# ---------------------------------------------------------------------------
# Comparison tests
# ---------------------------------------------------------------------------


def test_evaluate_comparisons() -> None:
    """Evaluator evaluates all comparison operators correctly."""
    assert ev("1 < 2") is True
    assert ev("2 < 1") is False
    assert ev("2 <= 2") is True
    assert ev("3 > 2") is True
    assert ev("2 >= 3") is False
    assert ev("1 == 1") is True
    assert ev("1 != 2") is True
    assert ev("1 == 2") is False


def test_evaluate_string_comparison() -> None:
    """Evaluator compares strings lexicographically."""
    assert ev('"apple" < "banana"') is True
    assert ev('"zebra" > "ant"') is True
    assert ev('"hello" == "hello"') is True


def test_evaluate_equality_numeric_coercion() -> None:
    """Evaluator treats 1 == 1.0 as equal (numeric)."""
    assert ev("1 == 1.0") is True
    assert ev("3 == 3.0") is True


# ---------------------------------------------------------------------------
# Environment / scoping tests
# ---------------------------------------------------------------------------


def test_evaluator_define_and_lookup() -> None:
    """Evaluator.define() and lookup() work on the global environment."""
    e = Evaluator()
    e.define("answer", 42)
    assert e.lookup("answer") == 42


def test_evaluator_undefined_variable_raises() -> None:
    """Evaluator raises EvalError for undefined variable references."""
    with pytest.raises(EvalError, match="Undefined variable"):
        ev("undeclared_variable")


def test_evaluator_builtin_pi_constant() -> None:
    """Evaluator exposes math.pi as the constant 'pi'."""
    result = ev("pi")
    assert isinstance(result, float)
    assert result == pytest.approx(math.pi)


def test_evaluator_builtin_e_constant() -> None:
    """Evaluator exposes math.e as the constant 'e'."""
    result = ev("e")
    assert isinstance(result, float)
    assert result == pytest.approx(math.e)


# ---------------------------------------------------------------------------
# Multi-statement tests
# ---------------------------------------------------------------------------


def test_evaluate_semicolon_returns_last() -> None:
    """Evaluator returns the value of the last semicolon-separated expression."""
    assert ev("1; 2; 3") == 3
    assert ev("100; 200") == 200


def test_evaluate_let_then_use() -> None:
    """Evaluator evaluates sequential let expressions correctly."""
    # Separate statements; only the last is returned
    result = ev("let x = 10 in x * x")
    assert result == 100


# ---------------------------------------------------------------------------
# List tests
# ---------------------------------------------------------------------------


def test_evaluate_list_literal() -> None:
    """Evaluator creates a list from a literal."""
    result = ev("[1, 2, 3]")
    assert result == [1, 2, 3]


def test_evaluate_empty_list_literal() -> None:
    """Evaluator creates an empty list from []."""
    result = ev("[]")
    assert result == []


def test_evaluate_list_len() -> None:
    """Evaluator computes the length of a list literal."""
    assert ev("len([10, 20, 30])") == 3


def test_evaluate_list_sum() -> None:
    """Evaluator sums a list literal."""
    assert ev("sum([1, 2, 3, 4])") == 10


def test_evaluate_nested_list() -> None:
    """Evaluator handles list literals containing other list literals."""
    result = ev("[[1, 2], [3, 4]]")
    assert result == [[1, 2], [3, 4]]


# ---------------------------------------------------------------------------
# Type error tests
# ---------------------------------------------------------------------------


def test_evaluate_type_error_subtract_strings() -> None:
    """Evaluator raises EvalError when subtracting strings."""
    with pytest.raises(EvalError):
        ev('"hello" - "world"')


def test_evaluate_type_error_compare_number_string() -> None:
    """Evaluator raises EvalError when comparing number and string with <."""
    with pytest.raises(EvalError):
        ev('1 < "hello"')


def test_evaluate_call_non_function_raises() -> None:
    """Evaluator raises EvalError when calling a non-function value."""
    with pytest.raises(EvalError):
        ev("let f = 42 in f(1)")


def test_evaluate_wrong_arg_count_raises() -> None:
    """Evaluator raises EvalError when passing wrong number of args to built-in."""
    with pytest.raises(EvalError):
        ev("abs(1, 2)")


# ---------------------------------------------------------------------------
# Complex integration tests
# ---------------------------------------------------------------------------


def test_evaluate_fibonacci_via_let() -> None:
    """Evaluator can compute small Fibonacci values via nested let."""
    # a=0, b=1, c=a+b=1, d=b+c=2, e=c+d=3, f=d+e=5, g=e+f=8
    source = """
    let a = 0 in
    let b = 1 in
    let c = a + b in
    let d = b + c in
    let e = c + d in
    let f = d + e in
    let g = e + f in
    g
    """
    assert ev(source) == 8


def test_evaluate_circle_area() -> None:
    """Evaluator computes pi * r^2 using the built-in pi constant."""
    result = ev("pi * 5 ^ 2")
    assert result == pytest.approx(math.pi * 25)


def test_evaluate_complex_string_pipeline() -> None:
    """Evaluator chains string operations: upper then strip then len."""
    result = ev('len(strip(upper("  hello  ")))')
    assert result == 5  # "  HELLO  ".strip() = "HELLO" -> len 5


def test_evaluate_range_sum() -> None:
    """Evaluator computes sum of range(1, 101) = 5050."""
    assert ev("sum(range(1, 101))") == 5050


def test_evaluate_abs_in_let() -> None:
    """Evaluator uses abs() inside a let expression."""
    assert ev("let x = 0 - 7 in abs(x)") == 7
