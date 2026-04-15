"""
lexer.py — Full lexer for the python_project expression language.

The :class:`Lexer` accepts a source string and produces a flat list of
:class:`~python_project.types.Token` objects terminated by a single
``EOF`` token.

Supported token kinds
---------------------
- Integer and floating-point numeric literals (``42``, ``3.14``, ``1e10``,
  ``0xDEAD``, ``0o17``, ``0b1010``).
- String literals with escape sequences (``\\n``, ``\\t``, ``\\\\``,
  ``\\'``, ``\\"``, ``\\uXXXX``).
- Boolean literals: ``true`` / ``false``.
- Null literal: ``null``.
- Identifiers and reserved keywords.
- All arithmetic, comparison, and logical operators.
- Grouping tokens: ``(``, ``)``, ``[``, ``]``, ``,``, ``.``, ``;``, ``:``.
- Single-line comments starting with ``#``.
- Multi-line comments delimited by ``/*`` and ``*/``.
"""

from __future__ import annotations

from python_project.types import (
    KEYWORDS,
    LexError,
    Token,
    TokenKind,
)


# ---------------------------------------------------------------------------
# Escape sequence table
# ---------------------------------------------------------------------------

_SIMPLE_ESCAPES: dict[str, str] = {
    "n": "\n",
    "t": "\t",
    "r": "\r",
    "\\": "\\",
    "'": "'",
    '"': '"',
    "0": "\0",
    "a": "\a",
    "b": "\b",
    "f": "\f",
    "v": "\v",
}
"""
Mapping of single-character escape letters to their replacement characters.

Used by :meth:`Lexer.lex_string` when processing backslash sequences.
"""


# ---------------------------------------------------------------------------
# Main Lexer class
# ---------------------------------------------------------------------------


