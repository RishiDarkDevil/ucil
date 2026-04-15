//! Recursive-descent parser for arithmetic and boolean expressions.
//!
//! Grammar (informal, precedence from lowest to highest):
//!
//! ```text
//! expr        ::= let_expr | if_expr | or_expr
//! let_expr    ::= 'let' IDENT '=' expr 'in' expr
//! if_expr     ::= 'if' expr 'then' expr 'else' expr
//! or_expr     ::= and_expr ( '||' and_expr )*
//! and_expr    ::= cmp_expr ( '&&' cmp_expr )*
//! cmp_expr    ::= add_expr ( ( '==' | '!=' | '<' | '<=' | '>' | '>=' ) add_expr )?
//! add_expr    ::= mul_expr ( ( '+' | '-' ) mul_expr )*
//! mul_expr    ::= pow_expr ( ( '*' | '/' | '%' ) pow_expr )*
//! pow_expr    ::= unary_expr ( '**' unary_expr )*   (right-associative)
//! unary_expr  ::= ( '-' | '!' ) unary_expr | primary
//! primary     ::= NUMBER | BOOL | IDENT | '(' expr ')'
//! ```
//!
//! The parser operates in two phases:
//! 1. **Tokenisation** — converts the raw input string into a flat `Vec<Token>`.
//! 2. **Parsing** — walks the token stream using recursive descent with
//!    explicit precedence layers.  No Pratt-binding-power table is needed
//!    because the grammar is small enough that layered functions are clearer.

use std::fmt;

// ---------------------------------------------------------------------------
// AST types
// ---------------------------------------------------------------------------

/// Binary operators ordered roughly by ascending precedence so that the
/// discriminant value can double as a quick "precedence tier" lookup in debug
/// output, though the parser does NOT rely on this ordering at runtime.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    /// Logical disjunction (`||`).
    Or,
    /// Logical conjunction (`&&`).
    And,
    /// Equality comparison (`==`).
    Eq,
    /// Inequality comparison (`!=`).
    Ne,
    /// Less-than (`<`).
    Lt,
    /// Less-than-or-equal (`<=`).
    Le,
    /// Greater-than (`>`).
    Gt,
    /// Greater-than-or-equal (`>=`).
    Ge,
    /// Addition (`+`).
    Add,
    /// Subtraction (`-`).
    Sub,
    /// Multiplication (`*`).
    Mul,
    /// Division (`/`).
    Div,
    /// Modulo (`%`).
    Mod,
    /// Exponentiation (`**`), right-associative.
    Pow,
}

impl fmt::Display for BinOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sym = match self {
            Self::Or => "||",
            Self::And => "&&",
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Mod => "%",
            Self::Pow => "**",
        };
        write!(f, "{sym}")
    }
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnOp {
    /// Arithmetic negation (`-`).
    Neg,
    /// Logical negation (`!`).
    Not,
}

impl fmt::Display for UnOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Neg => write!(f, "-"),
            Self::Not => write!(f, "!"),
        }
    }
}

/// The expression AST node.
///
/// Every variant is deliberately kept small — sub-expressions are heap
/// allocated via `Box` so that `Expr` itself stays one-pointer-wide on the
/// stack in the common `Box<Expr>` case.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// A numeric literal, stored as IEEE-754 `f64`.
    Number(f64),

    /// A boolean literal (`true` or `false`).
    Bool(bool),

    /// A bare variable reference, resolved at evaluation time.
    Variable(String),

    /// A binary operation such as `lhs + rhs`.
    BinaryOp {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },

    /// A unary prefix operation such as `-x` or `!flag`.
    UnaryOp { op: UnOp, operand: Box<Expr> },

    /// A conditional expression: `if cond then a else b`.
    If {
        cond: Box<Expr>,
        then_: Box<Expr>,
        else_: Box<Expr>,
    },

    /// A let-binding: `let x = value in body`.
    Let {
        name: String,
        value: Box<Expr>,
        body: Box<Expr>,
    },
}

impl fmt::Display for Expr {
    /// Pretty-print the expression tree using a minimal parenthesised
    /// S-expression style.  Useful for debugging and snapshot tests.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(n) => write!(f, "{n}"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Variable(name) => write!(f, "{name}"),
            Self::BinaryOp { op, lhs, rhs } => {
                write!(f, "({lhs} {op} {rhs})")
            }
            Self::UnaryOp { op, operand } => {
                write!(f, "({op}{operand})")
            }
            Self::If { cond, then_, else_ } => {
                write!(f, "(if {cond} then {then_} else {else_})")
            }
            Self::Let { name, value, body } => {
                write!(f, "(let {name} = {value} in {body})")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Parse errors
// ---------------------------------------------------------------------------

/// Position within the source string, measured in byte offset.
/// Used by `ParseError` to point the user at the offending location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// Byte offset of the start of the problematic region.
    pub start: usize,
    /// Byte offset one past the end (exclusive).
    pub end: usize,
}

/// All the ways parsing can fail.
///
/// We deliberately avoid `thiserror` because this is a test fixture crate
/// with no external dependencies — `Display` and `Error` are implemented
/// by hand.
#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    /// The parser encountered a token it did not expect.
    UnexpectedToken {
        expected: String,
        found: String,
        span: Span,
    },
    /// The input ended before the parser found a complete expression.
    UnexpectedEof { expected: String },
    /// A numeric literal could not be converted to `f64`.
    InvalidLiteral { text: String, span: Span },
    /// An unknown or invalid character was found during tokenisation.
    InvalidCharacter { ch: char, offset: usize },
    /// Trailing tokens remain after a complete expression was parsed.
    TrailingInput { span: Span },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedToken {
                expected,
                found,
                span,
            } => {
                write!(
                    f,
                    "unexpected token at {}..{}: expected {expected}, found {found}",
                    span.start, span.end
                )
            }
            Self::UnexpectedEof { expected } => {
                write!(f, "unexpected end of input: expected {expected}")
            }
            Self::InvalidLiteral { text, span } => {
                write!(
                    f,
                    "invalid numeric literal '{text}' at {}..{}",
                    span.start, span.end
                )
            }
            Self::InvalidCharacter { ch, offset } => {
                write!(f, "invalid character '{ch}' at offset {offset}")
            }
            Self::TrailingInput { span } => {
                write!(
                    f,
                    "unexpected trailing input at {}..{}",
                    span.start, span.end
                )
            }
        }
    }
}

