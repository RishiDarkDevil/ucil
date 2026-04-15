"""
types.py — AST node types, token definitions, environment, and error classes
for the python_project expression language interpreter.

This module defines the complete type hierarchy used across the lexer, parser,
and evaluator. Every public class and function has full type hints and docstrings.
"""

from __future__ import annotations

import enum
from dataclasses import dataclass, field
from typing import Union


# ---------------------------------------------------------------------------
# Value type alias
# ---------------------------------------------------------------------------

Value = Union[int, float, str, bool, None, list]
"""
The set of runtime values the evaluator can produce.

Supported types:
  - int     — integer arithmetic result
  - float   — floating-point arithmetic result
  - str     — string value
  - bool    — boolean (True / False)
  - None    — null / absence of value
  - list    — ordered sequence (produced by range/map/filter/zip)
"""


# ---------------------------------------------------------------------------
# Exception hierarchy
# ---------------------------------------------------------------------------


class UCILError(Exception):
    """
    Base class for all python_project interpreter errors.

    Provides a unified exception root so callers can catch a single type
    when they do not care about the specific error kind.
    """

    def __init__(self, message: str, line: int = 0, col: int = 0) -> None:
        """
        Initialise a UCILError.

        Args:
            message: Human-readable description of the error.
            line:    1-based source line where the error occurred (0 if unknown).
            col:     1-based column position (0 if unknown).
        """
        super().__init__(message)
        self.line = line
        self.col = col

    def __str__(self) -> str:
        """Return a formatted string including source position if available."""
        if self.line > 0:
            return f"[{self.line}:{self.col}] {self.args[0]}"
        return self.args[0]


class LexError(UCILError):
    """
    Raised by the Lexer when it encounters an unexpected character or
    malformed token (e.g. unterminated string literal, bad escape sequence).

    Example::

        raise LexError("Unterminated string literal", line=3, col=5)
    """

    def __init__(self, message: str, line: int = 0, col: int = 0) -> None:
        """
        Initialise a LexError.

        Args:
            message: Description of the lexical error.
            line:    Source line (1-based).
            col:     Column position (1-based).
        """
        super().__init__(message, line, col)


class ParseError(UCILError):
    """
    Raised by the Parser when the token stream does not conform to the grammar.

    Common causes:
      - Missing closing parenthesis
      - Unexpected token in expression position
      - Malformed let / if expression

    Example::

        raise ParseError("Expected ')' after argument list", line=7, col=12)
    """

    def __init__(self, message: str, line: int = 0, col: int = 0) -> None:
        """
        Initialise a ParseError.

        Args:
            message: Description of the parse error.
            line:    Source line (1-based).
            col:     Column position (1-based).
        """
        super().__init__(message, line, col)


class EvalError(UCILError):
    """
    Raised by the Evaluator when runtime semantics are violated.

    Common causes:
      - Division by zero
      - Type mismatch in a binary operation
      - Reference to an undefined variable
      - Wrong number of arguments to a built-in function

    Example::

        raise EvalError("Division by zero", line=2, col=8)
    """

    def __init__(self, message: str, line: int = 0, col: int = 0) -> None:
        """
        Initialise an EvalError.

        Args:
            message: Description of the evaluation error.
            line:    Source line (1-based).
            col:     Column position (1-based).
        """
        super().__init__(message, line, col)


# ---------------------------------------------------------------------------
# Token kinds
# ---------------------------------------------------------------------------