class Lexer:
    """
    Tokeniser for the python_project expression language.

    The lexer operates on a single pass over the source string.  It is not
    lazy — :meth:`tokenize` walks the entire input and returns all tokens
    up front.

    Attributes:
        _source:  The raw source string being tokenised.
        _tokens:  Accumulated list of tokens (built during tokenization).
        _start:   Index of the first character of the current token.
        _current: Index of the character currently being examined.
        _line:    Current 1-based line number.
        _col:     Current 1-based column number.
        _line_start: Index in ``_source`` where the current line began.

    Examples::

        lexer = Lexer("1 + 2 * 3")
        tokens = lexer.tokenize()
        # [Token(NUMBER, 1, ...), Token(PLUS, "+", ...), ...]
    """

    def __init__(self, source: str) -> None:
        """
        Initialise the Lexer with a source string.

        Args:
            source: The complete source text to tokenise.
        """
        self._source: str = source
        self._tokens: list[Token] = []
        self._start: int = 0
        self._current: int = 0
        self._line: int = 1
        self._col: int = 1
        self._line_start: int = 0

    # ------------------------------------------------------------------
    # Public interface
    # ------------------------------------------------------------------

    def tokenize(self) -> list[Token]:
        """
        Scan the entire source and return a list of tokens.

        The list always ends with a single :attr:`~python_project.types.TokenKind.EOF`
        token.  The method can be called multiple times (it resets internal
        state each time).

        Returns:
            A list of :class:`~python_project.types.Token` objects.

        Raises:
            LexError: On any lexical error (unexpected character, unterminated
                      string literal, bad escape sequence, etc.).

        Examples::

            tokens = Lexer("1 + 2").tokenize()
            assert tokens[-1].kind == TokenKind.EOF
        """
        self._tokens = []
        self._start = 0
        self._current = 0
        self._line = 1
        self._col = 1
        self._line_start = 0

        while not self._is_at_end():
            self._start = self._current
            self._scan_token()

        self._tokens.append(Token(TokenKind.EOF, "", self._line, self._col))
        return self._tokens

    # ------------------------------------------------------------------
    # Low-level character navigation
    # ------------------------------------------------------------------

    def _is_at_end(self) -> bool:
        """
        Return True if the current position has reached or passed the end
        of the source string.

        Returns:
            ``True`` when ``self._current >= len(self._source)``.
        """
        return self._current >= len(self._source)

    def peek(self) -> str:
        """
        Return the character at the current position without consuming it.

        Returns the null character ``"\\0"`` if the end of input has been
        reached.

        Returns:
            A single-character string or ``"\\0"``.

        Examples::

            lexer = Lexer("abc")
            lexer.peek()   # "a"
        """
        if self._is_at_end():
            return "\0"
        return self._source[self._current]

    def peek_next(self) -> str:
        """
        Return the character *one ahead* of the current position without
        consuming either character.

        Returns ``"\\0"`` if the lookahead position is past the end.

        Returns:
            A single-character string or ``"\\0"``.
        """
        pos = self._current + 1
        if pos >= len(self._source):
            return "\0"
        return self._source[pos]

    def peek_at(self, offset: int) -> str:
        """
        Return the character at ``_current + offset`` without consuming it.

        Args:
            offset: Non-negative distance ahead of the current position.

        Returns:
            A single-character string or ``"\\0"`` if out of bounds.
        """
        pos = self._current + offset
        if pos >= len(self._source):
            return "\0"
        return self._source[pos]

    def advance(self) -> str:
        """
        Consume and return the current character, advancing the position.

        Also updates the line/column tracking.  When a newline is consumed,
        :attr:`_line` is incremented and :attr:`_col` is reset to 1.

        Returns:
            The character that was consumed.
        """
        ch = self._source[self._current]
        self._current += 1
        if ch == "\n":
            self._line += 1
            self._col = 1
            self._line_start = self._current
        else:
            self._col += 1
        return ch

    def match(self, expected: str) -> bool:
        """
        Consume the current character and return True if it equals ``expected``.

        If the current character does not match or we are at the end, the
        position is NOT advanced and False is returned.

        Args:
            expected: The single character to test against.

        Returns:
            ``True`` if the character was consumed, ``False`` otherwise.

        Examples::

            lexer = Lexer(">=")
            lexer.advance()   # consume ">"
            lexer.match("=")  # True — consumes "="
        """
        if self._is_at_end():
            return False
        if self._source[self._current] != expected:
            return False
        self.advance()
        return True

    # ------------------------------------------------------------------
    # Whitespace and comment skipping
    # ------------------------------------------------------------------

    def skip_whitespace(self) -> None:
        """
        Advance past any ASCII whitespace characters (space, tab, carriage
        return, newline) at the current position.

        This method is called before each token scan to skip inter-token
        whitespace.  Newlines are counted for line tracking.
        """
        while not self._is_at_end():
            ch = self.peek()
            if ch in (" ", "\t", "\r", "\n"):
                self.advance()
            elif ch == "#":
                self.skip_line_comment()
            elif ch == "/" and self.peek_next() == "*":
                self.skip_block_comment()
            else:
                break

    def skip_line_comment(self) -> None:
        """
        Advance past a single-line comment that starts with ``#``.

        Consumes characters until a newline or end of input is encountered.
        The newline itself is *not* consumed (it will be handled by
        :meth:`skip_whitespace`).
        """
        # consume the '#'
        self.advance()
        while not self._is_at_end() and self.peek() != "\n":
            self.advance()

    def skip_block_comment(self) -> None:
        """
        Advance past a block comment delimited by ``/*`` and ``*/``.

        Block comments may span multiple lines.  Raises :class:`LexError`
        if the input ends before the closing ``*/`` is found.

        Raises:
            LexError: If the block comment is unterminated.
        """
        start_line = self._line
        start_col = self._col
        # consume '/*'
        self.advance()  # '/'
        self.advance()  # '*'
        while not self._is_at_end():
            if self.peek() == "*" and self.peek_next() == "/":
                self.advance()  # '*'
                self.advance()  # '/'
                return
            self.advance()
        raise LexError("Unterminated block comment", line=start_line, col=start_col)

    # ------------------------------------------------------------------
    # Token scanning dispatch
    # ------------------------------------------------------------------

    def _scan_token(self) -> None:
        """
        Scan one token starting at the current position and append it to
        :attr:`_tokens`.

        This method is the main dispatch hub.  It reads the first character
        and delegates to the appropriate ``lex_*`` helper or directly emits
        a simple single/two-character token.

        Raises:
            LexError: On an unexpected character.
        """
        self.skip_whitespace()
        if self._is_at_end():
            return

        self._start = self._current
        tok_line = self._line
        tok_col = self._col

        ch = self.advance()

        # ---- Numeric literals ----------------------------------------
        if ch.isdigit() or (
            ch == "0" and self.peek() in ("x", "o", "b", "X", "O", "B")
        ):
            self._lex_number_dispatch(ch, tok_line, tok_col)
            return

        # ---- String literals -----------------------------------------
        if ch in ('"', "'"):
            tok = self.lex_string(ch, tok_line, tok_col)
            self._tokens.append(tok)
            return

        # ---- Identifiers / keywords ----------------------------------
        if ch.isalpha() or ch == "_":
            tok = self.lex_identifier(ch, tok_line, tok_col)
            self._tokens.append(tok)
            return

        # ---- Operators and punctuation -------------------------------
        tok = self._lex_operator(ch, tok_line, tok_col)
        if tok is not None:
            self._tokens.append(tok)

    def _lex_number_dispatch(self, first: str, line: int, col: int) -> None:
        """
        Dispatch to the appropriate numeric lexing method based on the prefix.

        Handles:
          - ``0x`` / ``0X`` prefixed hexadecimal integers.
          - ``0o`` / ``0O`` prefixed octal integers.
          - ``0b`` / ``0B`` prefixed binary integers.
          - Decimal integers and floats (including scientific notation).

        Args:
            first: The first digit character already consumed.
            line:  Source line for error reporting.
            col:   Source column for error reporting.
        """
        if first == "0" and self.peek() in ("x", "X"):
            tok = self._lex_hex_number(line, col)
        elif first == "0" and self.peek() in ("o", "O"):
            tok = self._lex_octal_number(line, col)
        elif first == "0" and self.peek() in ("b", "B"):
            tok = self._lex_binary_number(line, col)
        else:
            tok = self.lex_number(first, line, col)
        self._tokens.append(tok)

    # ------------------------------------------------------------------
    # Numeric literal lexing
    # ------------------------------------------------------------------

    def lex_number(self, first: str, line: int, col: int) -> Token:
        """
        Lex a decimal integer or floating-point number.

        Supports:
          - Plain integers: ``42``, ``0``, ``1000000``.
          - Floats with decimal point: ``3.14``, ``0.5``, ``.75`` (not
            supported — must start with a digit).
          - Scientific notation: ``1e10``, ``2.5e-3``, ``6.022E23``.
          - Underscore separators: ``1_000_000``, ``3.141_592``.

        Args:
            first: The first digit character already consumed.
            line:  Source line of the token start.
            col:   Source column of the token start.

        Returns:
            A :class:`Token` of kind ``NUMBER`` whose value is ``int`` or
            ``float``.

        Raises:
            LexError: On a malformed number (e.g. trailing ``e`` without
                      an exponent).
        """
        buf = [first]

        # Integer part
        while self.peek().isdigit() or self.peek() == "_":
            buf.append(self.advance())

        is_float = False

        # Optional fractional part
        if self.peek() == "." and (
            self.peek_next().isdigit() or self.peek_next() == "_"
        ):
            is_float = True
            buf.append(self.advance())  # '.'
            while self.peek().isdigit() or self.peek() == "_":
                buf.append(self.advance())

        # Optional exponent
        if self.peek() in ("e", "E"):
            is_float = True
            buf.append(self.advance())  # 'e' or 'E'
            if self.peek() in ("+", "-"):
                buf.append(self.advance())
            if not self.peek().isdigit():
                raise LexError(
                    "Expected digits after exponent in numeric literal",
                    line=line,
                    col=col,
                )
            while self.peek().isdigit() or self.peek() == "_":
                buf.append(self.advance())

        raw = "".join(buf).replace("_", "")
        if is_float:
            return Token(TokenKind.NUMBER, float(raw), line, col)
        return Token(TokenKind.NUMBER, int(raw), line, col)

    def _lex_hex_number(self, line: int, col: int) -> Token:
        """
        Lex a hexadecimal integer literal (``0x`` prefix).

        Consumes the ``x`` prefix and subsequent hex digits including
        underscore separators.

        Args:
            line: Source line of the ``0`` character.
            col:  Source column of the ``0`` character.

        Returns:
            A ``NUMBER`` token whose value is a Python ``int``.

        Raises:
            LexError: If no hex digits follow the prefix.
        """
        self.advance()  # consume 'x' or 'X'
        buf: list[str] = []
        while self.peek() in "0123456789abcdefABCDEF_":
            buf.append(self.advance())
        if not buf or all(c == "_" for c in buf):
            raise LexError("Expected hexadecimal digits after '0x'", line=line, col=col)
        raw = "".join(buf).replace("_", "")
        return Token(TokenKind.NUMBER, int(raw, 16), line, col)

    def _lex_octal_number(self, line: int, col: int) -> Token:
        """
        Lex an octal integer literal (``0o`` prefix).

        Args:
            line: Source line of the ``0`` character.
            col:  Source column of the ``0`` character.

        Returns:
            A ``NUMBER`` token whose value is a Python ``int``.

        Raises:
            LexError: If no octal digits follow the prefix.
        """
        self.advance()  # consume 'o' or 'O'
        buf: list[str] = []
        while self.peek() in "01234567_":
            buf.append(self.advance())
        if not buf or all(c == "_" for c in buf):
            raise LexError("Expected octal digits after '0o'", line=line, col=col)
        raw = "".join(buf).replace("_", "")
        return Token(TokenKind.NUMBER, int(raw, 8), line, col)

    def _lex_binary_number(self, line: int, col: int) -> Token:
        """
        Lex a binary integer literal (``0b`` prefix).

        Args:
            line: Source line of the ``0`` character.
            col:  Source column of the ``0`` character.

        Returns:
            A ``NUMBER`` token whose value is a Python ``int``.

        Raises:
            LexError: If no binary digits follow the prefix.
        """
        self.advance()  # consume 'b' or 'B'
        buf: list[str] = []
        while self.peek() in "01_":
            buf.append(self.advance())
        if not buf or all(c == "_" for c in buf):
            raise LexError("Expected binary digits after '0b'", line=line, col=col)
        raw = "".join(buf).replace("_", "")
        return Token(TokenKind.NUMBER, int(raw, 2), line, col)

    # ------------------------------------------------------------------
    # String literal lexing
    # ------------------------------------------------------------------

    def lex_string(self, quote: str, line: int, col: int) -> Token:
        """
        Lex a string literal delimited by ``quote`` (either ``"`` or ``'``).

        Supports:
          - Simple text content.
          - Standard single-character escape sequences (``\\n``, ``\\t``,
            ``\\\\``, etc.) via :data:`_SIMPLE_ESCAPES`.
          - Four-digit Unicode escapes: ``\\uXXXX``.
          - Eight-digit Unicode escapes: ``\\UXXXXXXXX``.
          - Hex character escapes: ``\\xHH``.

        Multi-line strings (where the string body contains a literal newline)
        are allowed.

        Args:
            quote: The opening quote character (``"`` or ``'``).
            line:  Source line of the opening quote.
            col:   Source column of the opening quote.

        Returns:
            A :class:`Token` of kind ``STRING`` whose value is the decoded
            Python string (without surrounding quotes).

        Raises:
            LexError: If the string is unterminated or contains an invalid
                      escape sequence.

        Examples::

            lexer = Lexer('"hello\\\\nworld"')
            tok = lexer.lex_string('"', 1, 1)
            tok.value  # "hello\\nworld"
        """
        buf: list[str] = []

        while True:
            if self._is_at_end():
                raise LexError("Unterminated string literal", line=line, col=col)
            ch = self.advance()
            if ch == quote:
                break
            if ch == "\\":
                buf.append(self._lex_escape_sequence(line, col))
            else:
                buf.append(ch)

        return Token(TokenKind.STRING, "".join(buf), line, col)

    def _lex_escape_sequence(self, str_line: int, str_col: int) -> str:
        """
        Consume and decode one escape sequence (the backslash has already
        been consumed by the caller).

        Args:
            str_line: Source line of the opening string quote (for errors).
            str_col:  Source column of the opening string quote (for errors).

        Returns:
            The decoded character(s) as a Python string.

        Raises:
            LexError: On an unrecognised escape letter or malformed
                      ``\\u`` / ``\\x`` sequence.
        """
        if self._is_at_end():
            raise LexError(
                "Unterminated escape sequence at end of input",
                line=str_line,
                col=str_col,
            )
        esc = self.advance()
        if esc in _SIMPLE_ESCAPES:
            return _SIMPLE_ESCAPES[esc]
        if esc == "u":
            return self._lex_unicode_escape(4, str_line, str_col)
        if esc == "U":
            return self._lex_unicode_escape(8, str_line, str_col)
        if esc == "x":
            return self._lex_hex_escape(str_line, str_col)
        raise LexError(
            f"Unknown escape sequence '\\{esc}'",
            line=str_line,
            col=str_col,
        )

    def _lex_unicode_escape(self, length: int, line: int, col: int) -> str:
        """
        Read exactly ``length`` hex digits and return the corresponding
        Unicode character.

        Args:
            length: Number of hex digits to consume (4 for ``\\u``, 8 for
                    ``\\U``).
            line:   Source line for error reporting.
            col:    Source column for error reporting.

        Returns:
            The Unicode character as a Python string.

        Raises:
            LexError: If fewer than ``length`` hex digits are available or if
                      the codepoint is out of range.
        """
        digits: list[str] = []
        for _ in range(length):
            if self._is_at_end():
                raise LexError(
                    f"Incomplete \\u escape — expected {length} hex digits",
                    line=line,
                    col=col,
                )
            ch = self.peek()
            if ch not in "0123456789abcdefABCDEF":
                raise LexError(
                    f"Non-hex character '{ch}' in \\u escape",
                    line=line,
                    col=col,
                )
            digits.append(self.advance())
        codepoint = int("".join(digits), 16)
        try:
            return chr(codepoint)
        except (ValueError, OverflowError) as exc:
            raise LexError(
                f"Unicode codepoint U+{codepoint:04X} is out of range",
                line=line,
                col=col,
            ) from exc

    def _lex_hex_escape(self, line: int, col: int) -> str:
        """
        Read exactly two hex digits for a ``\\xHH`` escape.

        Args:
            line: Source line for error reporting.
            col:  Source column for error reporting.

        Returns:
            The character corresponding to the hex value.

        Raises:
            LexError: If fewer than two hex digits follow ``\\x``.
        """
        digits: list[str] = []
        for _ in range(2):
            if self._is_at_end():
                raise LexError("Incomplete \\x escape", line=line, col=col)
            ch = self.peek()
            if ch not in "0123456789abcdefABCDEF":
                raise LexError(
                    f"Non-hex character '{ch}' in \\x escape",
                    line=line,
                    col=col,
                )
            digits.append(self.advance())
        return chr(int("".join(digits), 16))

    # ------------------------------------------------------------------
    # Identifier / keyword lexing
    # ------------------------------------------------------------------

    def lex_identifier(self, first: str, line: int, col: int) -> Token:
        """
        Lex an identifier or a reserved keyword.

        An identifier begins with a letter or underscore and continues with
        letters, digits, or underscores.  After collecting all characters,
        the text is checked against :data:`~python_project.types.KEYWORDS`.

        Args:
            first: The first letter or underscore already consumed.
            line:  Source line of the start of the identifier.
            col:   Source column of the start of the identifier.

        Returns:
            A :class:`Token` of kind ``IDENTIFIER`` (with the name as value)
            or the appropriate keyword kind (with the canonical Python value
            for ``BOOLEAN`` / ``NULL``).

        Examples::

            lexer = Lexer("true and myVar")
            toks = lexer.tokenize()
            toks[0].kind   # TokenKind.BOOLEAN
            toks[0].value  # True
            toks[1].kind   # TokenKind.AND
            toks[2].kind   # TokenKind.IDENTIFIER
        """
        buf = [first]
        while self.peek().isalnum() or self.peek() == "_":
            buf.append(self.advance())
        word = "".join(buf)

        if word in KEYWORDS:
            kind = KEYWORDS[word]
            if kind == TokenKind.BOOLEAN:
                value: object = word == "true"
            elif kind == TokenKind.NULL:
                value = None
            else:
                value = word
            return Token(kind, value, line, col)

        return Token(TokenKind.IDENTIFIER, word, line, col)

    # ------------------------------------------------------------------
    # Operator and punctuation lexing
    # ------------------------------------------------------------------

    def _lex_operator(self, ch: str, line: int, col: int) -> Token | None:
        """
        Lex a single or double-character operator or punctuation token.

        Single-character tokens: ``+``, ``-``, ``*``, ``%``, ``^``, ``(``,
        ``)``, ``[``, ``]``, ``,``, ``.``, ``;``, ``:``.

        Potential two-character tokens: ``==``, ``!=``, ``<=``, ``>=``.

        Args:
            ch:   The first character (already consumed by :meth:`_scan_token`).
            line: Source line for the token.
            col:  Source column for the token.

        Returns:
            A :class:`Token`, or ``None`` if the character is whitespace that
            was already handled (should not normally occur).

        Raises:
            LexError: On an unexpected character.
        """
        # Single-character tokens
        _SINGLE: dict[str, TokenKind] = {
            "+": TokenKind.PLUS,
            "-": TokenKind.MINUS,
            "*": TokenKind.STAR,
            "%": TokenKind.PERCENT,
            "^": TokenKind.CARET,
            "(": TokenKind.LPAREN,
            ")": TokenKind.RPAREN,
            "[": TokenKind.LBRACKET,
            "]": TokenKind.RBRACKET,
            ",": TokenKind.COMMA,
            ".": TokenKind.DOT,
            ";": TokenKind.SEMICOLON,
            ":": TokenKind.COLON,
        }
        if ch in _SINGLE:
            return Token(_SINGLE[ch], ch, line, col)

        # Slash — operator or division
        if ch == "/":
            return Token(TokenKind.SLASH, ch, line, col)

        # Two-character comparison operators
        if ch == "=":
            if self.match("="):
                return Token(TokenKind.EQ, "==", line, col)
            return Token(TokenKind.ASSIGN, "=", line, col)
        if ch == "!":
            if self.match("="):
                return Token(TokenKind.NEQ, "!=", line, col)
            raise LexError(
                "Unexpected character '!'; did you mean '!='?", line=line, col=col
            )
        if ch == "<":
            if self.match("="):
                return Token(TokenKind.LTE, "<=", line, col)
            return Token(TokenKind.LT, "<", line, col)
        if ch == ">":
            if self.match("="):
                return Token(TokenKind.GTE, ">=", line, col)
            return Token(TokenKind.GT, ">", line, col)

        raise LexError(f"Unexpected character {ch!r}", line=line, col=col)

    # ------------------------------------------------------------------
    # Utility / inspection helpers
    # ------------------------------------------------------------------

    def current_lexeme(self) -> str:
        """
        Return the substring of the source from :attr:`_start` to
        :attr:`_current` (the text of the token currently being lexed).

        Returns:
            The raw source text for the current token scan.
        """
        return self._source[self._start : self._current]

    def remaining_source(self) -> str:
        """
        Return the as-yet-unprocessed portion of the source string.

        Returns:
            The substring from :attr:`_current` to the end.
        """
        return self._source[self._current :]

    def source_line(self, line_number: int) -> str:
        """
        Return the full text of a specific source line (1-based index).

        Useful for producing contextual error messages.

        Args:
            line_number: The 1-based line number to retrieve.

        Returns:
            The line text (without the trailing newline), or ``""`` if the
            line number is out of range.
        """
        lines = self._source.splitlines()
        idx = line_number - 1
        if 0 <= idx < len(lines):
            return lines[idx]
        return ""

    def position_pointer(self, col: int) -> str:
        """
        Return a caret string ``"^"`` positioned under the given column.

        Used for producing nicely-formatted error messages.

        Args:
            col: The 1-based column to point at.

        Returns:
            A string of spaces followed by ``"^"``.

        Examples::

            lexer.position_pointer(5)  # "    ^"
        """
        return " " * max(0, col - 1) + "^"

    def error_context(self, line: int, col: int) -> str:
        """
        Build a multi-line error context string showing the source line and
        a caret pointing to the error position.

        Args:
            line: The 1-based source line.
            col:  The 1-based column.

        Returns:
            A string containing the source line and a pointer line.
        """
        src_line = self.source_line(line)
        pointer = self.position_pointer(col)
        return f"{src_line}\n{pointer}"

    def count_lines(self) -> int:
        """
        Return the total number of lines in the source.

        Returns:
            The number of newline-separated lines.
        """
        return self._source.count("\n") + 1

    def count_tokens(self) -> int:
        """
        Return the number of tokens produced by the most recent call to
        :meth:`tokenize` (including the EOF token).

        Returns:
            Non-negative integer token count.
        """
        return len(self._tokens)

    def tokens_of_kind(self, kind: TokenKind) -> list[Token]:
        """
        Return all tokens from the last tokenization run that match the
        given kind.

        Args:
            kind: The :class:`~python_project.types.TokenKind` to filter by.

        Returns:
            A (possibly empty) list of matching :class:`Token` objects.
        """
        return [t for t in self._tokens if t.kind == kind]

    @staticmethod
    def quick_tokenize(source: str) -> list[Token]:
        """
        Convenience static method: create a Lexer and immediately tokenize.

        Args:
            source: The source string to tokenize.

        Returns:
            A list of tokens including the trailing ``EOF`` token.

        Raises:
            LexError: On any lexical error.

        Examples::

            tokens = Lexer.quick_tokenize("1 + 2")
        """
        return Lexer(source).tokenize()

    # ------------------------------------------------------------------
    # Validation helpers
    # ------------------------------------------------------------------

    def validate_balanced_parens(self, tokens: list[Token]) -> bool:
        """
        Check that parentheses are balanced in the provided token list.

        This is a static-analysis helper; it does not affect the lexer state.

        Args:
            tokens: A list of tokens (typically from :meth:`tokenize`).

        Returns:
            ``True`` if every ``LPAREN`` is matched by a corresponding
            ``RPAREN``, ``False`` otherwise.
        """
        depth = 0
        for tok in tokens:
            if tok.kind == TokenKind.LPAREN:
                depth += 1
            elif tok.kind == TokenKind.RPAREN:
                depth -= 1
                if depth < 0:
                    return False
        return depth == 0

    def validate_balanced_brackets(self, tokens: list[Token]) -> bool:
        """
        Check that square brackets are balanced in the provided token list.

        Args:
            tokens: A list of tokens.

        Returns:
            ``True`` if brackets are balanced, ``False`` otherwise.
        """
        depth = 0
        for tok in tokens:
            if tok.kind == TokenKind.LBRACKET:
                depth += 1
            elif tok.kind == TokenKind.RBRACKET:
                depth -= 1
                if depth < 0:
                    return False
        return depth == 0

    def has_unterminated_string(self) -> bool:
        """
        Perform a quick scan to determine if the source appears to contain
        an unterminated string literal.

        This is a best-effort heuristic and may produce false negatives for
        complex inputs with escaped quotes.

        Returns:
            ``True`` if the source likely has an unterminated string,
            ``False`` otherwise.
        """
        in_str: str | None = None
        i = 0
        while i < len(self._source):
            ch = self._source[i]
            if in_str is None:
                if ch in ('"', "'"):
                    in_str = ch
            else:
                if ch == "\\" and i + 1 < len(self._source):
                    i += 2
                    continue
                if ch == in_str:
                    in_str = None
            i += 1
        return in_str is not None

    def token_at_offset(self, char_offset: int) -> Token | None:
        """
        Return the token whose source span contains the given character offset.

        Performs a linear scan over :attr:`_tokens`.  Requires that
        :meth:`tokenize` has been called.

        Args:
            char_offset: Zero-based character index into the source.

        Returns:
            The matching :class:`Token`, or ``None`` if not found.
        """
        # Rebuild a rough offset map from line/col
        lines = self._source.splitlines(keepends=True)
        line_offsets: list[int] = []
        acc = 0
        for line in lines:
            line_offsets.append(acc)
            acc += len(line)

        for tok in self._tokens:
            if tok.kind == TokenKind.EOF:
                continue
            tok_line_idx = tok.line - 1
            if tok_line_idx < len(line_offsets):
                tok_offset = line_offsets[tok_line_idx] + tok.col - 1
                tok_end = tok_offset + len(str(tok.value))
                if tok_offset <= char_offset < tok_end:
                    return tok
        return None

    def all_string_values(self) -> list[str]:
        """
        Return the decoded values of every STRING token produced by the last
        tokenization run.

        Returns:
            A list of Python strings.
        """
        return [str(t.value) for t in self._tokens if t.kind == TokenKind.STRING]

    def all_number_values(self) -> list[int | float]:
        """
        Return the numeric values of every NUMBER token produced by the last
        tokenization run.

        Returns:
            A list of ``int`` or ``float`` values.
        """
        result: list[int | float] = []
        for t in self._tokens:
            if t.kind == TokenKind.NUMBER:
                v = t.value
                if isinstance(v, (int, float)):
                    result.append(v)
        return result

    def all_identifiers(self) -> list[str]:
        """
        Return the names of every IDENTIFIER token produced by the last
        tokenization run.

        Returns:
            A list of identifier name strings.
        """
        return [str(t.value) for t in self._tokens if t.kind == TokenKind.IDENTIFIER]

    def dump(self) -> str:
        """
        Return a human-readable dump of all tokens produced by the last
        tokenization run, one token per line.

        Returns:
            A multi-line string listing each token.
        """
        lines: list[str] = []
        for tok in self._tokens:
            lines.append(
                f"  [{tok.line:3d}:{tok.col:2d}] {tok.kind.name:<12s}  {tok.value!r}"
            )
        return "\n".join(lines)

    def __repr__(self) -> str:
        """Return a concise representation of the Lexer state."""
        return (
            f"Lexer(pos={self._current}/{len(self._source)}, "
            f"line={self._line}, tokens={len(self._tokens)})"
        )