impl std::error::Error for ParseError {}

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

/// Tokens produced by the lexer.
///
/// The `span` on each token records the byte range in the original source so
/// that error messages can point at the right location.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

/// The different kinds of token that the lexer produces.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    /// A numeric literal (integer or float).
    Number(f64),
    /// An identifier or keyword.  Keywords (`let`, `in`, `if`, `then`,
    /// `else`, `true`, `false`) are disambiguated during parsing, not
    /// during lexing, to keep the lexer simpler.
    Ident(String),
    // -- single-character operators & punctuation --
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Bang,
    LParen,
    RParen,
    Equals,
    // -- two-character operators --
    StarStar,
    EqEq,
    BangEq,
    LtEq,
    GtEq,
    Lt,
    Gt,
    AmpAmp,
    PipePipe,
    // -- sentinel --
    Eof,
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(n) => write!(f, "{n}"),
            Self::Ident(s) => write!(f, "{s}"),
            Self::Plus => write!(f, "+"),
            Self::Minus => write!(f, "-"),
            Self::Star => write!(f, "*"),
            Self::Slash => write!(f, "/"),
            Self::Percent => write!(f, "%"),
            Self::Bang => write!(f, "!"),
            Self::LParen => write!(f, "("),
            Self::RParen => write!(f, ")"),
            Self::Equals => write!(f, "="),
            Self::StarStar => write!(f, "**"),
            Self::EqEq => write!(f, "=="),
            Self::BangEq => write!(f, "!="),
            Self::LtEq => write!(f, "<="),
            Self::GtEq => write!(f, ">="),
            Self::Lt => write!(f, "<"),
            Self::Gt => write!(f, ">"),
            Self::AmpAmp => write!(f, "&&"),
            Self::PipePipe => write!(f, "||"),
            Self::Eof => write!(f, "<eof>"),
        }
    }
}

// ---------------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------------

/// A simple hand-written lexer (scanner) that converts a source string into
/// a sequence of `Token`s.  It consumes the input character-by-character,
/// skipping whitespace, and accumulates multi-character tokens such as
/// identifiers, numbers, and two-character operators.
struct Lexer<'a> {
    /// The full source input as bytes for fast ASCII scanning.
    src: &'a [u8],
    /// Current byte offset into `src`.
    pos: usize,
}

impl<'a> Lexer<'a> {
    /// Create a new lexer over the given source string.
    fn new(input: &'a str) -> Self {
        Self {
            src: input.as_bytes(),
            pos: 0,
        }
    }

    /// Peek at the current byte without advancing.
    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    /// Peek at the byte `n` positions ahead of `self.pos`.
    fn peek_ahead(&self, n: usize) -> Option<u8> {
        self.src.get(self.pos + n).copied()
    }

    /// Advance by one byte and return it.
    fn advance(&mut self) -> Option<u8> {
        let b = self.src.get(self.pos).copied()?;
        self.pos += 1;
        Some(b)
    }

