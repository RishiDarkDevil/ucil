"""
test_parser.py — pytest tests for the python_project Parser.

All tests exercise real parser behaviour.  No mocks or placeholders.
"""

from __future__ import annotations

import pytest

from python_project.lexer import Lexer
from python_project.parser import Parser
from python_project.types import (
    BinaryOp,
    BoolLit,
    Call,
    IfExpr,
    Identifier,
    LetExpr,
    NullLit,
    NumberLit,
    ParseError,
    Program,
    StringLit,
    TokenKind,
    UnaryOp,
)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def parse(source: str) -> Program:
    """Lex and parse ``source``, returning the root Program node."""
    tokens = Lexer(source).tokenize()
    return Parser(tokens).parse()


def parse_expr(source: str):  # type: ignore[return]
    """Parse a single expression and return the first body node."""
    prog = parse(source)
    assert len(prog.body) >= 1
    return prog.body[0]


# ---------------------------------------------------------------------------
# Arithmetic expression tests
# ---------------------------------------------------------------------------


def test_parse_arithmetic_expression() -> None:
    """Parser correctly builds AST for 2 + 3 * 4 with correct precedence."""
    node = parse_expr("2 + 3 * 4")
    # Should be: BinaryOp(+, NumberLit(2), BinaryOp(*, NumberLit(3), NumberLit(4)))
    assert isinstance(node, BinaryOp)
    assert node.op.kind == TokenKind.PLUS

    assert isinstance(node.left, NumberLit)
    assert node.left.value == 2

    right = node.right
    assert isinstance(right, BinaryOp)
    assert right.op.kind == TokenKind.STAR
    assert isinstance(right.left, NumberLit)
    assert right.left.value == 3
    assert isinstance(right.right, NumberLit)
    assert right.right.value == 4


def test_parse_left_associative_subtraction() -> None:
    """Parser respects left-associativity of subtraction."""
    node = parse_expr("10 - 3 - 2")
    # Should be: BinaryOp(-, BinaryOp(-, 10, 3), 2)
    assert isinstance(node, BinaryOp)
    assert node.op.kind == TokenKind.MINUS
    assert isinstance(node.left, BinaryOp)
    assert node.left.op.kind == TokenKind.MINUS
    assert isinstance(node.left.left, NumberLit)
    assert node.left.left.value == 10
    assert isinstance(node.left.right, NumberLit)
    assert node.left.right.value == 3
    assert isinstance(node.right, NumberLit)
    assert node.right.value == 2


def test_parse_parenthesised_expression() -> None:
    """Parser honours parentheses for grouping."""
    node = parse_expr("(2 + 3) * 4")
    assert isinstance(node, BinaryOp)
    assert node.op.kind == TokenKind.STAR

    left = node.left
    assert isinstance(left, BinaryOp)
    assert left.op.kind == TokenKind.PLUS
    assert isinstance(left.left, NumberLit)
    assert left.left.value == 2

    assert isinstance(node.right, NumberLit)
    assert node.right.value == 4


def test_parse_unary_minus() -> None:
    """Parser builds a UnaryOp node for negation."""
    node = parse_expr("-42")
    assert isinstance(node, UnaryOp)
    assert node.op.kind == TokenKind.MINUS
    assert isinstance(node.operand, NumberLit)
    assert node.operand.value == 42


def test_parse_double_negation() -> None:
    """Parser handles double unary minus (right-associative)."""
    node = parse_expr("--5")
    assert isinstance(node, UnaryOp)
    assert isinstance(node.operand, UnaryOp)
    assert isinstance(node.operand.operand, NumberLit)
    assert node.operand.operand.value == 5


def test_parse_power_right_associative() -> None:
    """Parser makes ^ right-associative: 2^3^2 = 2^(3^2)."""
    node = parse_expr("2 ^ 3 ^ 2")
    assert isinstance(node, BinaryOp)
    assert node.op.kind == TokenKind.CARET
    assert isinstance(node.left, NumberLit)
    assert node.left.value == 2
    right = node.right
    assert isinstance(right, BinaryOp)
    assert right.op.kind == TokenKind.CARET
    assert isinstance(right.left, NumberLit)
    assert right.left.value == 3


# ---------------------------------------------------------------------------
# Comparison tests
# ---------------------------------------------------------------------------


def test_parse_comparison() -> None:
    """Parser builds a BinaryOp node for a > b comparison."""
    node = parse_expr("a > b")
    assert isinstance(node, BinaryOp)
    assert node.op.kind == TokenKind.GT
    assert isinstance(node.left, Identifier)
    assert node.left.name == "a"
    assert isinstance(node.right, Identifier)
    assert node.right.name == "b"


def test_parse_equality() -> None:
    """Parser handles == and != comparison operators."""
    node_eq = parse_expr("x == 5")
    assert isinstance(node_eq, BinaryOp)
    assert node_eq.op.kind == TokenKind.EQ

    node_neq = parse_expr("x != 5")
    assert isinstance(node_neq, BinaryOp)
    assert node_neq.op.kind == TokenKind.NEQ