class TokenKind(enum.Enum):
    """
    Enumeration of every token kind recognised by the lexer.

    Literal tokens
    --------------
    NUMBER      — integer or floating-point numeric literal.
    STRING      — string literal (double or single quoted).
    BOOLEAN     — ``true`` or ``false`` keyword.
    NULL        — ``null`` keyword.

    Name tokens
    -----------
    IDENTIFIER  — user-defined name (variable, function).

    Arithmetic operators
    --------------------
    PLUS        — ``+`` binary/unary addition.
    MINUS       — ``-`` binary/unary subtraction / negation.
    STAR        — ``*`` multiplication.
    SLASH       — ``/`` floating-point division.
    PERCENT     — ``%`` modulo / remainder.
    CARET       — ``^`` exponentiation.

    Grouping and separators
    -----------------------
    LPAREN      — ``(`` left parenthesis.
    RPAREN      — ``)`` right parenthesis.
    LBRACKET    — ``[`` left square bracket (list literal).
    RBRACKET    — ``]`` right square bracket.
    COMMA       — ``,`` argument / element separator.
    DOT         — ``.`` member access.
    SEMICOLON   — ``;`` expression separator.
    COLON       — ``:`` used in slice / dict syntax.

    Comparison operators
    --------------------
    EQ          — ``==`` equality.
    NEQ         — ``!=`` inequality.
    LT          — ``<`` less-than.
    LTE         — ``<=`` less-than-or-equal.
    GT          — ``>`` greater-than.
    GTE         — ``>=`` greater-than-or-equal.

    Assignment
    ----------
    ASSIGN      — ``=`` single-equals assignment (used in let-binding).

    Logical operators
    -----------------
    AND         — ``and`` keyword.
    OR          — ``or`` keyword.
    NOT         — ``not`` keyword.

    Control flow keywords
    ---------------------
    IF          — ``if`` keyword.
    ELSE        — ``else`` keyword.
    LET         — ``let`` keyword.
    IN          — ``in`` keyword.
    THEN        — ``then`` keyword (optional, for clarity in if-then-else).

    Sentinel
    --------
    EOF         — end of the token stream.
    """

    # Literal tokens
    NUMBER = "NUMBER"
    STRING = "STRING"
    BOOLEAN = "BOOLEAN"
    NULL = "NULL"

    # Name token
    IDENTIFIER = "IDENTIFIER"

    # Arithmetic operators
    PLUS = "PLUS"
    MINUS = "MINUS"
    STAR = "STAR"
    SLASH = "SLASH"
    PERCENT = "PERCENT"
    CARET = "CARET"

    # Grouping and separators
    LPAREN = "LPAREN"
    RPAREN = "RPAREN"
    LBRACKET = "LBRACKET"
    RBRACKET = "RBRACKET"
    COMMA = "COMMA"
    DOT = "DOT"
    SEMICOLON = "SEMICOLON"
    COLON = "COLON"

    # Comparison operators
    EQ = "EQ"
    NEQ = "NEQ"
    LT = "LT"
    LTE = "LTE"
    GT = "GT"
    GTE = "GTE"

    # Assignment
    ASSIGN = "ASSIGN"

    # Logical operators
    AND = "AND"
    OR = "OR"
    NOT = "NOT"

    # Control flow
    IF = "IF"
    ELSE = "ELSE"
    LET = "LET"
    IN = "IN"
    THEN = "THEN"

    # Sentinel
    EOF = "EOF"


# Mapping from keyword string to TokenKind
KEYWORDS: dict[str, TokenKind] = {
    "true": TokenKind.BOOLEAN,
    "false": TokenKind.BOOLEAN,
    "null": TokenKind.NULL,
    "and": TokenKind.AND,
    "or": TokenKind.OR,
    "not": TokenKind.NOT,
    "if": TokenKind.IF,
    "else": TokenKind.ELSE,
    "let": TokenKind.LET,
    "in": TokenKind.IN,
    "then": TokenKind.THEN,
}
"""
Mapping of reserved keyword strings to their corresponding ``TokenKind``.

The lexer checks each identifier against this table after reading the full
word; if found, the token is classified as a keyword rather than an identifier.
"""


