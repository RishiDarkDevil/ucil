"""
python_project — A mini expression language interpreter.

Public interface
----------------
- :class:`Lexer`       — tokenises source text.
- :class:`Parser`      — builds an AST from tokens.
- :class:`Evaluator`   — tree-walks the AST to produce a value.
- :class:`Environment` — lexically-scoped variable store.
- :class:`Token`       — a single lexical unit.
- :class:`TokenKind`   — enumeration of all token categories.
- :data:`Value`        — the runtime value type alias.
- :class:`LexError`    — raised on lexical errors.
- :class:`ParseError`  — raised on parse errors.
- :class:`EvalError`   — raised on runtime errors.
- Key AST node classes for introspection.

Quick usage::

    from python_project import Evaluator
    result = Evaluator().eval_source("let x = 6 in x * 7")
    assert result == 42
"""

from __future__ import annotations

from python_project.evaluator import Evaluator
from python_project.lexer import Lexer
from python_project.parser import Parser
from python_project.types import (
    ASTNode,
    BinaryOp,
    BoolLit,
    Call,
    Environment,
    EvalError,
    IfExpr,
    Identifier,
    LetExpr,
    LexError,
    NullLit,
    NumberLit,
    ParseError,
    Program,
    StringLit,
    Token,
    TokenKind,
    UnaryOp,
    Value,
    is_truthy,
    type_name,
    value_to_display,
    values_equal,
)

__all__ = [
    # Core pipeline
    "Lexer",
    "Parser",
    "Evaluator",
    # Environment
    "Environment",
    # Token types
    "Token",
    "TokenKind",
    # Value type alias
    "Value",
    # Errors
    "LexError",
    "ParseError",
    "EvalError",
    # AST nodes
    "ASTNode",
    "NumberLit",
    "StringLit",
    "BoolLit",
    "NullLit",
    "Identifier",
    "BinaryOp",
    "UnaryOp",
    "Call",
    "IfExpr",
    "LetExpr",
    "Program",
    # Helpers
    "is_truthy",
    "type_name",
    "value_to_display",
    "values_equal",
]

__version__ = "0.1.0"