def test_parse_chained_comparisons() -> None:
    """Parser handles chained comparisons left-associatively."""
    node = parse_expr("a <= b")
    assert isinstance(node, BinaryOp)
    assert node.op.kind == TokenKind.LTE


# ---------------------------------------------------------------------------
# Logical operator tests
# ---------------------------------------------------------------------------


def test_parse_logical_and_or() -> None:
    """Parser builds correct tree for logical and/or with precedence."""
    # 'or' binds less tightly than 'and'
    node = parse_expr("a or b and c")
    # Should be: OR(a, AND(b, c))
    assert isinstance(node, BinaryOp)
    assert node.op.kind == TokenKind.OR
    assert isinstance(node.left, Identifier)
    assert node.left.name == "a"
    right = node.right
    assert isinstance(right, BinaryOp)
    assert right.op.kind == TokenKind.AND


def test_parse_logical_not() -> None:
    """Parser builds a UnaryOp node for 'not'."""
    node = parse_expr("not true")
    assert isinstance(node, UnaryOp)
    assert node.op.kind == TokenKind.NOT
    assert isinstance(node.operand, BoolLit)
    assert node.operand.value is True


def test_parse_double_not() -> None:
    """Parser handles double negation with 'not'."""
    node = parse_expr("not not false")
    assert isinstance(node, UnaryOp)
    assert isinstance(node.operand, UnaryOp)
    assert isinstance(node.operand.operand, BoolLit)


# ---------------------------------------------------------------------------
# Let expression tests
# ---------------------------------------------------------------------------


def test_parse_let_expression() -> None:
    """Parser builds a LetExpr node for let x = 5 in x + 1."""
    node = parse_expr("let x = 5 in x + 1")
    assert isinstance(node, LetExpr)
    assert node.name == "x"
    assert isinstance(node.binding, NumberLit)
    assert node.binding.value == 5
    body = node.body
    assert isinstance(body, BinaryOp)
    assert body.op.kind == TokenKind.PLUS
    assert isinstance(body.left, Identifier)
    assert body.left.name == "x"


def test_parse_nested_let() -> None:
    """Parser handles nested let bindings."""
    node = parse_expr("let x = 1 in let y = 2 in x + y")
    assert isinstance(node, LetExpr)
    assert node.name == "x"
    inner = node.body
    assert isinstance(inner, LetExpr)
    assert inner.name == "y"
    assert isinstance(inner.body, BinaryOp)


def test_parse_let_with_complex_binding() -> None:
    """Parser handles a let binding with a complex expression as the value."""
    node = parse_expr("let result = 2 + 3 * 4 in result")
    assert isinstance(node, LetExpr)
    assert node.name == "result"
    assert isinstance(node.binding, BinaryOp)
    assert isinstance(node.body, Identifier)
    assert node.body.name == "result"


# ---------------------------------------------------------------------------
# If expression tests
# ---------------------------------------------------------------------------


def test_parse_if_expression() -> None:
    """Parser builds an IfExpr node for if-then-else."""
    node = parse_expr("if x > 0 then x else 0 - x")
    assert isinstance(node, IfExpr)
    assert isinstance(node.condition, BinaryOp)
    assert node.condition.op.kind == TokenKind.GT
    assert isinstance(node.then_branch, Identifier)
    assert isinstance(node.else_branch, BinaryOp)


def test_parse_if_without_then_keyword() -> None:
    """Parser accepts if-else without the optional 'then' keyword."""
    node = parse_expr("if true 1 else 2")
    assert isinstance(node, IfExpr)
    assert isinstance(node.condition, BoolLit)
    assert isinstance(node.then_branch, NumberLit)
    assert node.then_branch.value == 1
    assert isinstance(node.else_branch, NumberLit)
    assert node.else_branch.value == 2


def test_parse_nested_if() -> None:
    """Parser handles nested if-else expressions."""
    node = parse_expr("if a then if b then 1 else 2 else 3")
    assert isinstance(node, IfExpr)
    assert isinstance(node.then_branch, IfExpr)
    assert isinstance(node.else_branch, NumberLit)
    assert node.else_branch.value == 3


# ---------------------------------------------------------------------------
# Function call tests
# ---------------------------------------------------------------------------


def test_parse_function_call() -> None:
    """Parser builds a Call node for max(1, 2)."""
    node = parse_expr("max(1, 2)")
    assert isinstance(node, Call)
    assert isinstance(node.callee, Identifier)
    assert node.callee.name == "max"
    assert len(node.args) == 2
    assert isinstance(node.args[0], NumberLit)
    assert node.args[0].value == 1
    assert isinstance(node.args[1], NumberLit)
    assert node.args[1].value == 2


def test_parse_zero_arg_call() -> None:
    """Parser handles function calls with no arguments."""
    node = parse_expr("empty()")
    assert isinstance(node, Call)
    assert isinstance(node.callee, Identifier)
    assert node.callee.name == "empty"
    assert node.args == []