# ---------------------------------------------------------------------------
# Token dataclass
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class Token:
    """
    A single lexical unit produced by the Lexer.

    Attributes:
        kind:  The category of this token (from :class:`TokenKind`).
        value: The literal source text of the token, or the canonical
               Python value for literals (e.g. ``42``, ``3.14``, ``True``,
               ``"hello"``).
        line:  1-based source line on which this token appears.
        col:   1-based column of the first character of this token.

    Examples::

        Token(kind=TokenKind.NUMBER, value=42, line=1, col=1)
        Token(kind=TokenKind.IDENTIFIER, value="x", line=1, col=5)
        Token(kind=TokenKind.PLUS, value="+", line=1, col=7)
    """

    kind: TokenKind
    value: object  # str | int | float | bool | None
    line: int
    col: int

    def is_kind(self, *kinds: TokenKind) -> bool:
        """
        Return True if this token's kind matches any of the provided kinds.

        Args:
            *kinds: One or more :class:`TokenKind` values to check against.

        Returns:
            ``True`` if ``self.kind in kinds``, ``False`` otherwise.

        Examples::

            tok = Token(TokenKind.PLUS, "+", 1, 1)
            tok.is_kind(TokenKind.PLUS, TokenKind.MINUS)  # True
            tok.is_kind(TokenKind.STAR)                   # False
        """
        return self.kind in kinds

    def __repr__(self) -> str:
        """Return a compact debug representation of this token."""
        return f"Token({self.kind.name}, {self.value!r}, {self.line}:{self.col})"


# ---------------------------------------------------------------------------
# AST base class
# ---------------------------------------------------------------------------


@dataclass
class ASTNode:
    """
    Abstract base class for all AST nodes.

    Every concrete node subclass carries the source ``line`` and ``col`` so
    that the evaluator can produce error messages with accurate positions.

    Attributes:
        line: 1-based source line of the node's first token.
        col:  1-based column of the node's first token.
    """

    line: int = field(default=0, repr=False)
    col: int = field(default=0, repr=False)

    def node_name(self) -> str:
        """
        Return the short class name of this AST node for display purposes.

        Returns:
            The unqualified class name string (e.g. ``"BinaryOp"``).
        """
        return type(self).__name__


# ---------------------------------------------------------------------------
# Literal AST nodes
# ---------------------------------------------------------------------------


@dataclass
class NumberLit(ASTNode):
    """
    AST node representing a numeric literal (integer or float).

    Attributes:
        value: The numeric value — either ``int`` or ``float`` as parsed.

    Examples::

        NumberLit(value=42, line=1, col=1)
        NumberLit(value=3.14, line=2, col=5)
    """

    value: int | float = field(default=0)

    def __repr__(self) -> str:
        """Return a string representation highlighting the numeric value."""
        return f"NumberLit({self.value!r})"


@dataclass
class StringLit(ASTNode):
    """
    AST node representing a string literal.

    Attributes:
        value: The decoded string content (escape sequences already resolved).

    Examples::

        StringLit(value="hello world", line=1, col=1)
        StringLit(value="tab\\there", line=2, col=3)
    """

    value: str = field(default="")

    def __repr__(self) -> str:
        """Return a string representation highlighting the string value."""
        return f"StringLit({self.value!r})"


@dataclass
class BoolLit(ASTNode):
    """
    AST node representing a boolean literal (``true`` or ``false``).

    Attributes:
        value: ``True`` or ``False``.

    Examples::

        BoolLit(value=True, line=1, col=1)
        BoolLit(value=False, line=1, col=6)
    """

    value: bool = field(default=False)

    def __repr__(self) -> str:
        """Return a string representation highlighting the boolean value."""
        return f"BoolLit({self.value!r})"


@dataclass
class NullLit(ASTNode):
    """
    AST node representing the ``null`` literal.

    The value is always Python ``None``; this class exists so the visitor
    pattern has a dedicated dispatch point.

    Examples::

        NullLit(line=3, col=7)
    """

    def __repr__(self) -> str:
        """Return a fixed null representation."""
        return "NullLit()"


# ---------------------------------------------------------------------------
# Variable reference
# ---------------------------------------------------------------------------


@dataclass
class Identifier(ASTNode):
    """
    AST node representing a variable reference.

    Attributes:
        name: The variable name as a string.

    Examples::

        Identifier(name="x", line=1, col=5)
        Identifier(name="myVar", line=2, col=1)
    """

    name: str = field(default="")

    def __repr__(self) -> str:
        """Return a string representation showing the identifier name."""
        return f"Identifier({self.name!r})"


# ---------------------------------------------------------------------------
# Operator nodes
# ---------------------------------------------------------------------------


