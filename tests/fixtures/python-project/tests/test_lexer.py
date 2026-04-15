"""
test_lexer.py — pytest tests for the python_project Lexer.

All tests exercise real lexer behaviour with genuine assertions.
No mocks, no placeholders.
"""

from __future__ import annotations

import pytest

from python_project.lexer import Lexer
from python_project.types import LexError, Token, TokenKind


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def tokenize(source: str) -> list[Token]:
    """Lex ``source`` and return all tokens (including EOF)."""
    return Lexer(source).tokenize()


def kinds(source: str) -> list[TokenKind]:
    """Return just the TokenKind sequence for a source string."""
    return [t.kind for t in tokenize(source)]


# ---------------------------------------------------------------------------
# Numeric literal tests
# ---------------------------------------------------------------------------


def test_tokenize_numbers() -> None:
    """Lexer produces correct NUMBER tokens for integer and float literals."""
    toks = tokenize("42 3.14 0 100_000")
    # Expect: NUMBER(42), NUMBER(3.14), NUMBER(0), NUMBER(100000), EOF
    assert toks[0].kind == TokenKind.NUMBER
    assert toks[0].value == 42
    assert isinstance(toks[0].value, int)

    assert toks[1].kind == TokenKind.NUMBER
    assert abs(toks[1].value - 3.14) < 1e-10  # type: ignore[operator]
    assert isinstance(toks[1].value, float)

    assert toks[2].kind == TokenKind.NUMBER
    assert toks[2].value == 0

    assert toks[3].kind == TokenKind.NUMBER
    assert toks[3].value == 100_000

    assert toks[-1].kind == TokenKind.EOF


def test_tokenize_float_scientific() -> None:
    """Lexer handles scientific notation in numeric literals."""
    toks = tokenize("1e10 2.5e-3 6.022E23")
    assert toks[0].kind == TokenKind.NUMBER
    assert toks[0].value == 1e10

    assert toks[1].kind == TokenKind.NUMBER
    assert abs(toks[1].value - 2.5e-3) < 1e-15  # type: ignore[operator]

    assert toks[2].kind == TokenKind.NUMBER
    assert abs(toks[2].value - 6.022e23) < 1e15  # type: ignore[operator]


def test_tokenize_hex_octal_binary() -> None:
    """Lexer handles 0x, 0o, and 0b prefixed integer literals."""
    toks = tokenize("0xFF 0o17 0b1010")
    assert toks[0].kind == TokenKind.NUMBER
    assert toks[0].value == 255  # 0xFF

    assert toks[1].kind == TokenKind.NUMBER
    assert toks[1].value == 15  # 0o17

    assert toks[2].kind == TokenKind.NUMBER
    assert toks[2].value == 10  # 0b1010


# ---------------------------------------------------------------------------
# Operator tests
# ---------------------------------------------------------------------------


def test_tokenize_operators() -> None:
    """Lexer handles all operator tokens correctly."""
    source = "+ - * / % ^ == != < <= > >= = and or not"
    token_kinds = kinds(source)

    expected = [
        TokenKind.PLUS,
        TokenKind.MINUS,
        TokenKind.STAR,
        TokenKind.SLASH,
        TokenKind.PERCENT,
        TokenKind.CARET,
        TokenKind.EQ,
        TokenKind.NEQ,
        TokenKind.LT,
        TokenKind.LTE,
        TokenKind.GT,
        TokenKind.GTE,
        TokenKind.ASSIGN,
        TokenKind.AND,
        TokenKind.OR,
        TokenKind.NOT,
        TokenKind.EOF,
    ]
    assert token_kinds == expected


def test_tokenize_punctuation() -> None:
    """Lexer produces correct tokens for punctuation characters."""
    token_kinds = kinds("( ) [ ] , . ; :")
    expected = [
        TokenKind.LPAREN,
        TokenKind.RPAREN,
        TokenKind.LBRACKET,
        TokenKind.RBRACKET,
        TokenKind.COMMA,
        TokenKind.DOT,
        TokenKind.SEMICOLON,
        TokenKind.COLON,
        TokenKind.EOF,
    ]
    assert token_kinds == expected


# ---------------------------------------------------------------------------
# String literal tests
# ---------------------------------------------------------------------------


def test_tokenize_string_literals() -> None:
    """Lexer handles string literals including escape sequences."""
    toks = tokenize(r'"hello\nworld"')
    assert toks[0].kind == TokenKind.STRING
    assert toks[0].value == "hello\nworld"

    toks2 = tokenize(r'"tab\there"')
    assert toks2[0].value == "tab\there"

    toks3 = tokenize(r'"back\\slash"')
    assert toks3[0].value == "back\\slash"