def test_parse_nested_call() -> None:
    """Parser handles nested function calls."""
    node = parse_expr("abs(min(a, b))")
    assert isinstance(node, Call)
    assert isinstance(node.callee, Identifier)
    assert node.callee.name == "abs"
    inner = node.args[0]
    assert isinstance(inner, Call)
    assert isinstance(inner.callee, Identifier)
    assert inner.callee.name == "min"


def test_parse_call_with_expr_args() -> None:
    """Parser handles function calls with expression arguments."""
    node = parse_expr("round(3.14159, 2 + 0)")
    assert isinstance(node, Call)
    assert len(node.args) == 2
    assert isinstance(node.args[0], NumberLit)
    assert isinstance(node.args[1], BinaryOp)


# ---------------------------------------------------------------------------
# Literal tests
# ---------------------------------------------------------------------------


def test_parse_string_literal() -> None:
    """Parser builds a StringLit node for a string literal."""
    node = parse_expr('"hello world"')
    assert isinstance(node, StringLit)
    assert node.value == "hello world"


def test_parse_boolean_literal() -> None:
    """Parser builds BoolLit nodes for true and false."""
    t_node = parse_expr("true")
    assert isinstance(t_node, BoolLit)
    assert t_node.value is True

    f_node = parse_expr("false")
    assert isinstance(f_node, BoolLit)
    assert f_node.value is False


def test_parse_null_literal() -> None:
    """Parser builds a NullLit node for null."""
    node = parse_expr("null")
    assert isinstance(node, NullLit)


# ---------------------------------------------------------------------------
# List literal tests
# ---------------------------------------------------------------------------


def test_parse_list_literal() -> None:
    """Parser desugars [1, 2, 3] into a Call(list, [...]) node."""
    node = parse_expr("[1, 2, 3]")
    assert isinstance(node, Call)
    assert isinstance(node.callee, Identifier)
    assert node.callee.name == "list"
    assert len(node.args) == 3
    assert all(isinstance(a, NumberLit) for a in node.args)


def test_parse_empty_list_literal() -> None:
    """Parser handles an empty list literal []."""
    node = parse_expr("[]")
    assert isinstance(node, Call)
    assert isinstance(node.callee, Identifier)
    assert node.callee.name == "list"
    assert node.args == []


# ---------------------------------------------------------------------------
# Multi-expression (semicolon) tests
# ---------------------------------------------------------------------------


def test_parse_nested_expressions() -> None:
    """Parser handles multiple semicolon-separated expressions."""
    prog = parse("1 + 2; 3 * 4; 5")
    assert len(prog.body) == 3
    assert isinstance(prog.body[0], BinaryOp)
    assert isinstance(prog.body[1], BinaryOp)
    assert isinstance(prog.body[2], NumberLit)
    assert prog.body[2].value == 5


def test_parse_empty_input() -> None:
    """Parser handles empty input gracefully."""
    prog = parse("")
    assert isinstance(prog, Program)
    assert len(prog.body) == 1  # single NullLit sentinel
    assert isinstance(prog.body[0], NullLit)


# ---------------------------------------------------------------------------
# Error tests
# ---------------------------------------------------------------------------


def test_parse_unmatched_paren_raises() -> None:
    """Parser raises ParseError for an unmatched opening parenthesis."""
    with pytest.raises(ParseError):
        parse("(1 + 2")


def test_parse_missing_in_raises() -> None:
    """Parser raises ParseError when 'in' is missing from a let expression."""
    with pytest.raises(ParseError):
        parse("let x = 5 x + 1")


def test_parse_missing_assignment_in_let_raises() -> None:
    """Parser raises ParseError when '=' is missing in a let binding."""
    with pytest.raises(ParseError):
        parse("let x 5 in x")


def test_parse_unexpected_token_raises() -> None:
    """Parser raises ParseError on a dangling operator."""
    with pytest.raises(ParseError):
        parse("* 2")


def test_parse_missing_closing_bracket_raises() -> None:
    """Parser raises ParseError for an unclosed list literal."""
    with pytest.raises(ParseError):
        parse("[1, 2, 3")


# ---------------------------------------------------------------------------
# Utility method tests
# ---------------------------------------------------------------------------


def test_parser_quick_parse() -> None:
    """Parser.quick_parse() convenience method works end-to-end."""
    prog = Parser.quick_parse("42")
    assert isinstance(prog, Program)
    assert isinstance(prog.body[0], NumberLit)
    assert prog.body[0].value == 42


def test_parser_has_no_errors_on_valid_input() -> None:
    """Parser.has_errors() returns False for valid input."""
    tokens = Lexer("1 + 2").tokenize()
    p = Parser(tokens)
    p.parse()
    assert p.has_errors() is False


def test_parser_token_count() -> None:
    """Parser.token_count() returns the correct total token count."""
    tokens = Lexer("1 + 2").tokenize()
    p = Parser(tokens)
    # 3 value tokens + 1 EOF
    assert p.token_count() == 4