@dataclass
class BinaryOp(ASTNode):
    """
    AST node for a binary infix operation.

    Attributes:
        op:    The operator token (carries ``kind`` and ``value``).
        left:  The left operand (any ASTNode).
        right: The right operand (any ASTNode).

    Supported operators (by ``op.kind``):
        PLUS, MINUS, STAR, SLASH, PERCENT, CARET,
        EQ, NEQ, LT, LTE, GT, GTE, AND, OR

    Examples::

        BinaryOp(op=Token(PLUS, "+", 1, 3), left=NumberLit(2), right=NumberLit(3))
    """

    op: Token = field(default_factory=lambda: Token(TokenKind.PLUS, "+", 0, 0))
    left: ASTNode = field(default_factory=lambda: NullLit())
    right: ASTNode = field(default_factory=lambda: NullLit())

    def __repr__(self) -> str:
        """Return a nested string representation of this binary operation."""
        return f"BinaryOp({self.op.value!r}, {self.left!r}, {self.right!r})"


@dataclass
class UnaryOp(ASTNode):
    """
    AST node for a unary prefix operation.

    Attributes:
        op:     The operator token.
        operand: The single operand.

    Supported operators (by ``op.kind``):
        MINUS (negation), NOT (logical not)

    Examples::

        UnaryOp(op=Token(MINUS, "-", 1, 1), operand=NumberLit(5))
        UnaryOp(op=Token(NOT, "not", 1, 1), operand=BoolLit(True))
    """

    op: Token = field(default_factory=lambda: Token(TokenKind.MINUS, "-", 0, 0))
    operand: ASTNode = field(default_factory=lambda: NullLit())

    def __repr__(self) -> str:
        """Return a string representation showing the operator and operand."""
        return f"UnaryOp({self.op.value!r}, {self.operand!r})"


# ---------------------------------------------------------------------------
# Function call node
# ---------------------------------------------------------------------------


@dataclass
class Call(ASTNode):
    """
    AST node for a function call expression.

    Attributes:
        callee: The expression that evaluates to the function (typically an
                :class:`Identifier`).
        args:   Ordered list of argument expressions.

    Examples::

        Call(
            callee=Identifier(name="max"),
            args=[NumberLit(1), NumberLit(2)],
        )
    """

    callee: ASTNode = field(default_factory=lambda: NullLit())
    args: list[ASTNode] = field(default_factory=list)

    def __repr__(self) -> str:
        """Return a string representation showing callee and argument count."""
        return f"Call({self.callee!r}, [{len(self.args)} args])"


# ---------------------------------------------------------------------------
# Special-form nodes
# ---------------------------------------------------------------------------


@dataclass
class IfExpr(ASTNode):
    """
    AST node for an ``if`` conditional expression (ternary style).

    The grammar is::

        if <condition> then <then_branch> else <else_branch>

    or equivalently (without ``then``)::

        if <condition> <then_branch> else <else_branch>

    Attributes:
        condition:   The boolean test expression.
        then_branch: Expression evaluated when condition is truthy.
        else_branch: Expression evaluated when condition is falsy.
                     May be ``None`` if the ``else`` clause is omitted.

    Examples::

        IfExpr(
            condition=BoolLit(True),
            then_branch=NumberLit(1),
            else_branch=NumberLit(0),
        )
    """

    condition: ASTNode = field(default_factory=lambda: NullLit())
    then_branch: ASTNode = field(default_factory=lambda: NullLit())
    else_branch: ASTNode | None = field(default=None)

    def __repr__(self) -> str:
        """Return a compact representation of the if expression."""
        return (
            f"IfExpr(condition={self.condition!r}, "
            f"then={self.then_branch!r}, "
            f"else={self.else_branch!r})"
        )


@dataclass
class LetExpr(ASTNode):
    """
    AST node for a ``let`` binding expression.

    The grammar is::

        let <name> = <binding> in <body>

    Attributes:
        name:    The variable name being bound.
        binding: The expression whose value is bound to ``name``.
        body:    The expression evaluated with ``name`` in scope.

    Examples::

        LetExpr(
            name="x",
            binding=NumberLit(5),
            body=BinaryOp(op=PLUS, left=Identifier("x"), right=NumberLit(1)),
        )
    """

    name: str = field(default="")
    binding: ASTNode = field(default_factory=lambda: NullLit())
    body: ASTNode = field(default_factory=lambda: NullLit())

    def __repr__(self) -> str:
        """Return a compact representation of the let expression."""
        return (
            f"LetExpr(name={self.name!r}, binding={self.binding!r}, body={self.body!r})"
        )


