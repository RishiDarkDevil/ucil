"""
parser.py — Recursive descent parser for the python_project expression language.

The :class:`Parser` accepts a flat list of :class:`~python_project.types.Token`
objects (produced by the :class:`~python_project.lexer.Lexer`) and builds a
typed AST rooted at a :class:`~python_project.types.Program` node.

Grammar (informal)
------------------
::

    program      → expr ( ";" expr )* EOF
    expr         → let_expr | if_expr | or_expr
    let_expr     → "let" IDENTIFIER "=" expr "in" expr
    if_expr      → "if" expr "then"? expr "else" expr
    or_expr      → and_expr ( "or" and_expr )*
    and_expr     → not_expr ( "and" not_expr )*
    not_expr     → "not" not_expr | comparison
    comparison   → addition ( ( "==" | "!=" | "<" | "<=" | ">" | ">=" ) addition )*
    addition     → multiplication ( ( "+" | "-" ) multiplication )*
    multiplication → unary ( ( "*" | "/" | "%" ) unary )*
    unary        → "-" unary | power
    power        → call ( "^" unary )*
    call         → primary ( "(" args ")" )*
    primary      → NUMBER | STRING | BOOLEAN | NULL | IDENTIFIER
                 | "(" expr ")"
                 | "[" ( expr ( "," expr )* )? "]"
    args         → ( expr ( "," expr )* )?

The parser implements full error recovery via :meth:`Parser.synchronize`.
"""

from __future__ import annotations

from python_project.types import (
    ASTNode,
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
    Token,
    TokenKind,
    UnaryOp,
)


# ---------------------------------------------------------------------------
# Synchronisation sentinel token kinds
# ---------------------------------------------------------------------------

# When error-recovering, the parser advances until it sees one of these.
_SYNC_TOKENS: frozenset[TokenKind] = frozenset(
    {
        TokenKind.SEMICOLON,
        TokenKind.EOF,
        TokenKind.RPAREN,
        TokenKind.RBRACKET,
        TokenKind.IF,
        TokenKind.LET,
    }
)


# ---------------------------------------------------------------------------
# Parser class
# ---------------------------------------------------------------------------