def test_tokenize_string_unicode_escape() -> None:
    """Lexer handles \\uXXXX unicode escape sequences in strings."""
    toks = tokenize('"\\u0041"')  # U+0041 = 'A'
    assert toks[0].kind == TokenKind.STRING
    assert toks[0].value == "A"

    toks2 = tokenize('"\\u03B1"')  # U+03B1 = 'α'
    assert toks2[0].kind == TokenKind.STRING
    assert toks2[0].value == "α"


def test_tokenize_single_quoted_string() -> None:
    """Lexer handles single-quoted string literals."""
    toks = tokenize("'hello world'")
    assert toks[0].kind == TokenKind.STRING
    assert toks[0].value == "hello world"


def test_tokenize_unterminated_string_raises() -> None:
    """Lexer raises LexError for an unterminated string literal."""
    with pytest.raises(LexError, match="Unterminated string"):
        tokenize('"oops')


# ---------------------------------------------------------------------------
# Keyword tests
# ---------------------------------------------------------------------------


def test_tokenize_keywords() -> None:
    """Lexer recognises boolean and null keywords correctly."""
    toks = tokenize("true false null")

    assert toks[0].kind == TokenKind.BOOLEAN
    assert toks[0].value is True

    assert toks[1].kind == TokenKind.BOOLEAN
    assert toks[1].value is False

    assert toks[2].kind == TokenKind.NULL
    assert toks[2].value is None


def test_tokenize_control_keywords() -> None:
    """Lexer recognises if, else, let, in, then keywords."""
    token_kinds = kinds("if else let in then")
    assert token_kinds == [
        TokenKind.IF,
        TokenKind.ELSE,
        TokenKind.LET,
        TokenKind.IN,
        TokenKind.THEN,
        TokenKind.EOF,
    ]


# ---------------------------------------------------------------------------
# Identifier tests
# ---------------------------------------------------------------------------


def test_tokenize_identifiers() -> None:
    """Lexer produces IDENTIFIER tokens for user-defined names."""
    toks = tokenize("x myVar _private camelCase snake_case")
    ident_toks = [t for t in toks if t.kind == TokenKind.IDENTIFIER]
    assert len(ident_toks) == 5
    names = [t.value for t in ident_toks]
    assert names == ["x", "myVar", "_private", "camelCase", "snake_case"]


def test_tokenize_identifier_vs_keyword() -> None:
    """Keywords are not classified as identifiers."""
    toks = tokenize("truthy falsey")  # not keywords
    assert toks[0].kind == TokenKind.IDENTIFIER
    assert toks[1].kind == TokenKind.IDENTIFIER

    toks2 = tokenize("true false")  # are keywords
    assert toks2[0].kind == TokenKind.BOOLEAN
    assert toks2[1].kind == TokenKind.BOOLEAN


# ---------------------------------------------------------------------------
# Whitespace and comment tests
# ---------------------------------------------------------------------------


def test_tokenize_empty_input() -> None:
    """Lexer handles empty input, producing only an EOF token."""
    toks = tokenize("")
    assert len(toks) == 1
    assert toks[0].kind == TokenKind.EOF


def test_tokenize_whitespace_only() -> None:
    """Lexer handles whitespace-only input."""
    toks = tokenize("   \t\n  ")
    assert len(toks) == 1
    assert toks[0].kind == TokenKind.EOF


def test_tokenize_line_comment() -> None:
    """Lexer skips single-line comments starting with #."""
    toks = tokenize("1 # this is a comment\n2")
    number_toks = [t for t in toks if t.kind == TokenKind.NUMBER]
    assert len(number_toks) == 2
    assert number_toks[0].value == 1
    assert number_toks[1].value == 2


def test_tokenize_block_comment() -> None:
    """Lexer skips block comments delimited by /* and */."""
    toks = tokenize("1 /* this is\na block comment */ 2")
    number_toks = [t for t in toks if t.kind == TokenKind.NUMBER]
    assert len(number_toks) == 2
    assert number_toks[0].value == 1
    assert number_toks[1].value == 2


def test_tokenize_unterminated_block_comment_raises() -> None:
    """Lexer raises LexError for an unterminated block comment."""
    with pytest.raises(LexError, match="Unterminated block comment"):
        tokenize("1 /* oops")


# ---------------------------------------------------------------------------
# Line / column tracking tests
# ---------------------------------------------------------------------------