# ---------------------------------------------------------------------------
# Program root node
# ---------------------------------------------------------------------------


@dataclass
class Program(ASTNode):
    """
    Root AST node representing a complete program.

    A program is a sequence of top-level expressions separated by semicolons
    (or simply a single expression). The evaluator returns the value of the
    last expression.

    Attributes:
        body: Ordered list of top-level expression nodes.

    Examples::

        Program(body=[NumberLit(42)])
        Program(body=[LetExpr(...), BinaryOp(...)])
    """

    body: list[ASTNode] = field(default_factory=list)

    def __repr__(self) -> str:
        """Return a string representation showing the number of statements."""
        return f"Program([{len(self.body)} exprs])"


# ---------------------------------------------------------------------------
# Environment (variable scope)
# ---------------------------------------------------------------------------


class Environment:
    """
    A lexically-scoped variable environment (symbol table).

    Each environment has an optional reference to an enclosing (parent)
    environment, implementing the classic chain-of-scope lookup rule: a
    variable lookup starts in the current frame and walks up the chain until
    the name is found or the chain is exhausted.

    Attributes:
        _bindings: Mapping from variable name to its current value.
        _parent:   The enclosing environment, or ``None`` at the global scope.

    Examples::

        global_env = Environment()
        global_env.define("pi", 3.14159)

        local_env = Environment(parent=global_env)
        local_env.define("x", 42)
        local_env.lookup("pi")  # => 3.14159 (found in parent)
        local_env.lookup("x")   # => 42 (found locally)
    """

    def __init__(self, parent: Environment | None = None) -> None:
        """
        Initialise an environment.

        Args:
            parent: The enclosing environment, used as a fallback during
                    variable lookup.  Pass ``None`` for the global scope.
        """
        self._bindings: dict[str, Value] = {}
        self._parent: Environment | None = parent

    def define(self, name: str, value: Value) -> None:
        """
        Bind ``name`` to ``value`` in the current (innermost) scope.

        Defines a new binding in this environment frame.  If a binding for
        ``name`` already exists in this frame it is silently overwritten.

        Args:
            name:  The variable name to bind.
            value: The runtime value to associate with the name.

        Examples::

            env = Environment()
            env.define("x", 10)
            env.lookup("x")  # => 10
        """
        self._bindings[name] = value

    def assign(self, name: str, value: Value) -> None:
        """
        Assign ``value`` to an *existing* binding for ``name``.

        Walks up the environment chain to find the frame where ``name`` is
        currently bound, then updates it.  Raises :class:`EvalError` if
        ``name`` is not bound anywhere in the chain.

        Args:
            name:  The variable name whose binding should be updated.
            value: The new value.

        Raises:
            EvalError: If ``name`` has not been defined in any enclosing scope.
        """
        env: Environment | None = self
        while env is not None:
            if name in env._bindings:
                env._bindings[name] = value
                return
            env = env._parent
        raise EvalError(f"Undefined variable '{name}' (cannot assign)")

    def lookup(self, name: str) -> Value:
        """
        Look up the value bound to ``name`` by walking up the scope chain.

        Args:
            name: The variable name to resolve.

        Returns:
            The runtime value associated with ``name`` in the nearest
            enclosing scope where it is defined.

        Raises:
            EvalError: If ``name`` is not bound in any enclosing scope.

        Examples::

            global_env = Environment()
            global_env.define("answer", 42)
            child_env = Environment(parent=global_env)
            child_env.lookup("answer")  # => 42
        """
        env: Environment | None = self
        while env is not None:
            if name in env._bindings:
                return env._bindings[name]
            env = env._parent
        raise EvalError(f"Undefined variable '{name}'")

    def has(self, name: str) -> bool:
        """
        Return True if ``name`` is bound in this environment or any parent.

        Args:
            name: The variable name to test.

        Returns:
            ``True`` if ``name`` is reachable from this scope, else ``False``.
        """
        env: Environment | None = self
        while env is not None:
            if name in env._bindings:
                return True
            env = env._parent
        return False

    def child(self) -> Environment:
        """
        Create and return a new child environment that inherits from this one.

        Returns:
            A fresh :class:`Environment` whose parent is ``self``.

        Examples::

            parent = Environment()
            parent.define("x", 1)
            child = parent.child()
            child.define("y", 2)
            child.lookup("x")  # => 1
        """
        return Environment(parent=self)

    def bindings_snapshot(self) -> dict[str, Value]:
        """
        Return a shallow copy of this environment's local bindings.

        Does NOT include bindings from parent environments.

        Returns:
            A ``dict`` mapping each locally-defined name to its current value.
        """
        return dict(self._bindings)

    def all_names(self) -> list[str]:
        """
        Return all variable names visible from this environment, including
        those defined in parent scopes.

        Names from inner scopes shadow those in outer scopes; each name
        appears at most once in the result.

        Returns:
            A list of unique variable names in resolution order.
        """
        seen: set[str] = set()
        names: list[str] = []
        env: Environment | None = self
        while env is not None:
            for name in env._bindings:
                if name not in seen:
                    seen.add(name)
                    names.append(name)
            env = env._parent
        return names

    def depth(self) -> int:
        """
        Return the nesting depth of this environment.

        The global (root) environment has depth 0.  Each child increments
        the depth by 1.

        Returns:
            Non-negative integer representing how many parent scopes exist
            above this one.
        """
        d = 0
        env: Environment | None = self._parent
        while env is not None:
            d += 1
            env = env._parent
        return d

    def __repr__(self) -> str:
        """Return a compact representation showing local bindings and depth."""
        return (
            f"Environment(depth={self.depth()}, locals={list(self._bindings.keys())!r})"
        )