class Parser:
    """
    Recursive-descent parser that converts a token stream into an AST.

    The parser is single-pass and does not backtrack.  Operator precedence
    is handled by the call hierarchy (deeper methods bind more tightly).

    Attributes:
        _tokens:  The flat list of tokens to parse.
        _pos:     Current index into ``_tokens``.
        _errors:  Accumulated parse errors (for error-recovery mode).

    Examples::

        from python_project.lexer import Lexer
        from python_project.parser import Parser

        tokens = Lexer("1 + 2 * 3").tokenize()
        tree = Parser(tokens).parse()
        # Program([BinaryOp('+', NumberLit(1), BinaryOp('*', NumberLit(2), NumberLit(3)))])
    """

    def __init__(self, tokens: list[Token]) -> None:
        """
        Initialise the Parser with a token list.

        Args:
            tokens: The token list returned by :meth:`~python_project.lexer.Lexer.tokenize`.
                    Must end with an ``EOF`` token.
        """
        self._tokens: list[Token] = tokens
        self._pos: int = 0
        self._errors: list[ParseError] = []

    # ------------------------------------------------------------------
    # Public interface
    # ------------------------------------------------------------------

    def parse(self) -> Program:
        """
        Parse the entire token stream and return the root :class:`Program` node.

        A program consists of one or more expressions separated by semicolons.
        The last expression's value becomes the program's value when evaluated.

        Returns:
            A :class:`~python_project.types.Program` node containing all
            top-level expression nodes.

        Raises:
            ParseError: On the first unrecoverable parse error.  In
                        error-recovery mode, a :class:`ParseError` is raised
                        after the full parse if any errors were accumulated.

        Examples::

            tree = Parser(Lexer("1 + 2").tokenize()).parse()
        """
        body = self.parse_program()
        return Program(body=body, line=1, col=1)

    def errors(self) -> list[ParseError]:
        """
        Return the list of parse errors accumulated during the last parse.

        Returns:
            A (possibly empty) list of :class:`ParseError` instances.
        """
        return list(self._errors)

    def has_errors(self) -> bool:
        """
        Return True if any parse errors were recorded during the last parse.

        Returns:
            ``True`` when :attr:`_errors` is non-empty.
        """
        return len(self._errors) > 0

    # ------------------------------------------------------------------
    # Token navigation helpers
    # ------------------------------------------------------------------

    def peek(self) -> Token:
        """
        Return the current token without consuming it.

        Returns:
            The :class:`Token` at the current position.  If past the end of
            the token list the last token (``EOF``) is returned.
        """
        if self._pos >= len(self._tokens):
            return self._tokens[-1]
        return self._tokens[self._pos]

    def peek_next(self) -> Token:
        """
        Return the token *one ahead* of the current position without
        consuming either token.

        Returns:
            The next-next :class:`Token`, or the last ``EOF`` token if
            beyond the end.
        """
        pos = self._pos + 1
        if pos >= len(self._tokens):
            return self._tokens[-1]
        return self._tokens[pos]

    def peek_at(self, offset: int) -> Token:
        """
        Return the token at ``_pos + offset`` without consuming anything.

        Args:
            offset: Distance ahead of the current position (0 = current).

        Returns:
            The :class:`Token` at ``_pos + offset`` or the final ``EOF``.
        """
        pos = self._pos + offset
        if pos >= len(self._tokens):
            return self._tokens[-1]
        return self._tokens[pos]

    def advance(self) -> Token:
        """
        Consume and return the current token, advancing the position.

        Returns:
            The token that was at the current position before advancing.
        """
        tok = self.peek()
        if not self._is_at_end():
            self._pos += 1
        return tok

    def check(self, *kinds: TokenKind) -> bool:
        """
        Return True if the current token's kind is one of the provided kinds
        without consuming it.

        Args:
            *kinds: One or more :class:`~python_project.types.TokenKind` values.

        Returns:
            ``True`` if the current token matches any of the given kinds.
        """
        return self.peek().kind in kinds

    def match(self, *kinds: TokenKind) -> Token | None:
        """
        If the current token matches one of the provided kinds, consume and
        return it.  Otherwise return ``None`` without advancing.

        Args:
            *kinds: Token kinds to test.

        Returns:
            The consumed :class:`Token`, or ``None``.
        """
        if self.check(*kinds):
            return self.advance()
        return None

    def expect(self, kind: TokenKind, message: str) -> Token:
        """
        Consume the current token if it matches ``kind``, otherwise raise a
        :class:`ParseError`.

        Args:
            kind:    The expected :class:`~python_project.types.TokenKind`.
            message: Human-readable error description used in the exception.

        Returns:
            The consumed :class:`Token`.

        Raises:
            ParseError: If the current token does not match ``kind``.
        """
        if self.check(kind):
            return self.advance()
        tok = self.peek()
        raise ParseError(
            f"{message} — got {tok.kind.name} {tok.value!r} instead",
            line=tok.line,
            col=tok.col,
        )

    def _is_at_end(self) -> bool:
        """
        Return True if the current token is an ``EOF`` token.

        Returns:
            ``True`` when :meth:`peek` returns a token of kind ``EOF``.
        """
        return self.peek().kind == TokenKind.EOF

    # ------------------------------------------------------------------
    # Error recovery
    # ------------------------------------------------------------------

    def synchronize(self) -> None:
        """
        Advance the parser past erroneous tokens until a plausible statement
        boundary is reached.

        This implements panic-mode error recovery.  The method advances until
        it sees a token in :data:`_SYNC_TOKENS` or reaches ``EOF``.  It is
        called after recording a :class:`ParseError` to allow the parse to
        continue and collect additional errors.
        """
        while not self._is_at_end():
            if self.peek().kind in _SYNC_TOKENS:
                return
            self.advance()

    def _record_error(self, error: ParseError) -> None:
        """
        Record a :class:`ParseError` without immediately raising.

        Used by error-recovery code paths.

        Args:
            error: The error to record.
        """
        self._errors.append(error)

    # ------------------------------------------------------------------
    # Top-level program parsing
    # ------------------------------------------------------------------

    def parse_program(self) -> list[ASTNode]:
        """
        Parse a sequence of semicolon-separated expressions.

        Returns:
            A list of top-level :class:`~python_project.types.ASTNode` objects.
            The list has at least one element unless the input is empty (in
            which case a single :class:`~python_project.types.NullLit` is
            returned).

        Examples::

            Parser(Lexer("1; 2; 3").tokenize()).parse_program()
            # [NumberLit(1), NumberLit(2), NumberLit(3)]
        """
        stmts: list[ASTNode] = []

        # Handle completely empty input
        if self._is_at_end():
            return [NullLit(line=1, col=1)]

        # Parse first expression
        stmts.append(self.parse_expr())

        # Parse remaining semicolon-separated expressions
        while self.match(TokenKind.SEMICOLON) and not self._is_at_end():
            if self._is_at_end():
                break
            stmts.append(self.parse_expr())

        return stmts

    # ------------------------------------------------------------------
    # Expression parsing — top level
    # ------------------------------------------------------------------

    def parse_expr(self) -> ASTNode:
        """
        Parse a single expression.

        Delegates to :meth:`parse_let_expr` or :meth:`parse_if_expr` for
        special forms, otherwise falls through to :meth:`parse_or`.

        Returns:
            An :class:`~python_project.types.ASTNode`.
        """
        tok = self.peek()
        if tok.kind == TokenKind.LET:
            return self.parse_let_expr()
        if tok.kind == TokenKind.IF:
            return self.parse_if_expr()
        return self.parse_or()

    # ------------------------------------------------------------------
    # Special forms
    # ------------------------------------------------------------------

    def parse_let_expr(self) -> LetExpr:
        """
        Parse a ``let`` binding expression.

        Grammar::

            let_expr → "let" IDENTIFIER "=" expr "in" expr

        Returns:
            A :class:`~python_project.types.LetExpr` node.

        Raises:
            ParseError: If the grammar is not followed (missing identifier,
                        missing ``=``, or missing ``in``).

        Examples::

            Parser(Lexer("let x = 5 in x + 1").tokenize()).parse_expr()
            # LetExpr(name='x', binding=NumberLit(5), body=BinaryOp('+', ...))
        """
        let_tok = self.expect(TokenKind.LET, "Expected 'let'")
        name_tok = self.expect(
            TokenKind.IDENTIFIER, "Expected variable name after 'let'"
        )
        self.expect(TokenKind.ASSIGN, "Expected '=' after variable name in let binding")
        binding = self.parse_expr()
        self.expect(TokenKind.IN, "Expected 'in' after binding expression in let")
        body = self.parse_expr()

        name = str(name_tok.value)
        return LetExpr(
            name=name,
            binding=binding,
            body=body,
            line=let_tok.line,
            col=let_tok.col,
        )

    def parse_if_expr(self) -> IfExpr:
        """
        Parse an ``if`` conditional expression.

        Grammar::

            if_expr → "if" expr "then"? expr "else" expr

        The ``then`` keyword is optional for compatibility with inline
        conditional syntax.

        Returns:
            An :class:`~python_project.types.IfExpr` node.  The
            ``else_branch`` is set to :class:`~python_project.types.NullLit`
            if the ``else`` clause is omitted.

        Raises:
            ParseError: On a malformed if expression.

        Examples::

            Parser(Lexer("if x > 0 then x else 0 - x").tokenize()).parse_expr()
        """
        if_tok = self.expect(TokenKind.IF, "Expected 'if'")
        condition = self.parse_or()

        # Optional 'then' keyword
        self.match(TokenKind.THEN)

        then_branch = self.parse_expr()

        else_branch: ASTNode | None
        if self.match(TokenKind.ELSE):
            else_branch = self.parse_expr()
        else:
            else_branch = NullLit(line=if_tok.line, col=if_tok.col)

        return IfExpr(
            condition=condition,
            then_branch=then_branch,
            else_branch=else_branch,
            line=if_tok.line,
            col=if_tok.col,
        )

    # ------------------------------------------------------------------
    # Binary operator parsing — precedence ladder
    # ------------------------------------------------------------------

    def parse_or(self) -> ASTNode:
        """
        Parse a logical ``or`` expression.

        Grammar::

            or_expr → and_expr ( "or" and_expr )*

        ``or`` is left-associative with the lowest binary precedence.

        Returns:
            An :class:`~python_project.types.ASTNode` — either a
            :class:`~python_project.types.BinaryOp` with kind ``OR`` or
            whatever :meth:`parse_and` returns.

        Examples::

            Parser(Lexer("a or b or c").tokenize()).parse_or()
            # BinaryOp('or', BinaryOp('or', a, b), c)
        """
        left = self.parse_and()
        while True:
            op_tok = self.match(TokenKind.OR)
            if op_tok is None:
                break
            right = self.parse_and()
            left = BinaryOp(
                op=op_tok,
                left=left,
                right=right,
                line=op_tok.line,
                col=op_tok.col,
            )
        return left

    def parse_and(self) -> ASTNode:
        """
        Parse a logical ``and`` expression.

        Grammar::

            and_expr → not_expr ( "and" not_expr )*

        ``and`` is left-associative and binds more tightly than ``or``.

        Returns:
            An :class:`~python_project.types.ASTNode`.

        Examples::

            Parser(Lexer("a and b and c").tokenize()).parse_and()
            # BinaryOp('and', BinaryOp('and', a, b), c)
        """
        left = self.parse_not()
        while True:
            op_tok = self.match(TokenKind.AND)
            if op_tok is None:
                break
            right = self.parse_not()
            left = BinaryOp(
                op=op_tok,
                left=left,
                right=right,
                line=op_tok.line,
                col=op_tok.col,
            )
        return left

    def parse_not(self) -> ASTNode:
        """
        Parse a logical ``not`` (prefix unary) expression.

        Grammar::

            not_expr → "not" not_expr | comparison

        ``not`` is right-associative (``not not x`` = ``not (not x)``).

        Returns:
            A :class:`~python_project.types.UnaryOp` node or whatever
            :meth:`parse_comparison` returns.

        Examples::

            Parser(Lexer("not true").tokenize()).parse_not()
            # UnaryOp('not', BoolLit(True))
        """
        op_tok = self.match(TokenKind.NOT)
        if op_tok is not None:
            operand = self.parse_not()
            return UnaryOp(
                op=op_tok,
                operand=operand,
                line=op_tok.line,
                col=op_tok.col,
            )
        return self.parse_comparison()

    def parse_comparison(self) -> ASTNode:
        """
        Parse a comparison expression.

        Grammar::

            comparison → addition ( ( "==" | "!=" | "<" | "<=" | ">" | ">=" ) addition )*

        Comparison operators are left-associative.  Chaining (e.g.,
        ``a < b < c``) is parsed as ``(a < b) < c``.

        Returns:
            An :class:`~python_project.types.ASTNode`.

        Examples::

            Parser(Lexer("x >= 10").tokenize()).parse_comparison()
            # BinaryOp('>=', Identifier('x'), NumberLit(10))
        """
        left = self.parse_addition()
        while True:
            op_tok = self.match(
                TokenKind.EQ,
                TokenKind.NEQ,
                TokenKind.LT,
                TokenKind.LTE,
                TokenKind.GT,
                TokenKind.GTE,
            )
            if op_tok is None:
                break
            right = self.parse_addition()
            left = BinaryOp(
                op=op_tok,
                left=left,
                right=right,
                line=op_tok.line,
                col=op_tok.col,
            )
        return left

    def parse_addition(self) -> ASTNode:
        """
        Parse an additive expression.

        Grammar::

            addition → multiplication ( ( "+" | "-" ) multiplication )*

        ``+`` and ``-`` are left-associative.

        Returns:
            An :class:`~python_project.types.ASTNode`.

        Examples::

            Parser(Lexer("1 + 2 - 3").tokenize()).parse_addition()
            # BinaryOp('-', BinaryOp('+', 1, 2), 3)
        """
        left = self.parse_multiplication()
        while True:
            op_tok = self.match(TokenKind.PLUS, TokenKind.MINUS)
            if op_tok is None:
                break
            right = self.parse_multiplication()
            left = BinaryOp(
                op=op_tok,
                left=left,
                right=right,
                line=op_tok.line,
                col=op_tok.col,
            )
        return left

    def parse_multiplication(self) -> ASTNode:
        """
        Parse a multiplicative expression.

        Grammar::

            multiplication → unary ( ( "*" | "/" | "%" ) unary )*

        ``*``, ``/``, and ``%`` are left-associative.

        Returns:
            An :class:`~python_project.types.ASTNode`.

        Examples::

            Parser(Lexer("6 / 2 * 3").tokenize()).parse_multiplication()
            # BinaryOp('*', BinaryOp('/', 6, 2), 3)
        """
        left = self.parse_unary()
        while True:
            op_tok = self.match(TokenKind.STAR, TokenKind.SLASH, TokenKind.PERCENT)
            if op_tok is None:
                break
            right = self.parse_unary()
            left = BinaryOp(
                op=op_tok,
                left=left,
                right=right,
                line=op_tok.line,
                col=op_tok.col,
            )
        return left

    def parse_unary(self) -> ASTNode:
        """
        Parse a unary minus expression.

        Grammar::

            unary → "-" unary | power

        Unary minus is right-associative (``--x`` = ``-(-(x))``).

        Returns:
            A :class:`~python_project.types.UnaryOp` node or whatever
            :meth:`parse_power` returns.

        Examples::

            Parser(Lexer("-5").tokenize()).parse_unary()
            # UnaryOp('-', NumberLit(5))
        """
        op_tok = self.match(TokenKind.MINUS)
        if op_tok is not None:
            operand = self.parse_unary()
            return UnaryOp(
                op=op_tok,
                operand=operand,
                line=op_tok.line,
                col=op_tok.col,
            )
        return self.parse_power()

    def parse_power(self) -> ASTNode:
        """
        Parse an exponentiation expression.

        Grammar::

            power → call ( "^" unary )*

        ``^`` is right-associative: ``2 ^ 3 ^ 2`` = ``2 ^ (3 ^ 2)`` = ``512``.
        The right operand is parsed with :meth:`parse_unary` (not
        :meth:`parse_power`) which naturally gives right-associativity because
        the loop only grabs the immediate right-hand side.

        Returns:
            An :class:`~python_project.types.ASTNode`.

        Examples::

            Parser(Lexer("2 ^ 10").tokenize()).parse_power()
            # BinaryOp('^', NumberLit(2), NumberLit(10))
        """
        base = self.parse_call()
        if not self.check(TokenKind.CARET):
            return base
        op_tok = self.advance()
        exponent = self.parse_unary()
        return BinaryOp(
            op=op_tok,
            left=base,
            right=exponent,
            line=op_tok.line,
            col=op_tok.col,
        )

    # ------------------------------------------------------------------
    # Function call and member access
    # ------------------------------------------------------------------

    def parse_call(self) -> ASTNode:
        """
        Parse a function call expression.

        Grammar::

            call → primary ( "(" args ")" )*

        Multiple consecutive call suffixes are supported (curried calls):
        ``f(a)(b)`` parses as ``Call(Call(f, [a]), [b])``.

        Returns:
            An :class:`~python_project.types.ASTNode` — either a
            :class:`~python_project.types.Call` or whatever
            :meth:`parse_primary` returns.

        Examples::

            Parser(Lexer("max(1, 2)").tokenize()).parse_call()
            # Call(Identifier('max'), [NumberLit(1), NumberLit(2)])
        """
        expr = self.parse_primary()

        while self.check(TokenKind.LPAREN):
            lparen = self.advance()  # consume '('
            args = self.parse_arguments(lparen)
            expr = Call(
                callee=expr,
                args=args,
                line=lparen.line,
                col=lparen.col,
            )

        return expr

    def parse_arguments(self, lparen: Token) -> list[ASTNode]:
        """
        Parse a comma-separated argument list inside parentheses.

        The opening ``(`` has already been consumed by the caller.  This
        method consumes through the closing ``)``.

        Args:
            lparen: The ``(`` token (used for error reporting).

        Returns:
            A (possibly empty) list of expression nodes.

        Raises:
            ParseError: If the argument list is malformed or the closing
                        ``)`` is missing.

        Examples::

            # For the call max(1, 2, 3):
            # returns [NumberLit(1), NumberLit(2), NumberLit(3)]
        """
        args: list[ASTNode] = []

        if self.check(TokenKind.RPAREN):
            self.advance()  # consume ')'
            return args

        args.append(self.parse_expr())

        while self.match(TokenKind.COMMA):
            if self.check(TokenKind.RPAREN):
                break  # Allow trailing comma
            args.append(self.parse_expr())

        self.expect(
            TokenKind.RPAREN,
            f"Expected ')' to close argument list (opened at {lparen.line}:{lparen.col})",
        )
        return args

    # ------------------------------------------------------------------
    # Primary expressions
    # ------------------------------------------------------------------

    def parse_primary(self) -> ASTNode:
        """
        Parse a primary expression — the highest-precedence constructs.

        Handles:
          - Numeric literals → :class:`~python_project.types.NumberLit`
          - String literals  → :class:`~python_project.types.StringLit`
          - Boolean literals → :class:`~python_project.types.BoolLit`
          - Null literal     → :class:`~python_project.types.NullLit`
          - Identifiers      → :class:`~python_project.types.Identifier`
          - Parenthesised expressions
          - List literals    → :class:`~python_project.types.Call` of ``list``

        Returns:
            An :class:`~python_project.types.ASTNode`.

        Raises:
            ParseError: If the current token does not start a valid primary
                        expression.
        """
        tok = self.peek()

        # Number literal
        if tok.kind == TokenKind.NUMBER:
            self.advance()
            v = tok.value
            if not isinstance(v, (int, float)):
                v = 0
            return NumberLit(value=v, line=tok.line, col=tok.col)

        # String literal
        if tok.kind == TokenKind.STRING:
            self.advance()
            s = str(tok.value) if tok.value is not None else ""
            return StringLit(value=s, line=tok.line, col=tok.col)

        # Boolean literal
        if tok.kind == TokenKind.BOOLEAN:
            self.advance()
            b = bool(tok.value)
            return BoolLit(value=b, line=tok.line, col=tok.col)

        # Null literal
        if tok.kind == TokenKind.NULL:
            self.advance()
            return NullLit(line=tok.line, col=tok.col)

        # Identifier
        if tok.kind == TokenKind.IDENTIFIER:
            self.advance()
            name = str(tok.value)
            return Identifier(name=name, line=tok.line, col=tok.col)

        # Parenthesised expression
        if tok.kind == TokenKind.LPAREN:
            return self.parse_grouped()

        # List literal [ e1, e2, ... ]
        if tok.kind == TokenKind.LBRACKET:
            return self.parse_list_literal()

        raise ParseError(
            f"Unexpected token {tok.kind.name} {tok.value!r} — expected an expression",
            line=tok.line,
            col=tok.col,
        )

    def parse_grouped(self) -> ASTNode:
        """
        Parse a parenthesised expression ``"(" expr "}"``.

        The opening ``(`` is consumed here.

        Returns:
            The inner expression node (no wrapper node is created; the
            parentheses affect only precedence).

        Raises:
            ParseError: If the closing ``)`` is missing.

        Examples::

            Parser(Lexer("(1 + 2)").tokenize()).parse_grouped()
            # BinaryOp('+', NumberLit(1), NumberLit(2))
        """
        lparen = self.advance()  # consume '('
        inner = self.parse_expr()
        self.expect(
            TokenKind.RPAREN,
            f"Expected ')' to close parenthesised expression (opened at {lparen.line}:{lparen.col})",
        )
        return inner

    def parse_list_literal(self) -> ASTNode:
        """
        Parse a list literal ``"[" ( expr ("," expr)* )? "]"``.

        List literals are desugared into a :class:`~python_project.types.Call`
        to the built-in function ``list`` with all elements as arguments.
        This keeps the AST uniform — the evaluator handles ``list`` like any
        other built-in.

        Returns:
            A :class:`~python_project.types.Call` node whose callee is
            ``Identifier("list")``.

        Raises:
            ParseError: If the closing ``]`` is missing.

        Examples::

            Parser(Lexer("[1, 2, 3]").tokenize()).parse_list_literal()
            # Call(Identifier('list'), [NumberLit(1), NumberLit(2), NumberLit(3)])
        """
        lbracket = self.advance()  # consume '['
        elements: list[ASTNode] = []

        if not self.check(TokenKind.RBRACKET):
            elements.append(self.parse_expr())
            while self.match(TokenKind.COMMA):
                if self.check(TokenKind.RBRACKET):
                    break
                elements.append(self.parse_expr())

        self.expect(
            TokenKind.RBRACKET,
            f"Expected ']' to close list literal (opened at {lbracket.line}:{lbracket.col})",
        )

        callee = Identifier(name="list", line=lbracket.line, col=lbracket.col)
        return Call(
            callee=callee,
            args=elements,
            line=lbracket.line,
            col=lbracket.col,
        )

    # ------------------------------------------------------------------
    # Utility / inspection helpers
    # ------------------------------------------------------------------

    def current_token(self) -> Token:
        """
        Return the token at the current parse position.

        Returns:
            The current :class:`Token`.
        """
        return self.peek()

    def previous_token(self) -> Token:
        """
        Return the most recently consumed token.

        Returns:
            The :class:`Token` at ``_pos - 1``, or the first token if at
            the beginning.
        """
        pos = max(0, self._pos - 1)
        return self._tokens[pos]

    def remaining_tokens(self) -> list[Token]:
        """
        Return all tokens from the current position to the end (inclusive of EOF).

        Returns:
            A slice of the token list starting at the current position.
        """
        return self._tokens[self._pos :]

    def consumed_count(self) -> int:
        """
        Return how many tokens have been consumed so far.

        Returns:
            The current position index.
        """
        return self._pos

    def token_count(self) -> int:
        """
        Return the total number of tokens in the stream (including EOF).

        Returns:
            ``len(self._tokens)``
        """
        return len(self._tokens)

    def dump_remaining(self) -> str:
        """
        Return a human-readable string listing all remaining (unconsumed) tokens.

        Useful for debugging parse errors.

        Returns:
            A multi-line string.
        """
        lines: list[str] = []
        for tok in self.remaining_tokens():
            lines.append(f"  [{tok.line}:{tok.col}] {tok.kind.name:<12s} {tok.value!r}")
        return "\n".join(lines)

    def context_window(self, size: int = 3) -> list[Token]:
        """
        Return up to ``size`` tokens centred on the current position for
        use in error messages.

        Args:
            size: The radius of the context window (tokens before and after).

        Returns:
            A list of tokens from ``max(0, pos - size)`` to
            ``min(len, pos + size)``.
        """
        lo = max(0, self._pos - size)
        hi = min(len(self._tokens), self._pos + size + 1)
        return self._tokens[lo:hi]

    @staticmethod
    def quick_parse(source: str) -> Program:
        """
        Convenience static method: lex and parse a source string in one call.

        Imports the :class:`~python_project.lexer.Lexer` inline to avoid a
        circular import at module level (though there is none in practice).

        Args:
            source: The source string.

        Returns:
            A :class:`~python_project.types.Program` AST node.

        Raises:
            LexError:   On a lexical error.
            ParseError: On a parse error.
        """
        from python_project.lexer import Lexer  # noqa: PLC0415

        tokens = Lexer(source).tokenize()
        return Parser(tokens).parse()

    def __repr__(self) -> str:
        """Return a concise representation of the Parser state."""
        return (
            f"Parser(pos={self._pos}/{len(self._tokens)}, errors={len(self._errors)})"
        )