    /// Skip over ASCII whitespace.
    fn skip_whitespace(&mut self) {
        while let Some(b) = self.peek() {
            if b.is_ascii_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    /// Read a contiguous run of digits (with optional single dot for floats).
    /// Returns the raw text and its span.
    fn read_number(&mut self) -> Result<Token, ParseError> {
        let start = self.pos;
        let mut has_dot = false;

        while let Some(b) = self.peek() {
            if b.is_ascii_digit() {
                self.pos += 1;
            } else if b == b'.' && !has_dot {
                // Only accept the dot if followed by a digit — otherwise it
                // might be a method call or range operator in a richer language.
                if self
                    .peek_ahead(1)
                    .map_or(false, |next| next.is_ascii_digit())
                {
                    has_dot = true;
                    self.pos += 1; // consume '.'
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        let text = std::str::from_utf8(&self.src[start..self.pos])
            .expect("source is valid UTF-8 and we only consumed ASCII digits/dot");

        let value: f64 = text.parse().map_err(|_| ParseError::InvalidLiteral {
            text: text.to_owned(),
            span: Span {
                start,
                end: self.pos,
            },
        })?;

        Ok(Token {
            kind: TokenKind::Number(value),
            span: Span {
                start,
                end: self.pos,
            },
        })
    }

    /// Read an identifier (ASCII alpha / underscore start, then alnum / underscore).
    fn read_ident(&mut self) -> Token {
        let start = self.pos;

        while let Some(b) = self.peek() {
            if b.is_ascii_alphanumeric() || b == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }

        let text = std::str::from_utf8(&self.src[start..self.pos]).expect("ASCII-only identifiers");

        Token {
            kind: TokenKind::Ident(text.to_owned()),
            span: Span {
                start,
                end: self.pos,
            },
        }
    }

    /// Produce the next token, or `Eof` if the input is exhausted.
    fn next_token(&mut self) -> Result<Token, ParseError> {
        self.skip_whitespace();

        let start = self.pos;

        let byte = match self.peek() {
            Some(b) => b,
            None => {
                return Ok(Token {
                    kind: TokenKind::Eof,
                    span: Span {
                        start: self.pos,
                        end: self.pos,
                    },
                });
            }
        };

        // Digits → number literal
        if byte.is_ascii_digit() {
            return self.read_number();
        }

        // Letters or underscore → identifier / keyword
        if byte.is_ascii_alphabetic() || byte == b'_' {
            return Ok(self.read_ident());
        }

        // Otherwise, operator or punctuation.
        self.advance(); // consume the first character

        let kind = match byte {
            b'+' => TokenKind::Plus,
            b'-' => TokenKind::Minus,
            b'/' => TokenKind::Slash,
            b'%' => TokenKind::Percent,
            b'(' => TokenKind::LParen,
            b')' => TokenKind::RParen,

            // '*' or '**'
            b'*' => {
                if self.peek() == Some(b'*') {
                    self.advance();
                    TokenKind::StarStar
                } else {
                    TokenKind::Star
                }
            }

            // '=' or '=='
            b'=' => {
                if self.peek() == Some(b'=') {
                    self.advance();
                    TokenKind::EqEq
                } else {
                    TokenKind::Equals
                }
            }

            // '!' or '!='
            b'!' => {
                if self.peek() == Some(b'=') {
                    self.advance();
                    TokenKind::BangEq
                } else {
                    TokenKind::Bang
                }
            }

            // '<' or '<='
            b'<' => {
                if self.peek() == Some(b'=') {
                    self.advance();
                    TokenKind::LtEq
                } else {
                    TokenKind::Lt
                }
            }

            // '>' or '>='
            b'>' => {
                if self.peek() == Some(b'=') {
                    self.advance();
                    TokenKind::GtEq
                } else {
                    TokenKind::Gt
                }
            }

            // '&&'
            b'&' => {
                if self.peek() == Some(b'&') {
                    self.advance();
                    TokenKind::AmpAmp
                } else {
                    return Err(ParseError::InvalidCharacter {
                        ch: '&',
                        offset: start,
                    });
                }
            }

            // '||'
            b'|' => {
                if self.peek() == Some(b'|') {
                    self.advance();
                    TokenKind::PipePipe
                } else {
                    return Err(ParseError::InvalidCharacter {
                        ch: '|',
                        offset: start,
                    });
                }
            }

            _ => {
                return Err(ParseError::InvalidCharacter {
                    ch: byte as char,
                    offset: start,
                });
            }
        };

        Ok(Token {
            kind,
            span: Span {
                start,
                end: self.pos,
            },
        })
    }

    /// Tokenise the entire remaining input into a vector.
    /// The final token is always `Eof`.
    fn tokenise_all(&mut self) -> Result<Vec<Token>, ParseError> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token()?;
            let is_eof = tok.kind == TokenKind::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        Ok(tokens)
    }
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// A recursive-descent parser that consumes a pre-lexed token stream and
/// produces an `Expr` AST.
///
/// # Usage
///
/// ```ignore
/// let mut parser = Parser::new("1 + 2 * 3");
/// let expr = parser.parse_expr().unwrap();
/// ```
pub struct Parser<'a> {
    /// The raw source, kept around for error messages.
    _source: &'a str,
    /// Pre-lexed token stream.
    tokens: Vec<Token>,
    /// Current index into `self.tokens`.
    cursor: usize,
}

impl<'a> Parser<'a> {
    /// Create a new parser.  Tokenisation happens eagerly in the constructor
    /// so that any lexer errors surface immediately rather than mid-parse.
    pub fn new(input: &'a str) -> Self {
        // We perform tokenisation inside `new` and store the result.
        // If the lexer fails we stash a single Eof token — the actual error
        // will be re-surfaced on the first call to `parse_expr` by re-lexing.
        // This keeps the constructor infallible for ergonomic construction.
        let tokens = {
            let mut lexer = Lexer::new(input);
            lexer.tokenise_all().unwrap_or_else(|_| {
                vec![Token {
                    kind: TokenKind::Eof,
                    span: Span { start: 0, end: 0 },
                }]
            })
        };

        Self {
            _source: input,
            tokens,
            cursor: 0,
        }
    }

    // -- token-stream helpers -----------------------------------------------

    /// Return a reference to the current token without consuming it.
    fn peek(&self) -> &Token {
        self.tokens
            .get(self.cursor)
            .unwrap_or(self.tokens.last().expect("token stream always has Eof"))
    }

    /// Consume the current token and advance the cursor.
    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.cursor.min(self.tokens.len() - 1)];
        if self.cursor < self.tokens.len() {
            self.cursor += 1;
        }
        tok
    }

    /// Return `true` when the cursor is past the last real token (i.e. on or
    /// past the `Eof` sentinel).
    fn at_end(&self) -> bool {
        self.peek().kind == TokenKind::Eof
    }

    /// If the current token matches `kind`, consume and return it.
    /// Otherwise return `None`.
    fn eat(&mut self, kind: &TokenKind) -> Option<Token> {
        if std::mem::discriminant(&self.peek().kind) == std::mem::discriminant(kind) {
            Some(self.advance().clone())
        } else {
            None
        }
    }

    /// Like `eat`, but produces a `ParseError` on mismatch.
    fn expect(&mut self, kind: &TokenKind, label: &str) -> Result<Token, ParseError> {
        let current = self.peek().clone();
        if std::mem::discriminant(&current.kind) == std::mem::discriminant(kind) {
            self.advance();
            Ok(current)
        } else if current.kind == TokenKind::Eof {
            Err(ParseError::UnexpectedEof {
                expected: label.to_owned(),
            })
        } else {
            Err(ParseError::UnexpectedToken {
                expected: label.to_owned(),
                found: format!("{}", current.kind),
                span: current.span,
            })
        }
    }

    /// Check whether the current token is the identifier `word` (used for
    /// keyword detection since the lexer does not distinguish keywords).
    fn peek_keyword(&self, word: &str) -> bool {
        matches!(&self.peek().kind, TokenKind::Ident(w) if w == word)
    }

    /// Consume the current token if it is the keyword `word`.
    fn eat_keyword(&mut self, word: &str) -> bool {
        if self.peek_keyword(word) {
            self.advance();
            true
        } else {
            false
        }
    }

    /// Consume the current token asserting it is the keyword `word`, or
    /// return an error.
    fn expect_keyword(&mut self, word: &str) -> Result<Token, ParseError> {
        if self.peek_keyword(word) {
            Ok(self.advance().clone())
        } else {
            let current = self.peek().clone();
            if current.kind == TokenKind::Eof {
                Err(ParseError::UnexpectedEof {
                    expected: format!("keyword '{word}'"),
                })
            } else {
                Err(ParseError::UnexpectedToken {
                    expected: format!("keyword '{word}'"),
                    found: format!("{}", current.kind),
                    span: current.span,
                })
            }
        }
    }

    // -- public entry point -------------------------------------------------

    /// Parse the entire input as a single expression.
    ///
    /// After a successful parse the cursor should be at `Eof`. Any remaining
    /// tokens produce a `TrailingInput` error so that callers are confident
    /// the whole input was consumed.
    pub fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        // Re-lex to surface lexer errors that were swallowed in `new`.
        if self.tokens.len() == 1 && self.tokens[0].kind == TokenKind::Eof {
            let mut lexer = Lexer::new(self._source);
            self.tokens = lexer.tokenise_all()?;
            self.cursor = 0;
        }

        let expr = self.parse_expr_inner()?;

        // Ensure all input was consumed.
        if !self.at_end() {
            let span = self.peek().span;
            return Err(ParseError::TrailingInput { span });
        }

        Ok(expr)
    }

    // -- recursive-descent layers (private) ---------------------------------

    /// Internal entry: dispatches to `let`, `if`, or the precedence chain.
    fn parse_expr_inner(&mut self) -> Result<Expr, ParseError> {
        if self.peek_keyword("let") {
            return self.parse_let();
        }
        if self.peek_keyword("if") {
            return self.parse_if();
        }
        self.parse_or()
    }

    /// `let <name> = <expr> in <expr>`
    fn parse_let(&mut self) -> Result<Expr, ParseError> {
        self.expect_keyword("let")?;

        let name_tok = self.advance().clone();
        let name = match &name_tok.kind {
            TokenKind::Ident(n) => n.clone(),
            TokenKind::Eof => {
                return Err(ParseError::UnexpectedEof {
                    expected: "variable name after 'let'".to_owned(),
                });
            }
            other => {
                return Err(ParseError::UnexpectedToken {
                    expected: "variable name after 'let'".to_owned(),
                    found: format!("{other}"),
                    span: name_tok.span,
                });
            }
        };

        self.expect(&TokenKind::Equals, "'=' after variable name in let")?;

        let value = self.parse_expr_inner()?;

        self.expect_keyword("in")?;

        let body = self.parse_expr_inner()?;

        Ok(Expr::Let {
            name,
            value: Box::new(value),
            body: Box::new(body),
        })
    }

    /// `if <cond> then <expr> else <expr>`
    fn parse_if(&mut self) -> Result<Expr, ParseError> {
        self.expect_keyword("if")?;

        let cond = self.parse_expr_inner()?;

        self.expect_keyword("then")?;

        let then_ = self.parse_expr_inner()?;

        self.expect_keyword("else")?;

        let else_ = self.parse_expr_inner()?;

        Ok(Expr::If {
            cond: Box::new(cond),
            then_: Box::new(then_),
            else_: Box::new(else_),
        })
    }

    /// Or-expressions: `and_expr ( '||' and_expr )*`
    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_and()?;

        while self.eat(&TokenKind::PipePipe).is_some() {
            let rhs = self.parse_and()?;
            lhs = Expr::BinaryOp {
                op: BinOp::Or,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }

        Ok(lhs)
    }

    /// And-expressions: `cmp_expr ( '&&' cmp_expr )*`
    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_comparison()?;

        while self.eat(&TokenKind::AmpAmp).is_some() {
            let rhs = self.parse_comparison()?;
            lhs = Expr::BinaryOp {
                op: BinOp::And,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }

        Ok(lhs)
    }

    /// Comparison: `add_expr ( cmp_op add_expr )?`
    ///
    /// Comparison is non-associative — `a < b < c` is a syntax error, which
    /// is consistent with most expression languages.
    fn parse_comparison(&mut self) -> Result<Expr, ParseError> {
        let lhs = self.parse_addition()?;

        let op = match &self.peek().kind {
            TokenKind::EqEq => Some(BinOp::Eq),
            TokenKind::BangEq => Some(BinOp::Ne),
            TokenKind::Lt => Some(BinOp::Lt),
            TokenKind::LtEq => Some(BinOp::Le),
            TokenKind::Gt => Some(BinOp::Gt),
            TokenKind::GtEq => Some(BinOp::Ge),
            _ => None,
        };

        if let Some(binop) = op {
            self.advance();
            let rhs = self.parse_addition()?;
            Ok(Expr::BinaryOp {
                op: binop,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            })
        } else {
            Ok(lhs)
        }
    }

    /// Addition / subtraction: `mul_expr ( ('+' | '-') mul_expr )*`
    fn parse_addition(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_multiplication()?;

        loop {
            let op = match &self.peek().kind {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_multiplication()?;
            lhs = Expr::BinaryOp {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }

        Ok(lhs)
    }

    /// Multiplication / division / modulo: `pow_expr ( ('*' | '/' | '%') pow_expr )*`
    fn parse_multiplication(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_power()?;

        loop {
            let op = match &self.peek().kind {
                TokenKind::Star => BinOp::Mul,
                TokenKind::Slash => BinOp::Div,
                TokenKind::Percent => BinOp::Mod,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_power()?;
            lhs = Expr::BinaryOp {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }

        Ok(lhs)
    }

    /// Power (exponentiation): `unary_expr ( '**' power )*`
    ///
    /// Right-associative: `2 ** 3 ** 4` parses as `2 ** (3 ** 4)`.
    /// We achieve right-associativity by recursing into `parse_power` for
    /// the right operand instead of calling the next-lower precedence layer.
    fn parse_power(&mut self) -> Result<Expr, ParseError> {
        let base = self.parse_unary()?;

        if self.eat(&TokenKind::StarStar).is_some() {
            // Right-recursive call gives us right-associativity.
            let exponent = self.parse_power()?;
            Ok(Expr::BinaryOp {
                op: BinOp::Pow,
                lhs: Box::new(base),
                rhs: Box::new(exponent),
            })
        } else {
            Ok(base)
        }
    }

    /// Unary prefix: `('-' | '!') unary | primary`
    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        if self.eat(&TokenKind::Minus).is_some() {
            let operand = self.parse_unary()?;
            return Ok(Expr::UnaryOp {
                op: UnOp::Neg,
                operand: Box::new(operand),
            });
        }

        if self.eat(&TokenKind::Bang).is_some() {
            let operand = self.parse_unary()?;
            return Ok(Expr::UnaryOp {
                op: UnOp::Not,
                operand: Box::new(operand),
            });
        }

        self.parse_primary()
    }

    /// Primary expressions: literals, variables, parenthesised sub-exprs.
    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        let tok = self.peek().clone();

        match &tok.kind {
            // Numeric literal
            TokenKind::Number(n) => {
                let value = *n;
                self.advance();
                Ok(Expr::Number(value))
            }

            // Identifier — may be a keyword-literal (true/false) or a
            // variable reference.
            TokenKind::Ident(name) => {
                let name = name.clone();
                match name.as_str() {
                    "true" => {
                        self.advance();
                        Ok(Expr::Bool(true))
                    }
                    "false" => {
                        self.advance();
                        Ok(Expr::Bool(false))
                    }
                    // `let` and `if` are handled at the top of
                    // `parse_expr_inner`, so if we reach here they are
                    // treated as variable names (this is intentional — it
                    // allows shadowing in creative test inputs).
                    _ => {
                        self.advance();
                        Ok(Expr::Variable(name))
                    }
                }
            }

            // Parenthesised sub-expression
            TokenKind::LParen => {
                self.advance(); // consume '('
                let inner = self.parse_expr_inner()?;
                self.expect(&TokenKind::RParen, "closing ')'")?;
                Ok(inner)
            }

            // Eof — nothing left to parse
            TokenKind::Eof => Err(ParseError::UnexpectedEof {
                expected: "expression".to_owned(),
            }),

            // Anything else is unexpected here
            _ => Err(ParseError::UnexpectedToken {
                expected: "expression (number, variable, '(', 'let', 'if', etc.)".to_owned(),
                found: format!("{}", tok.kind),
                span: tok.span,
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: count symbols in the AST (useful for the transform / util modules)
// ---------------------------------------------------------------------------

/// Recursively count the number of AST nodes in an expression tree.
/// Handy for testing and for the simplifier to decide when a tree is
/// "small enough" to inline.
pub fn count_nodes(expr: &Expr) -> usize {
    match expr {
        Expr::Number(_) | Expr::Bool(_) | Expr::Variable(_) => 1,
        Expr::BinaryOp { lhs, rhs, .. } => 1 + count_nodes(lhs) + count_nodes(rhs),
        Expr::UnaryOp { operand, .. } => 1 + count_nodes(operand),
        Expr::If { cond, then_, else_ } => {
            1 + count_nodes(cond) + count_nodes(then_) + count_nodes(else_)
        }
        Expr::Let { value, body, .. } => 1 + count_nodes(value) + count_nodes(body),
    }
}

/// Collect all free variable names in the expression.
/// A variable is "free" if it is not bound by an enclosing `let`.
pub fn free_variables(expr: &Expr) -> Vec<String> {
    fn collect(expr: &Expr, bound: &[String], out: &mut Vec<String>) {
        match expr {
            Expr::Number(_) | Expr::Bool(_) => {}
            Expr::Variable(name) => {
                if !bound.contains(name) && !out.contains(name) {
                    out.push(name.clone());
                }
            }
            Expr::BinaryOp { lhs, rhs, .. } => {
                collect(lhs, bound, out);
                collect(rhs, bound, out);
            }
            Expr::UnaryOp { operand, .. } => {
                collect(operand, bound, out);
            }
            Expr::If { cond, then_, else_ } => {
                collect(cond, bound, out);
                collect(then_, bound, out);
                collect(else_, bound, out);
            }
            Expr::Let { name, value, body } => {
                collect(value, bound, out);
                let mut extended = bound.to_vec();
                extended.push(name.clone());
                collect(body, &extended, out);
            }
        }
    }

    let mut out = Vec::new();
    collect(expr, &[], &mut out);
    out
}

/// Substitute all free occurrences of `var` with `replacement` in `expr`.
/// Returns a new expression tree (does not mutate in place).
pub fn substitute(expr: &Expr, var: &str, replacement: &Expr) -> Expr {
    match expr {
        Expr::Number(_) | Expr::Bool(_) => expr.clone(),
        Expr::Variable(name) => {
            if name == var {
                replacement.clone()
            } else {
                expr.clone()
            }
        }
        Expr::BinaryOp { op, lhs, rhs } => Expr::BinaryOp {
            op: *op,
            lhs: Box::new(substitute(lhs, var, replacement)),
            rhs: Box::new(substitute(rhs, var, replacement)),
        },
        Expr::UnaryOp { op, operand } => Expr::UnaryOp {
            op: *op,
            operand: Box::new(substitute(operand, var, replacement)),
        },
        Expr::If { cond, then_, else_ } => Expr::If {
            cond: Box::new(substitute(cond, var, replacement)),
            then_: Box::new(substitute(then_, var, replacement)),
            else_: Box::new(substitute(else_, var, replacement)),
        },
        Expr::Let { name, value, body } => {
            let new_value = substitute(value, var, replacement);
            // If the let-binding shadows the variable we are substituting,
            // do NOT descend into the body — the binding captures the name.
            if name == var {
                Expr::Let {
                    name: name.clone(),
                    value: Box::new(new_value),
                    body: body.clone(),
                }
            } else {
                Expr::Let {
                    name: name.clone(),
                    value: Box::new(new_value),
                    body: Box::new(substitute(body, var, replacement)),
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- helpers ------------------------------------------------------------

    /// Parse a string and return the expression, panicking on failure.
    fn parse_ok(input: &str) -> Expr {
        let mut p = Parser::new(input);
        p.parse_expr()
            .unwrap_or_else(|e| panic!("parse failed for '{input}': {e}"))
    }

    /// Parse a string and assert that it fails with a `ParseError`.
    fn parse_err(input: &str) -> ParseError {
        let mut p = Parser::new(input);
        p.parse_expr().unwrap_err()
    }

    // -- basic literals -----------------------------------------------------

    #[test]
    fn test_parse_integer() {
        let expr = parse_ok("42");
        assert_eq!(expr, Expr::Number(42.0));
    }

    #[test]
    fn test_parse_float() {
        let expr = parse_ok("3.14");
        assert_eq!(expr, Expr::Number(3.14));
    }

    #[test]
    fn test_parse_bool_true() {
        let expr = parse_ok("true");
        assert_eq!(expr, Expr::Bool(true));
    }

    #[test]
    fn test_parse_bool_false() {
        let expr = parse_ok("false");
        assert_eq!(expr, Expr::Bool(false));
    }

    #[test]
    fn test_parse_variable() {
        let expr = parse_ok("foo");
        assert_eq!(expr, Expr::Variable("foo".to_owned()));
    }

    // -- arithmetic precedence ----------------------------------------------

    #[test]
    fn test_addition() {
        let expr = parse_ok("1 + 2");
        assert_eq!(
            expr,
            Expr::BinaryOp {
                op: BinOp::Add,
                lhs: Box::new(Expr::Number(1.0)),
                rhs: Box::new(Expr::Number(2.0)),
            }
        );
    }

    #[test]
    fn test_mul_binds_tighter_than_add() {
        // "1 + 2 * 3" should parse as "1 + (2 * 3)"
        let expr = parse_ok("1 + 2 * 3");
        assert_eq!(
            expr,
            Expr::BinaryOp {
                op: BinOp::Add,
                lhs: Box::new(Expr::Number(1.0)),
                rhs: Box::new(Expr::BinaryOp {
                    op: BinOp::Mul,
                    lhs: Box::new(Expr::Number(2.0)),
                    rhs: Box::new(Expr::Number(3.0)),
                }),
            }
        );
    }

    #[test]
    fn test_parenthesised_override() {
        // "(1 + 2) * 3" should override default precedence
        let expr = parse_ok("(1 + 2) * 3");
        assert_eq!(
            expr,
            Expr::BinaryOp {
                op: BinOp::Mul,
                lhs: Box::new(Expr::BinaryOp {
                    op: BinOp::Add,
                    lhs: Box::new(Expr::Number(1.0)),
                    rhs: Box::new(Expr::Number(2.0)),
                }),
                rhs: Box::new(Expr::Number(3.0)),
            }
        );
    }

    #[test]
    fn test_power_right_associative() {
        // "2 ** 3 ** 4" should parse as "2 ** (3 ** 4)"
        let expr = parse_ok("2 ** 3 ** 4");
        assert_eq!(
            expr,
            Expr::BinaryOp {
                op: BinOp::Pow,
                lhs: Box::new(Expr::Number(2.0)),
                rhs: Box::new(Expr::BinaryOp {
                    op: BinOp::Pow,
                    lhs: Box::new(Expr::Number(3.0)),
                    rhs: Box::new(Expr::Number(4.0)),
                }),
            }
        );
    }

    #[test]
    fn test_subtraction_left_associative() {
        // "10 - 3 - 2" should parse as "(10 - 3) - 2"
        let expr = parse_ok("10 - 3 - 2");
        assert_eq!(
            expr,
            Expr::BinaryOp {
                op: BinOp::Sub,
                lhs: Box::new(Expr::BinaryOp {
                    op: BinOp::Sub,
                    lhs: Box::new(Expr::Number(10.0)),
                    rhs: Box::new(Expr::Number(3.0)),
                }),
                rhs: Box::new(Expr::Number(2.0)),
            }
        );
    }

    // -- unary operators ----------------------------------------------------

    #[test]
    fn test_unary_negation() {
        let expr = parse_ok("-5");
        assert_eq!(
            expr,
            Expr::UnaryOp {
                op: UnOp::Neg,
                operand: Box::new(Expr::Number(5.0)),
            }
        );
    }

    #[test]
    fn test_unary_not() {
        let expr = parse_ok("!true");
        assert_eq!(
            expr,
            Expr::UnaryOp {
                op: UnOp::Not,
                operand: Box::new(Expr::Bool(true)),
            }
        );
    }

    #[test]
    fn test_double_negation() {
        let expr = parse_ok("--3");
        assert_eq!(
            expr,
            Expr::UnaryOp {
                op: UnOp::Neg,
                operand: Box::new(Expr::UnaryOp {
                    op: UnOp::Neg,
                    operand: Box::new(Expr::Number(3.0)),
                }),
            }
        );
    }

    // -- boolean / comparison / logical -------------------------------------

    #[test]
    fn test_comparison_eq() {
        let expr = parse_ok("x == 10");
        assert_eq!(
            expr,
            Expr::BinaryOp {
                op: BinOp::Eq,
                lhs: Box::new(Expr::Variable("x".to_owned())),
                rhs: Box::new(Expr::Number(10.0)),
            }
        );
    }

    #[test]
    fn test_logical_and_or_precedence() {
        // "a || b && c" should parse as "a || (b && c)" because && binds
        // tighter than ||.
        let expr = parse_ok("a || b && c");
        assert_eq!(
            expr,
            Expr::BinaryOp {
                op: BinOp::Or,
                lhs: Box::new(Expr::Variable("a".to_owned())),
                rhs: Box::new(Expr::BinaryOp {
                    op: BinOp::And,
                    lhs: Box::new(Expr::Variable("b".to_owned())),
                    rhs: Box::new(Expr::Variable("c".to_owned())),
                }),
            }
        );
    }

    #[test]
    fn test_comparison_operators() {
        // Smoke-test each comparison operator parses to the right BinOp.
        let cases = vec![
            ("1 != 2", BinOp::Ne),
            ("1 < 2", BinOp::Lt),
            ("1 <= 2", BinOp::Le),
            ("1 > 2", BinOp::Gt),
            ("1 >= 2", BinOp::Ge),
        ];
        for (input, expected_op) in cases {
            let expr = parse_ok(input);
            match expr {
                Expr::BinaryOp { op, .. } => assert_eq!(op, expected_op, "failed for '{input}'"),
                other => panic!("expected BinaryOp for '{input}', got {other:?}"),
            }
        }
    }

    // -- let expressions ----------------------------------------------------

    #[test]
    fn test_let_simple() {
        let expr = parse_ok("let x = 5 in x");
        assert_eq!(
            expr,
            Expr::Let {
                name: "x".to_owned(),
                value: Box::new(Expr::Number(5.0)),
                body: Box::new(Expr::Variable("x".to_owned())),
            }
        );
    }

    #[test]
    fn test_let_nested() {
        let expr = parse_ok("let x = 1 in let y = 2 in x + y");
        assert_eq!(
            expr,
            Expr::Let {
                name: "x".to_owned(),
                value: Box::new(Expr::Number(1.0)),
                body: Box::new(Expr::Let {
                    name: "y".to_owned(),
                    value: Box::new(Expr::Number(2.0)),
                    body: Box::new(Expr::BinaryOp {
                        op: BinOp::Add,
                        lhs: Box::new(Expr::Variable("x".to_owned())),
                        rhs: Box::new(Expr::Variable("y".to_owned())),
                    }),
                }),
            }
        );
    }

    // -- if expressions -----------------------------------------------------

    #[test]
    fn test_if_simple() {
        let expr = parse_ok("if true then 1 else 0");
        assert_eq!(
            expr,
            Expr::If {
                cond: Box::new(Expr::Bool(true)),
                then_: Box::new(Expr::Number(1.0)),
                else_: Box::new(Expr::Number(0.0)),
            }
        );
    }

    #[test]
    fn test_if_with_comparison() {
        let expr = parse_ok("if x > 0 then x else -x");
        assert_eq!(
            expr,
            Expr::If {
                cond: Box::new(Expr::BinaryOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Expr::Variable("x".to_owned())),
                    rhs: Box::new(Expr::Number(0.0)),
                }),
                then_: Box::new(Expr::Variable("x".to_owned())),
                else_: Box::new(Expr::UnaryOp {
                    op: UnOp::Neg,
                    operand: Box::new(Expr::Variable("x".to_owned())),
                }),
            }
        );
    }

    #[test]
    fn test_if_nested_in_else() {
        let expr = parse_ok("if a then 1 else if b then 2 else 3");
        assert_eq!(
            expr,
            Expr::If {
                cond: Box::new(Expr::Variable("a".to_owned())),
                then_: Box::new(Expr::Number(1.0)),
                else_: Box::new(Expr::If {
                    cond: Box::new(Expr::Variable("b".to_owned())),
                    then_: Box::new(Expr::Number(2.0)),
                    else_: Box::new(Expr::Number(3.0)),
                }),
            }
        );
    }

    // -- complex / combined expressions -------------------------------------

    #[test]
    fn test_let_with_if_body() {
        let expr = parse_ok("let sign = if x >= 0 then 1 else -1 in sign * x");
        assert_eq!(
            expr,
            Expr::Let {
                name: "sign".to_owned(),
                value: Box::new(Expr::If {
                    cond: Box::new(Expr::BinaryOp {
                        op: BinOp::Ge,
                        lhs: Box::new(Expr::Variable("x".to_owned())),
                        rhs: Box::new(Expr::Number(0.0)),
                    }),
                    then_: Box::new(Expr::Number(1.0)),
                    else_: Box::new(Expr::UnaryOp {
                        op: UnOp::Neg,
                        operand: Box::new(Expr::Number(1.0)),
                    }),
                }),
                body: Box::new(Expr::BinaryOp {
                    op: BinOp::Mul,
                    lhs: Box::new(Expr::Variable("sign".to_owned())),
                    rhs: Box::new(Expr::Variable("x".to_owned())),
                }),
            }
        );
    }

    #[test]
    fn test_modulo_and_division() {
        let expr = parse_ok("10 % 3 + 7 / 2");
        assert_eq!(
            expr,
            Expr::BinaryOp {
                op: BinOp::Add,
                lhs: Box::new(Expr::BinaryOp {
                    op: BinOp::Mod,
                    lhs: Box::new(Expr::Number(10.0)),
                    rhs: Box::new(Expr::Number(3.0)),
                }),
                rhs: Box::new(Expr::BinaryOp {
                    op: BinOp::Div,
                    lhs: Box::new(Expr::Number(7.0)),
                    rhs: Box::new(Expr::Number(2.0)),
                }),
            }
        );
    }

    #[test]
    fn test_deeply_nested_parens() {
        let expr = parse_ok("(((42)))");
        assert_eq!(expr, Expr::Number(42.0));
    }

    #[test]
    fn test_complex_expression_with_all_features() {
        // A kitchen-sink expression exercising let, if, arithmetic, boolean
        // logic, comparisons, unary ops, and parentheses all at once.
        let input = "let a = 2 ** 3 in if a > 5 && !(a == 10) then a * (a - 1) else -a + 1";
        let expr = parse_ok(input);
        // Verify the outermost structure is a Let whose body is an If.
        match &expr {
            Expr::Let { name, body, .. } => {
                assert_eq!(name, "a");
                match body.as_ref() {
                    Expr::If { .. } => {} // correct
                    other => panic!("expected If in body, got {other:?}"),
                }
            }
            other => panic!("expected Let at top level, got {other:?}"),
        }
        // Also verify node count as a sanity check (the exact count is
        // deterministic given the grammar).
        assert_eq!(count_nodes(&expr), 22);
    }

    // -- error cases --------------------------------------------------------

    #[test]
    fn test_error_empty_input() {
        let err = parse_err("");
        assert!(matches!(err, ParseError::UnexpectedEof { .. }));
    }

    #[test]
    fn test_error_unexpected_token() {
        let err = parse_err("+ 5");
        assert!(matches!(err, ParseError::UnexpectedToken { .. }));
    }

    #[test]
    fn test_error_unclosed_paren() {
        let err = parse_err("(1 + 2");
        match &err {
            ParseError::UnexpectedEof { expected } => {
                assert!(
                    expected.contains(")"),
                    "message should mention closing paren"
                );
            }
            other => panic!("expected UnexpectedEof, got {other:?}"),
        }
    }

    #[test]
    fn test_error_trailing_input() {
        let err = parse_err("1 2");
        assert!(matches!(err, ParseError::TrailingInput { .. }));
    }

    #[test]
    fn test_error_invalid_character() {
        let err = parse_err("1 @ 2");
        assert!(matches!(err, ParseError::InvalidCharacter { .. }));
    }

    #[test]
    fn test_error_let_missing_in() {
        let err = parse_err("let x = 5 x");
        // Should fail because 'in' keyword is missing — the parser sees
        // the second 'x' where it expects 'in'.
        match &err {
            ParseError::UnexpectedToken { expected, .. } => {
                assert!(
                    expected.contains("in"),
                    "error should mention 'in', got: {expected}"
                );
            }
            ParseError::TrailingInput { .. } => {
                // Also acceptable — after parsing "let x = 5", the next
                // token 'x' is trailing input if the inner parse consumed
                // just '5'.  Both are valid rejection behaviours.
            }
            other => panic!("expected UnexpectedToken or TrailingInput, got {other:?}"),
        }
    }

    #[test]
    fn test_error_if_missing_else() {
        let err = parse_err("if true then 1");
        assert!(matches!(err, ParseError::UnexpectedEof { .. }));
    }

    // -- display / formatting -----------------------------------------------

    #[test]
    fn test_display_roundtrip_smoke() {
        // The Display impl produces a parenthesised S-expression style.
        // Verify a few cases produce sensible output.
        let expr = parse_ok("1 + 2 * 3");
        let displayed = format!("{expr}");
        assert_eq!(displayed, "(1 + (2 * 3))");
    }

    #[test]
    fn test_display_let() {
        let expr = parse_ok("let x = 5 in x + 1");
        let displayed = format!("{expr}");
        assert_eq!(displayed, "(let x = 5 in (x + 1))");
    }

    #[test]
    fn test_display_if() {
        let expr = parse_ok("if true then 1 else 0");
        let displayed = format!("{expr}");
        assert_eq!(displayed, "(if true then 1 else 0)");
    }

    // -- helper function tests ----------------------------------------------

    #[test]
    fn test_count_nodes_literal() {
        assert_eq!(count_nodes(&Expr::Number(1.0)), 1);
        assert_eq!(count_nodes(&Expr::Bool(true)), 1);
        assert_eq!(count_nodes(&Expr::Variable("x".to_owned())), 1);
    }

    #[test]
    fn test_count_nodes_binary() {
        let expr = parse_ok("1 + 2");
        assert_eq!(count_nodes(&expr), 3); // Add, 1, 2
    }

    #[test]
    fn test_free_variables_simple() {
        let expr = parse_ok("x + y");
        let mut vars = free_variables(&expr);
        vars.sort();
        assert_eq!(vars, vec!["x", "y"]);
    }

    #[test]
    fn test_free_variables_let_binds() {
        // In "let x = 1 in x + y", x is bound and y is free.
        let expr = parse_ok("let x = 1 in x + y");
        let vars = free_variables(&expr);
        assert_eq!(vars, vec!["y"]);
    }

    #[test]
    fn test_free_variables_nested_let() {
        let expr = parse_ok("let x = a in let y = b in x + y + c");
        let mut vars = free_variables(&expr);
        vars.sort();
        assert_eq!(vars, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_substitute_basic() {
        let expr = parse_ok("x + 1");
        let result = substitute(&expr, "x", &Expr::Number(5.0));
        let expected = parse_ok("5 + 1");
        assert_eq!(result, expected);
    }

    #[test]
    fn test_substitute_respects_shadowing() {
        // "let x = 1 in x + y" — substituting x should NOT affect the
        // body because x is shadowed by the let.
        let expr = parse_ok("let x = 1 in x + y");
        let result = substitute(&expr, "x", &Expr::Number(99.0));
        // The value part should be unaffected (it's a literal 1, not x),
        // and the body should remain "x + y" because x is bound.
        let expected = parse_ok("let x = 1 in x + y");
        assert_eq!(result, expected);
    }

    #[test]
    fn test_substitute_in_value_of_let() {
        // "let y = x in y" — substituting x should change the value to 42
        // but not the body (y, not x).
        let expr = parse_ok("let y = x in y");
        let result = substitute(&expr, "x", &Expr::Number(42.0));
        assert_eq!(
            result,
            Expr::Let {
                name: "y".to_owned(),
                value: Box::new(Expr::Number(42.0)),
                body: Box::new(Expr::Variable("y".to_owned())),
            }
        );
    }

    // -- lexer edge cases ---------------------------------------------------

    #[test]
    fn test_lexer_adjacent_operators() {
        // "1+-2" should lex as [Number(1), Plus, Minus, Number(2)]
        let expr = parse_ok("1+-2");
        assert_eq!(
            expr,
            Expr::BinaryOp {
                op: BinOp::Add,
                lhs: Box::new(Expr::Number(1.0)),
                rhs: Box::new(Expr::UnaryOp {
                    op: UnOp::Neg,
                    operand: Box::new(Expr::Number(2.0)),
                }),
            }
        );
    }

    #[test]
    fn test_lexer_star_vs_starstar() {
        // Ensure "a*b" lexes as Star, not partial StarStar.
        let expr = parse_ok("a * b");
        match &expr {
            Expr::BinaryOp { op: BinOp::Mul, .. } => {}
            other => panic!("expected Mul, got {other:?}"),
        }

        // And "a**b" lexes as StarStar.
        let expr2 = parse_ok("a ** b");
        match &expr2 {
            Expr::BinaryOp { op: BinOp::Pow, .. } => {}
            other => panic!("expected Pow, got {other:?}"),
        }
    }

    #[test]
    fn test_lexer_identifiers_with_underscores_and_digits() {
        let expr = parse_ok("foo_bar_123");
        assert_eq!(expr, Expr::Variable("foo_bar_123".to_owned()));
    }

    #[test]
    fn test_parse_error_display() {
        // Ensure Display impls on errors produce human-readable messages.
        let err = ParseError::UnexpectedToken {
            expected: "number".to_owned(),
            found: "+".to_owned(),
            span: Span { start: 0, end: 1 },
        };
        let msg = format!("{err}");
        assert!(msg.contains("unexpected token"));
        assert!(msg.contains("number"));

        let err2 = ParseError::InvalidCharacter { ch: '@', offset: 5 };
        let msg2 = format!("{err2}");
        assert!(msg2.contains("invalid character"));
        assert!(msg2.contains('@'));
    }
}