# ---------------------------------------------------------------------------
# Operator precedence table
# ---------------------------------------------------------------------------

# Maps TokenKind to a (left_binding_power, right_binding_power) tuple.
# Higher numbers bind more tightly.
INFIX_PRECEDENCE: dict[TokenKind, tuple[int, int]] = {
    TokenKind.OR: (10, 11),
    TokenKind.AND: (20, 21),
    TokenKind.EQ: (30, 31),
    TokenKind.NEQ: (30, 31),
    TokenKind.LT: (40, 41),
    TokenKind.LTE: (40, 41),
    TokenKind.GT: (40, 41),
    TokenKind.GTE: (40, 41),
    TokenKind.PLUS: (50, 51),
    TokenKind.MINUS: (50, 51),
    TokenKind.STAR: (60, 61),
    TokenKind.SLASH: (60, 61),
    TokenKind.PERCENT: (60, 61),
    TokenKind.CARET: (70, 70),  # right-associative: equal bp on both sides
}
"""
Infix precedence table for Pratt / operator-precedence parsing.

Each entry is ``(left_binding_power, right_binding_power)``.
The right binding power is set one higher than the left for left-associative
operators; for right-associative operators (``^``) both are equal.
"""

PREFIX_PRECEDENCE: dict[TokenKind, int] = {
    TokenKind.MINUS: 80,
    TokenKind.NOT: 80,
}
"""
Prefix (unary) precedence values.

These are used by the parser to determine how tightly a prefix operator
binds to its operand.
"""


# ---------------------------------------------------------------------------
# Helper utilities
# ---------------------------------------------------------------------------


def is_truthy(value: Value) -> bool:
    """
    Determine the truthiness of a runtime value following the language rules.

    Truthiness rules:
      - ``None``  → False
      - ``False`` → False
      - ``0``     → False
      - ``0.0``   → False
      - ``""``    → False
      - ``[]``    → False
      - Everything else → True

    Args:
        value: Any runtime value produced by the evaluator.

    Returns:
        ``True`` if the value is considered truthy, ``False`` otherwise.

    Examples::

        is_truthy(1)     # True
        is_truthy(0)     # False
        is_truthy("")    # False
        is_truthy("hi")  # True
        is_truthy(None)  # False
    """
    if value is None:
        return False
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        return value != 0
    if isinstance(value, str):
        return len(value) > 0
    if isinstance(value, list):
        return len(value) > 0
    return True