def test_tokenize_line_col_tracking() -> None:
    """Lexer correctly reports line and column numbers for tokens."""
    toks = tokenize("1\n2\n3")
    num_toks = [t for t in toks if t.kind == TokenKind.NUMBER]
    assert num_toks[0].line == 1
    assert num_toks[0].col == 1
    assert num_toks[1].line == 2
    assert num_toks[1].col == 1
    assert num_toks[2].line == 3
    assert num_toks[2].col == 1


def test_tokenize_column_advances() -> None:
    """Lexer reports increasing column numbers within a single line."""
    toks = tokenize("a + b")
    assert toks[0].col == 1  # 'a'
    assert toks[1].col == 3  # '+'
    assert toks[2].col == 5  # 'b'


# ---------------------------------------------------------------------------
# Complex expression tokenization tests
# ---------------------------------------------------------------------------


def test_tokenize_complex_expression() -> None:
    """Lexer tokenises a complex nested expression correctly."""
    source = "let x = max(1, 2 + 3) in x * 4"
    token_kinds = kinds(source)
    expected = [
        TokenKind.LET,
        TokenKind.IDENTIFIER,  # x
        TokenKind.ASSIGN,
        TokenKind.IDENTIFIER,  # max
        TokenKind.LPAREN,
        TokenKind.NUMBER,  # 1
        TokenKind.COMMA,
        TokenKind.NUMBER,  # 2
        TokenKind.PLUS,
        TokenKind.NUMBER,  # 3
        TokenKind.RPAREN,
        TokenKind.IN,
        TokenKind.IDENTIFIER,  # x
        TokenKind.STAR,
        TokenKind.NUMBER,  # 4
        TokenKind.EOF,
    ]
    assert token_kinds == expected


def test_tokenize_if_expression() -> None:
    """Lexer tokenises an if-then-else expression."""
    source = "if x > 0 then x else 0 - x"
    token_kinds = kinds(source)
    assert TokenKind.IF in token_kinds
    assert TokenKind.THEN in token_kinds
    assert TokenKind.ELSE in token_kinds
    assert TokenKind.GT in token_kinds


def test_tokenize_string_concatenation_expr() -> None:
    """Lexer tokenises a string concatenation expression."""
    source = '"hello" + " " + "world"'
    str_toks = [t for t in tokenize(source) if t.kind == TokenKind.STRING]
    assert len(str_toks) == 3
    assert str_toks[0].value == "hello"
    assert str_toks[1].value == " "
    assert str_toks[2].value == "world"


# ---------------------------------------------------------------------------
# Utility method tests
# ---------------------------------------------------------------------------


def test_lexer_dump() -> None:
    """Lexer.dump() returns a non-empty string after tokenization."""
    lexer = Lexer("1 + 2")
    lexer.tokenize()
    dump_str = lexer.dump()
    assert "NUMBER" in dump_str
    assert "PLUS" in dump_str


def test_lexer_all_identifiers() -> None:
    """Lexer.all_identifiers() returns just the identifier names."""
    lexer = Lexer("let x = y + z in x")
    lexer.tokenize()
    idents = lexer.all_identifiers()
    assert set(idents) == {"x", "y", "z", "x"}  # x appears twice


def test_lexer_all_number_values() -> None:
    """Lexer.all_number_values() returns all numeric literal values."""
    lexer = Lexer("1 + 2.5 * 3")
    lexer.tokenize()
    nums = lexer.all_number_values()
    assert nums == [1, 2.5, 3]


def test_lexer_validate_balanced_parens() -> None:
    """Lexer.validate_balanced_parens() detects unbalanced parentheses."""
    lexer = Lexer("max(1, 2)")
    toks = lexer.tokenize()
    assert lexer.validate_balanced_parens(toks) is True

    lexer2 = Lexer("max(1, 2")
    toks2 = lexer2.tokenize()
    assert lexer2.validate_balanced_parens(toks2) is False


def test_lexer_quick_tokenize() -> None:
    """Lexer.quick_tokenize() is a convenience static method."""
    toks = Lexer.quick_tokenize("42")
    assert toks[0].kind == TokenKind.NUMBER
    assert toks[0].value == 42
    assert toks[-1].kind == TokenKind.EOF


def test_lexer_count_tokens() -> None:
    """Lexer.count_tokens() returns the correct token count."""
    lexer = Lexer("1 + 2")
    lexer.tokenize()
    # 3 value tokens + 1 EOF = 4
    assert lexer.count_tokens() == 4


def test_lexer_bad_character_raises() -> None:
    """Lexer raises LexError on an unexpected character."""
    with pytest.raises(LexError):
        tokenize("1 @ 2")


def test_lexer_bang_without_eq_raises() -> None:
    """Lexer raises LexError for a bare '!' not followed by '='."""
    with pytest.raises(LexError):
        tokenize("1 ! 2")