def value_to_display(value: Value) -> str:
    """
    Convert a runtime value to its canonical display string.

    This is what the built-in ``str()`` and ``print()`` functions produce.

    Args:
        value: Any runtime value.

    Returns:
        Human-readable string representation.

    Examples::

        value_to_display(42)      # "42"
        value_to_display(3.14)    # "3.14"
        value_to_display(True)    # "true"
        value_to_display(False)   # "false"
        value_to_display(None)    # "null"
        value_to_display("hi")    # "hi"
        value_to_display([1,2])   # "[1, 2]"
    """
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, list):
        items = ", ".join(value_to_display(v) for v in value)
        return f"[{items}]"
    return str(value)


def type_name(value: Value) -> str:
    """
    Return the language-level type name for a runtime value.

    Args:
        value: Any runtime value.

    Returns:
        One of ``"number"``, ``"string"``, ``"boolean"``, ``"null"``,
        ``"list"``.

    Examples::

        type_name(42)     # "number"
        type_name(3.14)   # "number"
        type_name("hi")   # "string"
        type_name(True)   # "boolean"
        type_name(None)   # "null"
        type_name([1,2])  # "list"
    """
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "boolean"
    if isinstance(value, (int, float)):
        return "number"
    if isinstance(value, str):
        return "string"
    if isinstance(value, list):
        return "list"
    return "unknown"


def coerce_to_number(value: Value, context: str = "") -> int | float:
    """
    Attempt to coerce a runtime value to a numeric type.

    Coercion rules:
      - ``int`` / ``float`` → returned as-is.
      - ``bool`` → ``1`` for ``True``, ``0`` for ``False``.
      - ``str``  → parsed as int or float.
      - ``None`` or ``list`` → raises :class:`EvalError`.

    Args:
        value:   The value to coerce.
        context: Optional description of the context for error messages.

    Returns:
        The numeric value.

    Raises:
        EvalError: If the value cannot be coerced to a number.

    Examples::

        coerce_to_number(3)       # 3
        coerce_to_number("42")    # 42
        coerce_to_number(True)    # 1
    """
    if isinstance(value, bool):
        return int(value)
    if isinstance(value, (int, float)):
        return value
    if isinstance(value, str):
        try:
            return int(value)
        except ValueError:
            pass
        try:
            return float(value)
        except ValueError:
            pass
        msg = f"Cannot convert string {value!r} to number"
        if context:
            msg = f"{context}: {msg}"
        raise EvalError(msg)
    msg = f"Cannot convert {type_name(value)!r} to number"
    if context:
        msg = f"{context}: {msg}"
    raise EvalError(msg)


def coerce_to_string(value: Value) -> str:
    """
    Coerce any runtime value to its string representation.

    This is a lossless conversion used by string concatenation and built-in
    ``str()``.

    Args:
        value: Any runtime value.

    Returns:
        The display string for the value (see :func:`value_to_display`).
    """
    return value_to_display(value)


def values_equal(a: Value, b: Value) -> bool:
    """
    Test deep equality between two runtime values.

    Numeric comparison is type-coercing: ``1 == 1.0`` is ``True``.
    String and boolean comparisons are strict.  ``None == None`` is ``True``.

    Args:
        a: First value.
        b: Second value.

    Returns:
        ``True`` if the values are considered equal by language semantics.

    Examples::

        values_equal(1, 1.0)     # True
        values_equal("a", "a")   # True
        values_equal(True, 1)    # False (bool != number in language)
        values_equal(None, None) # True
    """
    if a is None and b is None:
        return True
    if a is None or b is None:
        return False
    if isinstance(a, bool) or isinstance(b, bool):
        return a is b or (isinstance(a, bool) and isinstance(b, bool) and a == b)
    if isinstance(a, (int, float)) and isinstance(b, (int, float)):
        return a == b
    if isinstance(a, list) and isinstance(b, list):
        if len(a) != len(b):
            return False
        return all(values_equal(x, y) for x, y in zip(a, b))
    return a == b
