//! Advanced evaluation context for the expression language.
//!
//! This module provides [`EvalContext`], a stateful evaluation environment that
//! wraps [`crate::util::Env`] and [`crate::util::eval`] with:
//!
//! - **Step counting** to detect and terminate runaway computations.
//! - **Call-depth tracking** to guard against deeply nested expressions.
//! - **History recording** so callers can review past evaluations.
//! - **Variable injection** (`define`) for a REPL-like experience.
//!
//! On top of that, [`Tracer`] records a structured trace of every expression
//! node entered and exited during evaluation, and [`TracingContext`] combines
//! the two for instrumented runs.
//!
//! Finally, [`Interpreter`] provides a high-level REPL helper with a prelude
//! of mathematical constants and human-readable output formatting.
//!
//! # Error model
//!
//! [`CtxError`] is the error type returned by this module.  It wraps
//! [`crate::util::EvalError`] and adds parse-level, depth-limit, and
//! step-limit variants.  It deliberately does NOT extend `crate::util::EvalError`
//! (they are separate enums) to keep the boundary between the core evaluator
//! and the context layer clean.
//!
//! # Examples
//!
//! ```rust,ignore
//! use crate::eval_ctx::{EvalContext, Value};
//!
//! let mut ctx = EvalContext::new();
//! ctx.define("x", Value::Number(10.0));
//! let result = ctx.eval_str("x + 5").unwrap();
//! assert_eq!(result, Value::Number(15.0));
//! ```

use std::fmt;

use crate::parser::{BinOp, Expr, Parser, UnOp};
use crate::util::{Env, EvalError, Value};

// ---------------------------------------------------------------------------
// CtxError
// ---------------------------------------------------------------------------

/// Errors produced by [`EvalContext`] and [`TracingContext`].
///
/// This is intentionally a separate enum from [`crate::util::EvalError`] so
/// that the context layer can surface its own failure modes (parse failures,
/// depth/step limits) without polluting the core evaluator's error space.
#[derive(Debug, Clone)]
pub enum CtxError {
    /// The input string could not be parsed into a valid expression.
    Parse(String),
    /// The core evaluator returned an error (unbound variable, type mismatch,
    /// division by zero, etc.).
    Eval(EvalError),
    /// Recursive evaluation depth exceeded the configured maximum.
    DepthExceeded {
        /// The maximum call depth that was configured.
        max: usize,
    },
    /// The evaluation took more steps than the configured maximum.
    StepLimitExceeded {
        /// The maximum step count that was configured.
        max: u64,
    },
}

impl fmt::Display for CtxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CtxError::Parse(msg) => write!(f, "parse error: {msg}"),
            CtxError::Eval(inner) => write!(f, "eval error: {inner}"),
            CtxError::DepthExceeded { max } => {
                write!(f, "maximum call depth ({max}) exceeded")
            }
            CtxError::StepLimitExceeded { max } => {
                write!(f, "step limit ({max}) exceeded")
            }
        }
    }
}

impl std::error::Error for CtxError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CtxError::Eval(inner) => Some(inner),
            _ => None,
        }
    }
}

impl From<EvalError> for CtxError {
    fn from(e: EvalError) -> Self {
        CtxError::Eval(e)
    }
}

// ---------------------------------------------------------------------------
// HistoryEntry
// ---------------------------------------------------------------------------

/// A single entry in the evaluation history.
///
/// Records the original source text, the outcome (success or error message),
/// the number of evaluation steps consumed, and the maximum recursion depth
/// reached during that evaluation.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// The source text that was evaluated.
    pub source: String,
    /// The outcome: either a [`Value`] on success, or a human-readable error
    /// description on failure.
    pub result: Result<Value, String>,
    /// How many evaluation steps were consumed.
    pub steps: u64,
    /// The deepest recursion level reached during evaluation.
    pub depth_reached: usize,
}

impl HistoryEntry {
    /// Format this entry as a single human-readable line.
    ///
    /// On success: `"<source> => <value> (N steps, depth D)"`
    /// On failure: `"<source> => ERROR: <message> (N steps, depth D)"`
    pub fn format_line(&self) -> String {
        match &self.result {
            Ok(val) => format!(
                "{} => {} ({} steps, depth {})",
                self.source, val, self.steps, self.depth_reached
            ),
            Err(msg) => format!(
                "{} => ERROR: {} ({} steps, depth {})",
                self.source, msg, self.steps, self.depth_reached
            ),
        }
    }
}

impl fmt::Display for HistoryEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format_line())
    }
}

// ---------------------------------------------------------------------------
// EvalContext
// ---------------------------------------------------------------------------

/// A stateful evaluation context for the expression language.
///
/// Wraps an [`Env`] with step counting, depth tracking, history, and the
/// ability to inject named bindings before evaluation.
///
/// # Limits
///
/// Two configurable limits prevent runaway evaluations:
///
/// - `max_call_depth` (default 64): the maximum recursion depth that
///   [`eval_expr`](Self::eval_expr) will permit before returning
///   [`CtxError::DepthExceeded`].
/// - `max_steps` (default 100,000): the maximum number of AST-node
///   evaluations before returning [`CtxError::StepLimitExceeded`].
///
/// Both limits are checked at every recursive step during evaluation.
pub struct EvalContext {
    /// The variable environment (bindings from `define` and `let`).
    env: Env,
    /// History of all evaluations performed through this context.
    history: Vec<HistoryEntry>,
    /// Current recursion depth during an active evaluation.
    call_depth: usize,
    /// Maximum permitted recursion depth.
    max_call_depth: usize,
    /// Number of AST nodes evaluated so far in the current evaluation.
    step_count: u64,
    /// Maximum permitted steps per evaluation.
    max_steps: u64,
    /// High-water mark of call_depth reached during the current evaluation.
    depth_high_water: usize,
}

impl EvalContext {
    /// Create a new context with default limits.
    ///
    /// - `max_call_depth`: 64
    /// - `max_steps`: 100,000
    #[must_use]
    pub fn new() -> Self {
        Self::with_limits(64, 100_000)
    }

    /// Create a new context with custom limits.
    ///
    /// # Arguments
    ///
    /// * `max_call_depth` - Maximum recursion depth before `DepthExceeded`.
    /// * `max_steps` - Maximum AST-node evaluations before `StepLimitExceeded`.
    #[must_use]
    pub fn with_limits(max_call_depth: usize, max_steps: u64) -> Self {
        Self {
            env: Env::new(),
            history: Vec::new(),
            call_depth: 0,
            max_call_depth,
            step_count: 0,
            max_steps,
            depth_high_water: 0,
        }
    }

    /// Add a named binding to the environment.
    ///
    /// Subsequent evaluations will be able to reference `name` as a variable.
    /// If `name` already exists it is shadowed by the new value.
    pub fn define(&mut self, name: &str, val: Value) {
        self.env = self.env.extend(name.to_owned(), val);
    }

    /// Parse `source` into an expression and evaluate it.
    ///
    /// The result is recorded in history regardless of success or failure.
    ///
    /// # Errors
    ///
    /// Returns [`CtxError::Parse`] if the source cannot be parsed, or any
    /// evaluation-level error from [`eval_expr`](Self::eval_expr).
    pub fn eval_str(&mut self, source: &str) -> Result<Value, CtxError> {
        let mut parser = Parser::new(source);
        let expr = match parser.parse_expr() {
            Ok(e) => e,
            Err(parse_err) => {
                let err_msg = parse_err.to_string();
                self.history.push(HistoryEntry {
                    source: source.to_owned(),
                    result: Err(err_msg.clone()),
                    steps: 0,
                    depth_reached: 0,
                });
                return Err(CtxError::Parse(err_msg));
            }
        };

        // Reset per-evaluation counters.
        self.step_count = 0;
        self.call_depth = 0;
        self.depth_high_water = 0;

        let outcome = self.eval_expr(&expr);

        let steps_used = self.step_count;
        let depth_used = self.depth_high_water;

        match &outcome {
            Ok(val) => {
                self.history.push(HistoryEntry {
                    source: source.to_owned(),
                    result: Ok(val.clone()),
                    steps: steps_used,
                    depth_reached: depth_used,
                });
            }
            Err(e) => {
                self.history.push(HistoryEntry {
                    source: source.to_owned(),
                    result: Err(e.to_string()),
                    steps: steps_used,
                    depth_reached: depth_used,
                });
            }
        }

        outcome
    }

    /// Evaluate an already-parsed expression.
    ///
    /// Increments the step counter and checks both the depth limit and step
    /// limit at every recursive call.
    ///
    /// # Errors
    ///
    /// Returns [`CtxError::DepthExceeded`] or [`CtxError::StepLimitExceeded`]
    /// when limits are hit, or [`CtxError::Eval`] for runtime evaluation
    /// errors.
    pub fn eval_expr(&mut self, expr: &Expr) -> Result<Value, CtxError> {
        // Increment step counter and check limit.
        self.step_count += 1;
        if self.step_count > self.max_steps {
            return Err(CtxError::StepLimitExceeded {
                max: self.max_steps,
            });
        }

        // Increment depth and check limit.
        self.call_depth += 1;
        if self.call_depth > self.max_call_depth {
            self.call_depth -= 1;
            return Err(CtxError::DepthExceeded {
                max: self.max_call_depth,
            });
        }

        // Track the deepest level reached.
        if self.call_depth > self.depth_high_water {
            self.depth_high_water = self.call_depth;
        }

        let result = self.eval_inner(expr);

        self.call_depth -= 1;
        result
    }

    /// Return the evaluation history.
    #[must_use]
    pub fn history(&self) -> &[HistoryEntry] {
        &self.history
    }

    /// Return the most recent history entry, if any.
    #[must_use]
    pub fn last_result(&self) -> Option<&HistoryEntry> {
        self.history.last()
    }

    /// Clear the evaluation history.
    pub fn reset_history(&mut self) {
        self.history.clear();
    }

    /// Return the cumulative step count of the most recent evaluation.
    #[must_use]
    pub fn step_count(&self) -> u64 {
        self.step_count
    }

    /// Return a reference to the current environment.
    #[must_use]
    pub fn env(&self) -> &Env {
        &self.env
    }

    /// Return the configured maximum call depth.
    #[must_use]
    pub fn max_call_depth(&self) -> usize {
        self.max_call_depth
    }

    /// Return the configured maximum step count.
    #[must_use]
    pub fn max_steps(&self) -> u64 {
        self.max_steps
    }

    // -----------------------------------------------------------------------
    // Internal recursive evaluator
    // -----------------------------------------------------------------------

    /// Core recursive evaluator that dispatches on the expression variant.
    ///
    /// Each branch is responsible for recursing via [`eval_expr`](Self::eval_expr)
    /// so that the step/depth checks run on every node.
    fn eval_inner(&mut self, expr: &Expr) -> Result<Value, CtxError> {
        match expr {
            // ── literals ─────────────────────────────────────────────────
            Expr::Number(n) => Ok(Value::Number(*n)),
            Expr::Bool(b) => Ok(Value::Bool(*b)),

            // ── variable lookup ──────────────────────────────────────────
            Expr::Variable(name) => self
                .env
                .get(name)
                .cloned()
                .ok_or_else(|| CtxError::Eval(EvalError::UnboundVariable(name.clone()))),

            // ── binary operations ────────────────────────────────────────
            Expr::BinaryOp { op, lhs, rhs } => self.eval_binary(*op, lhs, rhs),

            // ── unary operations ─────────────────────────────────────────
            Expr::UnaryOp { op, operand } => self.eval_unary(*op, operand),

            // ── if / then / else ─────────────────────────────────────────
            Expr::If { cond, then_, else_ } => {
                let cond_val = self.eval_expr(cond)?;
                match cond_val {
                    Value::Bool(true) => self.eval_expr(then_),
                    Value::Bool(false) => self.eval_expr(else_),
                    other => Err(CtxError::Eval(EvalError::TypeMismatch {
                        op: "if",
                        expected: "bool",
                        got: value_type_name(&other),
                    })),
                }
            }

            // ── let bindings ─────────────────────────────────────────────
            Expr::Let { name, value, body } => {
                let val = self.eval_expr(value)?;
                let old_env = self.env.clone();
                self.env = self.env.extend(name.clone(), val);
                let result = self.eval_expr(body);
                self.env = old_env;
                result
            }
        }
    }

    /// Evaluate a binary operation with short-circuit semantics for `&&`/`||`.
    fn eval_binary(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr) -> Result<Value, CtxError> {
        // Short-circuit: evaluate lhs first; for && and || we may skip rhs.
        match op {
            BinOp::And => {
                let lval = self.eval_expr(lhs)?;
                match lval {
                    Value::Bool(false) => return Ok(Value::Bool(false)),
                    Value::Bool(true) => {
                        let rval = self.eval_expr(rhs)?;
                        return coerce_bool_ctx(rval, "&&");
                    }
                    _ => {
                        return Err(CtxError::Eval(EvalError::TypeMismatch {
                            op: "&&",
                            expected: "bool",
                            got: value_type_name(&lval),
                        }));
                    }
                }
            }
            BinOp::Or => {
                let lval = self.eval_expr(lhs)?;
                match lval {
                    Value::Bool(true) => return Ok(Value::Bool(true)),
                    Value::Bool(false) => {
                        let rval = self.eval_expr(rhs)?;
                        return coerce_bool_ctx(rval, "||");
                    }
                    _ => {
                        return Err(CtxError::Eval(EvalError::TypeMismatch {
                            op: "||",
                            expected: "bool",
                            got: value_type_name(&lval),
                        }));
                    }
                }
            }
            _ => {}
        }

        // Non-short-circuit: evaluate both sides then apply the operator.
        let lval = self.eval_expr(lhs)?;
        let rval = self.eval_expr(rhs)?;
        apply_binop_ctx(op, lval, rval)
    }

    /// Evaluate a unary operation.
    fn eval_unary(&mut self, op: UnOp, operand: &Expr) -> Result<Value, CtxError> {
        let val = self.eval_expr(operand)?;
        apply_unop_ctx(op, val)
    }
}

impl Default for EvalContext {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Context-level operator helpers
// ---------------------------------------------------------------------------

/// Ensure a value is boolean, returning a [`CtxError`] if not.
fn coerce_bool_ctx(val: Value, op: &'static str) -> Result<Value, CtxError> {
    match val {
        Value::Bool(b) => Ok(Value::Bool(b)),
        other => Err(CtxError::Eval(EvalError::TypeMismatch {
            op,
            expected: "bool",
            got: value_type_name(&other),
        })),
    }
}

/// Return a human-readable name for a [`Value`]'s type.
fn value_type_name(val: &Value) -> &'static str {
    match val {
        Value::Number(_) => "number",
        Value::Bool(_) => "bool",
    }
}

/// Apply a binary operator to two already-evaluated values.
fn apply_binop_ctx(op: BinOp, lval: Value, rval: Value) -> Result<Value, CtxError> {
    match (op, &lval, &rval) {
        // ── arithmetic ──────────────────────────────────────────────────
        (BinOp::Add, Value::Number(a), Value::Number(b)) => Ok(Value::Number(a + b)),
        (BinOp::Sub, Value::Number(a), Value::Number(b)) => Ok(Value::Number(a - b)),
        (BinOp::Mul, Value::Number(a), Value::Number(b)) => Ok(Value::Number(a * b)),
        (BinOp::Div, Value::Number(_), Value::Number(b)) if *b == 0.0 => {
            Err(CtxError::Eval(EvalError::DivisionByZero))
        }
        (BinOp::Div, Value::Number(a), Value::Number(b)) => Ok(Value::Number(a / b)),
        (BinOp::Mod, Value::Number(_), Value::Number(b)) if *b == 0.0 => {
            Err(CtxError::Eval(EvalError::DivisionByZero))
        }
        (BinOp::Mod, Value::Number(a), Value::Number(b)) => Ok(Value::Number(a % b)),
        (BinOp::Pow, Value::Number(a), Value::Number(b)) => Ok(Value::Number(a.powf(*b))),

        // ── comparisons ─────────────────────────────────────────────────
        (BinOp::Eq, Value::Number(a), Value::Number(b)) => {
            Ok(Value::Bool((a - b).abs() < f64::EPSILON))
        }
        (BinOp::Ne, Value::Number(a), Value::Number(b)) => {
            Ok(Value::Bool((a - b).abs() >= f64::EPSILON))
        }
        (BinOp::Lt, Value::Number(a), Value::Number(b)) => Ok(Value::Bool(a < b)),
        (BinOp::Le, Value::Number(a), Value::Number(b)) => Ok(Value::Bool(a <= b)),
        (BinOp::Gt, Value::Number(a), Value::Number(b)) => Ok(Value::Bool(a > b)),
        (BinOp::Ge, Value::Number(a), Value::Number(b)) => Ok(Value::Bool(a >= b)),

        // ── boolean (non-short-circuit arms already handled above) ───────
        (BinOp::And, Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a && *b)),
        (BinOp::Or, Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a || *b)),

        // ── type errors ─────────────────────────────────────────────────
        _ => Err(CtxError::Eval(EvalError::TypeMismatch {
            op: binop_name(op),
            expected: "number",
            got: value_type_name(&lval),
        })),
    }
}

/// Apply a unary operator to an already-evaluated value.
fn apply_unop_ctx(op: UnOp, val: Value) -> Result<Value, CtxError> {
    match (op, &val) {
        (UnOp::Neg, Value::Number(n)) => Ok(Value::Number(-n)),
        (UnOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
        (UnOp::Neg, _) => Err(CtxError::Eval(EvalError::TypeMismatch {
            op: "-",
            expected: "number",
            got: value_type_name(&val),
        })),
        (UnOp::Not, _) => Err(CtxError::Eval(EvalError::TypeMismatch {
            op: "!",
            expected: "bool",
            got: value_type_name(&val),
        })),
    }
}

/// Map a [`BinOp`] variant to its symbolic name for error messages.
fn binop_name(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::Pow => "**",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::And => "&&",
        BinOp::Or => "||",
    }
}

// ---------------------------------------------------------------------------
// expr_kind_str helper
// ---------------------------------------------------------------------------

/// Return a static string describing the kind of an [`Expr`] node.
///
/// Used by [`Tracer`] to label enter/exit events without allocating.
fn expr_kind_str(expr: &Expr) -> &'static str {
    match expr {
        Expr::Number(_) => "Number",
        Expr::Bool(_) => "Bool",
        Expr::Variable(_) => "Variable",
        Expr::BinaryOp { op, .. } => match op {
            BinOp::Add => "BinaryOp(+)",
            BinOp::Sub => "BinaryOp(-)",
            BinOp::Mul => "BinaryOp(*)",
            BinOp::Div => "BinaryOp(/)",
            BinOp::Mod => "BinaryOp(%)",
            BinOp::Pow => "BinaryOp(**)",
            BinOp::Eq => "BinaryOp(==)",
            BinOp::Ne => "BinaryOp(!=)",
            BinOp::Lt => "BinaryOp(<)",
            BinOp::Le => "BinaryOp(<=)",
            BinOp::Gt => "BinaryOp(>)",
            BinOp::Ge => "BinaryOp(>=)",
            BinOp::And => "BinaryOp(&&)",
            BinOp::Or => "BinaryOp(||)",
        },
        Expr::UnaryOp { op, .. } => match op {
            UnOp::Neg => "UnaryOp(-)",
            UnOp::Not => "UnaryOp(!)",
        },
        Expr::If { .. } => "If",
        Expr::Let { .. } => "Let",
    }
}

// ---------------------------------------------------------------------------
// TraceEvent
// ---------------------------------------------------------------------------

/// A single event recorded by the [`Tracer`] during expression evaluation.
///
/// Events come in three flavours: entering a node, exiting a node with a
/// value, and recording an error at a node.
#[derive(Debug, Clone)]
pub enum TraceEvent {
    /// The evaluator entered an expression node.
    Enter {
        /// A static string describing the AST node kind (e.g. `"BinaryOp(+)"`).
        expr_kind: &'static str,
        /// The recursion depth at entry.
        depth: usize,
    },
    /// The evaluator exited an expression node successfully.
    Exit {
        /// A static string describing the AST node kind.
        expr_kind: &'static str,
        /// The recursion depth at exit.
        depth: usize,
        /// A string representation of the computed value.
        value: String,
    },
    /// An error occurred while evaluating an expression node.
    Error {
        /// A human-readable description of the error.
        message: String,
        /// The recursion depth where the error occurred.
        depth: usize,
    },
}

impl TraceEvent {
    /// Return the depth associated with this event.
    #[must_use]
    pub fn depth(&self) -> usize {
        match self {
            TraceEvent::Enter { depth, .. }
            | TraceEvent::Exit { depth, .. }
            | TraceEvent::Error { depth, .. } => *depth,
        }
    }

    /// Format a single event as a human-readable string with indentation
    /// proportional to its depth.
    #[must_use]
    pub fn format_indented(&self) -> String {
        let indent = "  ".repeat(self.depth());
        match self {
            TraceEvent::Enter { expr_kind, depth } => {
                format!("{indent}ENTER {expr_kind} [depth={depth}]")
            }
            TraceEvent::Exit {
                expr_kind,
                depth,
                value,
            } => {
                format!("{indent}EXIT  {expr_kind} [depth={depth}] => {value}")
            }
            TraceEvent::Error { message, depth } => {
                format!("{indent}ERROR [depth={depth}]: {message}")
            }
        }
    }
}

impl fmt::Display for TraceEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format_indented())
    }
}

// ---------------------------------------------------------------------------
// Tracer
// ---------------------------------------------------------------------------

/// Records a sequence of [`TraceEvent`]s during expression evaluation.
///
/// The tracer can be enabled or disabled at construction time.  When disabled,
/// all recording methods are no-ops, so the overhead of carrying a tracer
/// through the evaluator is negligible in production.
///
/// # Example
///
/// ```rust,ignore
/// let mut tracer = Tracer::new(true);
/// tracer.enter("BinaryOp(+)", 1);
/// tracer.enter("Number", 2);
/// tracer.exit("Number", 2, "3");
/// tracer.enter("Number", 2);
/// tracer.exit("Number", 2, "4");
/// tracer.exit("BinaryOp(+)", 1, "7");
/// let text = tracer.format_trace();
/// assert!(!text.is_empty());
/// ```
pub struct Tracer {
    /// Accumulated trace events.
    events: Vec<TraceEvent>,
    /// Whether recording is active.
    enabled: bool,
}

impl Tracer {
    /// Create a new tracer.
    ///
    /// When `enabled` is `false`, all recording methods are no-ops.
    #[must_use]
    pub fn new(enabled: bool) -> Self {
        Self {
            events: Vec::new(),
            enabled,
        }
    }

    /// Record an "enter" event if tracing is enabled.
    pub fn enter(&mut self, kind: &'static str, depth: usize) {
        if self.enabled {
            self.events.push(TraceEvent::Enter {
                expr_kind: kind,
                depth,
            });
        }
    }

    /// Record an "exit" event if tracing is enabled.
    pub fn exit(&mut self, kind: &'static str, depth: usize, value: &str) {
        if self.enabled {
            self.events.push(TraceEvent::Exit {
                expr_kind: kind,
                depth,
                value: value.to_owned(),
            });
        }
    }

    /// Record an "error" event if tracing is enabled.
    pub fn error(&mut self, msg: &str, depth: usize) {
        if self.enabled {
            self.events.push(TraceEvent::Error {
                message: msg.to_owned(),
                depth,
            });
        }
    }

    /// Return the accumulated events.
    #[must_use]
    pub fn events(&self) -> &[TraceEvent] {
        &self.events
    }

    /// Clear all recorded events.
    pub fn clear(&mut self) {
        self.events.clear();
    }

    /// Return whether the tracer is enabled.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Enable or disable the tracer.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Return the number of recorded events.
    #[must_use]
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// Format the entire trace as indented text.
    ///
    /// Each event is rendered on its own line using
    /// [`TraceEvent::format_indented`].  The resulting string is suitable for
    /// logging or display in a debugger.
    #[must_use]
    pub fn format_trace(&self) -> String {
        let mut buf = String::new();
        for (i, event) in self.events.iter().enumerate() {
            if i > 0 {
                buf.push('\n');
            }
            buf.push_str(&event.format_indented());
        }
        buf
    }

    /// Return a summary line: "N events, max depth D".
    #[must_use]
    pub fn summary(&self) -> String {
        let max_depth = self.events.iter().map(|e| e.depth()).max().unwrap_or(0);
        format!("{} events, max depth {}", self.events.len(), max_depth)
    }
}

impl Default for Tracer {
    fn default() -> Self {
        Self::new(false)
    }
}

// ---------------------------------------------------------------------------
// TracingContext
// ---------------------------------------------------------------------------

/// A tracing wrapper around [`EvalContext`] that records every enter/exit
/// event through a [`Tracer`].
///
/// The [`eval_traced`](Self::eval_traced) method returns both the evaluation
/// result and a formatted trace string.
pub struct TracingContext {
    /// The underlying evaluation context.
    inner: EvalContext,
    /// The tracer that records events.
    tracer: Tracer,
}

impl TracingContext {
    /// Create a new tracing context with default limits and tracing enabled.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: EvalContext::new(),
            tracer: Tracer::new(true),
        }
    }

    /// Create a tracing context with custom limits.
    #[must_use]
    pub fn with_limits(max_call_depth: usize, max_steps: u64) -> Self {
        Self {
            inner: EvalContext::with_limits(max_call_depth, max_steps),
            tracer: Tracer::new(true),
        }
    }

    /// Add a named binding to the inner context's environment.
    pub fn define(&mut self, name: &str, val: Value) {
        self.inner.define(name, val);
    }

    /// Parse and evaluate `source`, recording a trace.
    ///
    /// Returns a tuple of `(result, formatted_trace)`.
    pub fn eval_traced(&mut self, source: &str) -> (Result<Value, CtxError>, String) {
        self.tracer.clear();

        // Parse the expression.
        let mut parser = Parser::new(source);
        let expr = match parser.parse_expr() {
            Ok(e) => e,
            Err(parse_err) => {
                let msg = parse_err.to_string();
                self.tracer.error(&msg, 0);
                let trace = self.tracer.format_trace();
                return (Err(CtxError::Parse(msg)), trace);
            }
        };

        // Reset inner context counters.
        self.inner.step_count = 0;
        self.inner.call_depth = 0;
        self.inner.depth_high_water = 0;

        let result = self.eval_traced_expr(&expr);

        let steps_used = self.inner.step_count;
        let depth_used = self.inner.depth_high_water;

        // Record to inner history.
        match &result {
            Ok(val) => {
                self.inner.history.push(HistoryEntry {
                    source: source.to_owned(),
                    result: Ok(val.clone()),
                    steps: steps_used,
                    depth_reached: depth_used,
                });
            }
            Err(e) => {
                self.inner.history.push(HistoryEntry {
                    source: source.to_owned(),
                    result: Err(e.to_string()),
                    steps: steps_used,
                    depth_reached: depth_used,
                });
            }
        }

        let trace = self.tracer.format_trace();
        (result, trace)
    }

    /// Return the inner context's history.
    #[must_use]
    pub fn history(&self) -> &[HistoryEntry] {
        self.inner.history()
    }

    /// Return the tracer's most recent events.
    #[must_use]
    pub fn trace_events(&self) -> &[TraceEvent] {
        self.tracer.events()
    }

    /// Return a summary of the most recent trace.
    #[must_use]
    pub fn trace_summary(&self) -> String {
        self.tracer.summary()
    }

    // -----------------------------------------------------------------------
    // Internal traced evaluator
    // -----------------------------------------------------------------------

    /// Recursively evaluate an expression while recording trace events.
    fn eval_traced_expr(&mut self, expr: &Expr) -> Result<Value, CtxError> {
        let kind = expr_kind_str(expr);
        let depth = self.inner.call_depth;

        self.inner.step_count += 1;
        if self.inner.step_count > self.inner.max_steps {
            self.tracer.error(
                &format!("step limit ({}) exceeded", self.inner.max_steps),
                depth,
            );
            return Err(CtxError::StepLimitExceeded {
                max: self.inner.max_steps,
            });
        }

        self.inner.call_depth += 1;
        if self.inner.call_depth > self.inner.max_call_depth {
            self.inner.call_depth -= 1;
            self.tracer.error(
                &format!("depth limit ({}) exceeded", self.inner.max_call_depth),
                depth,
            );
            return Err(CtxError::DepthExceeded {
                max: self.inner.max_call_depth,
            });
        }

        if self.inner.call_depth > self.inner.depth_high_water {
            self.inner.depth_high_water = self.inner.call_depth;
        }

        self.tracer.enter(kind, depth);

        let result = self.eval_traced_inner(expr);

        self.inner.call_depth -= 1;

        match &result {
            Ok(val) => {
                self.tracer.exit(kind, depth, &val.to_string());
            }
            Err(e) => {
                self.tracer.error(&e.to_string(), depth);
            }
        }

        result
    }

    /// Dispatch on expression variant, recursing through `eval_traced_expr`.
    fn eval_traced_inner(&mut self, expr: &Expr) -> Result<Value, CtxError> {
        match expr {
            Expr::Number(n) => Ok(Value::Number(*n)),
            Expr::Bool(b) => Ok(Value::Bool(*b)),

            Expr::Variable(name) => self
                .inner
                .env
                .get(name)
                .cloned()
                .ok_or_else(|| CtxError::Eval(EvalError::UnboundVariable(name.clone()))),

            Expr::BinaryOp { op, lhs, rhs } => self.eval_traced_binary(*op, lhs, rhs),

            Expr::UnaryOp { op, operand } => self.eval_traced_unary(*op, operand),

            Expr::If { cond, then_, else_ } => {
                let cond_val = self.eval_traced_expr(cond)?;
                match cond_val {
                    Value::Bool(true) => self.eval_traced_expr(then_),
                    Value::Bool(false) => self.eval_traced_expr(else_),
                    other => Err(CtxError::Eval(EvalError::TypeMismatch {
                        op: "if",
                        expected: "bool",
                        got: value_type_name(&other),
                    })),
                }
            }

            Expr::Let { name, value, body } => {
                let val = self.eval_traced_expr(value)?;
                let old_env = self.inner.env.clone();
                self.inner.env = self.inner.env.extend(name.clone(), val);
                let result = self.eval_traced_expr(body);
                self.inner.env = old_env;
                result
            }
        }
    }

    /// Evaluate a binary operation with tracing and short-circuit support.
    fn eval_traced_binary(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr) -> Result<Value, CtxError> {
        match op {
            BinOp::And => {
                let lval = self.eval_traced_expr(lhs)?;
                match lval {
                    Value::Bool(false) => return Ok(Value::Bool(false)),
                    Value::Bool(true) => {
                        let rval = self.eval_traced_expr(rhs)?;
                        return coerce_bool_ctx(rval, "&&");
                    }
                    _ => {
                        return Err(CtxError::Eval(EvalError::TypeMismatch {
                            op: "&&",
                            expected: "bool",
                            got: value_type_name(&lval),
                        }));
                    }
                }
            }
            BinOp::Or => {
                let lval = self.eval_traced_expr(lhs)?;
                match lval {
                    Value::Bool(true) => return Ok(Value::Bool(true)),
                    Value::Bool(false) => {
                        let rval = self.eval_traced_expr(rhs)?;
                        return coerce_bool_ctx(rval, "||");
                    }
                    _ => {
                        return Err(CtxError::Eval(EvalError::TypeMismatch {
                            op: "||",
                            expected: "bool",
                            got: value_type_name(&lval),
                        }));
                    }
                }
            }
            _ => {}
        }

        let lval = self.eval_traced_expr(lhs)?;
        let rval = self.eval_traced_expr(rhs)?;
        apply_binop_ctx(op, lval, rval)
    }

    /// Evaluate a unary operation with tracing.
    fn eval_traced_unary(&mut self, op: UnOp, operand: &Expr) -> Result<Value, CtxError> {
        let val = self.eval_traced_expr(operand)?;
        apply_unop_ctx(op, val)
    }
}

impl Default for TracingContext {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Interpreter
// ---------------------------------------------------------------------------

/// A high-level REPL helper that wraps [`EvalContext`] with prelude
/// definitions, formatting, and a configurable prompt string.
///
/// The interpreter automatically defines mathematical constants (`pi`, `e`,
/// `tau`) in the environment at construction time.  Additional definitions
/// can be loaded via [`load_prelude`](Self::load_prelude).
///
/// # Example
///
/// ```rust,ignore
/// let mut interp = Interpreter::new();
/// assert_eq!(interp.eval("2 + 3"), "5");
/// assert!(interp.eval("pi").contains("3.14159"));
/// ```
pub struct Interpreter {
    /// The underlying evaluation context.
    ctx: EvalContext,
    /// The prompt string for REPL display.
    prompt: String,
    /// Source strings of prelude definitions, for introspection.
    prelude: Vec<String>,
}

impl Interpreter {
    /// Well-known mathematical constants loaded into every interpreter.
    const PI: f64 = 3.141_592_653_589_793;
    const E: f64 = 2.718_281_828_459_045;
    const TAU: f64 = 6.283_185_307_179_586;

    /// Create a new interpreter with the default prompt `"> "` and standard
    /// mathematical constants.
    #[must_use]
    pub fn new() -> Self {
        let mut interp = Self {
            ctx: EvalContext::new(),
            prompt: "> ".to_owned(),
            prelude: Vec::new(),
        };
        interp.install_math_constants();
        interp
    }

    /// Create a new interpreter with a custom prompt.
    #[must_use]
    pub fn with_prompt(prompt: &str) -> Self {
        let mut interp = Self {
            ctx: EvalContext::new(),
            prompt: prompt.to_owned(),
            prelude: Vec::new(),
        };
        interp.install_math_constants();
        interp
    }

    /// Load a series of name-expression pairs into the environment.
    ///
    /// Each pair is treated as `let <name> = <expr>` and evaluated
    /// immediately.  The result is stored in the environment so subsequent
    /// evaluations can reference it.
    ///
    /// Definitions that fail to parse or evaluate are silently skipped but
    /// recorded in the prelude list with an error annotation.
    pub fn load_prelude(&mut self, definitions: &[(&str, &str)]) {
        for (name, expr_src) in definitions {
            let source = format!("{expr_src}");
            match self.ctx.eval_str(&source) {
                Ok(val) => {
                    self.ctx.define(name, val);
                    self.prelude.push(format!("{name} = {expr_src}"));
                }
                Err(e) => {
                    self.prelude
                        .push(format!("{name} = {expr_src} [FAILED: {e}]"));
                }
            }
        }
    }

    /// Evaluate `input` and return a formatted result string.
    ///
    /// On success returns the value's display representation.
    /// On failure returns `"ERROR: <message>"`.
    pub fn eval(&mut self, input: &str) -> String {
        match self.ctx.eval_str(input) {
            Ok(val) => format_value(&val),
            Err(e) => format!("ERROR: {e}"),
        }
    }

    /// Return a multi-line summary of the evaluation history.
    ///
    /// Each line is produced by [`HistoryEntry::format_line`].
    #[must_use]
    pub fn history_summary(&self) -> String {
        self.ctx
            .history()
            .iter()
            .map(|entry| entry.format_line())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Return the current prompt string.
    #[must_use]
    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    /// Change the prompt string.
    pub fn set_prompt(&mut self, prompt: &str) {
        self.prompt = prompt.to_owned();
    }

    /// Return the list of prelude definition source strings.
    #[must_use]
    pub fn prelude_definitions(&self) -> &[String] {
        &self.prelude
    }

    /// Return a reference to the underlying [`EvalContext`].
    #[must_use]
    pub fn context(&self) -> &EvalContext {
        &self.ctx
    }

    /// Return a mutable reference to the underlying [`EvalContext`].
    pub fn context_mut(&mut self) -> &mut EvalContext {
        &mut self.ctx
    }

    /// Define a new variable in the environment.
    pub fn define(&mut self, name: &str, val: Value) {
        self.ctx.define(name, val);
    }

    /// Format a prompt + input + result line, like a REPL session.
    pub fn format_repl_line(&self, input: &str, output: &str) -> String {
        format!("{}{}\n{}", self.prompt, input, output)
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Install the standard mathematical constants (pi, e, tau).
    fn install_math_constants(&mut self) {
        self.ctx.define("pi", Value::Number(Self::PI));
        self.ctx.define("e", Value::Number(Self::E));
        self.ctx.define("tau", Value::Number(Self::TAU));
        self.prelude.push(format!("pi = {}", Self::PI));
        self.prelude.push(format!("e = {}", Self::E));
        self.prelude.push(format!("tau = {}", Self::TAU));
    }
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Value formatting helper
// ---------------------------------------------------------------------------

/// Format a [`Value`] for REPL output.
///
/// Numbers that are exact integers are displayed without a decimal point.
/// Booleans display as `true` / `false`.
fn format_value(val: &Value) -> String {
    match val {
        Value::Number(n) => {
            if n.fract() == 0.0 && n.is_finite() {
                format!("{}", *n as i64)
            } else {
                format!("{n}")
            }
        }
        Value::Bool(b) => format!("{b}"),
    }
}

// ---------------------------------------------------------------------------
// Batch evaluator
// ---------------------------------------------------------------------------

/// Evaluate multiple expressions in sequence, accumulating results.
///
/// This is a convenience function for scripts and tests that need to process
/// a batch of expressions through a shared context.
///
/// Returns a vector of `(source, result_string)` pairs.
pub fn batch_eval(sources: &[&str]) -> Vec<(String, String)> {
    let mut ctx = EvalContext::new();
    let mut results = Vec::with_capacity(sources.len());
    for source in sources {
        let output = match ctx.eval_str(source) {
            Ok(val) => format_value(&val),
            Err(e) => format!("ERROR: {e}"),
        };
        results.push(((*source).to_owned(), output));
    }
    results
}

/// Evaluate multiple expressions and return only the successful values.
///
/// Expressions that fail to parse or evaluate are silently dropped.
pub fn batch_eval_ok(sources: &[&str]) -> Vec<Value> {
    let mut ctx = EvalContext::new();
    let mut values = Vec::new();
    for source in sources {
        if let Ok(val) = ctx.eval_str(source) {
            values.push(val);
        }
    }
    values
}

// ---------------------------------------------------------------------------
// Expression analysis helpers
// ---------------------------------------------------------------------------

/// Count how many AST nodes an expression string parses into.
///
/// Returns `None` if the string cannot be parsed.
pub fn count_nodes(source: &str) -> Option<usize> {
    let mut parser = Parser::new(source);
    let expr = parser.parse_expr().ok()?;
    Some(count_expr_nodes(&expr))
}

/// Recursively count all nodes in an [`Expr`] tree.
fn count_expr_nodes(expr: &Expr) -> usize {
    match expr {
        Expr::Number(_) | Expr::Bool(_) | Expr::Variable(_) => 1,
        Expr::BinaryOp { lhs, rhs, .. } => 1 + count_expr_nodes(lhs) + count_expr_nodes(rhs),
        Expr::UnaryOp { operand, .. } => 1 + count_expr_nodes(operand),
        Expr::If { cond, then_, else_ } => {
            1 + count_expr_nodes(cond) + count_expr_nodes(then_) + count_expr_nodes(else_)
        }
        Expr::Let { value, body, .. } => 1 + count_expr_nodes(value) + count_expr_nodes(body),
    }
}

/// Compute the maximum depth of an expression string's AST.
///
/// Returns `None` if the string cannot be parsed.
pub fn expr_depth(source: &str) -> Option<usize> {
    let mut parser = Parser::new(source);
    let expr = parser.parse_expr().ok()?;
    Some(compute_depth(&expr))
}

/// Recursively compute the maximum depth of an [`Expr`] tree.
fn compute_depth(expr: &Expr) -> usize {
    match expr {
        Expr::Number(_) | Expr::Bool(_) | Expr::Variable(_) => 1,
        Expr::BinaryOp { lhs, rhs, .. } => 1 + compute_depth(lhs).max(compute_depth(rhs)),
        Expr::UnaryOp { operand, .. } => 1 + compute_depth(operand),
        Expr::If { cond, then_, else_ } => {
            1 + compute_depth(cond)
                .max(compute_depth(then_))
                .max(compute_depth(else_))
        }
        Expr::Let { value, body, .. } => 1 + compute_depth(value).max(compute_depth(body)),
    }
}

/// Collect all variable names referenced in an expression string.
///
/// Returns `None` if the string cannot be parsed.
pub fn referenced_variables(source: &str) -> Option<Vec<String>> {
    let mut parser = Parser::new(source);
    let expr = parser.parse_expr().ok()?;
    let mut vars = Vec::new();
    collect_variables(&expr, &mut vars);
    vars.sort();
    vars.dedup();
    Some(vars)
}

/// Recursively collect all variable references in an [`Expr`] tree.
fn collect_variables(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::Number(_) | Expr::Bool(_) => {}
        Expr::Variable(name) => out.push(name.clone()),
        Expr::BinaryOp { lhs, rhs, .. } => {
            collect_variables(lhs, out);
            collect_variables(rhs, out);
        }
        Expr::UnaryOp { operand, .. } => collect_variables(operand, out),
        Expr::If { cond, then_, else_ } => {
            collect_variables(cond, out);
            collect_variables(then_, out);
            collect_variables(else_, out);
        }
        Expr::Let { name, value, body } => {
            // The bound name itself is a reference in the body context.
            out.push(name.clone());
            collect_variables(value, out);
            collect_variables(body, out);
        }
    }
}

// ---------------------------------------------------------------------------
// ExpressionStats — aggregate statistics for a set of expressions
// ---------------------------------------------------------------------------

/// Aggregate statistics computed over a batch of expressions.
///
/// Created by [`compute_stats`].
#[derive(Debug, Clone)]
pub struct ExpressionStats {
    /// Total number of expressions evaluated.
    pub total_expressions: usize,
    /// Number of expressions that evaluated successfully.
    pub successful: usize,
    /// Number of expressions that failed.
    pub failed: usize,
    /// Total number of evaluation steps across all expressions.
    pub total_steps: u64,
    /// Maximum depth reached across all expressions.
    pub max_depth: usize,
    /// Average steps per expression (0.0 if no expressions).
    pub avg_steps: f64,
}

impl fmt::Display for ExpressionStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ExpressionStats {{ total: {}, ok: {}, err: {}, steps: {}, \
             max_depth: {}, avg_steps: {:.1} }}",
            self.total_expressions,
            self.successful,
            self.failed,
            self.total_steps,
            self.max_depth,
            self.avg_steps,
        )
    }
}

/// Evaluate a batch of expressions and compute aggregate statistics.
pub fn compute_stats(sources: &[&str]) -> ExpressionStats {
    let mut ctx = EvalContext::new();
    let mut successful = 0usize;
    let mut failed = 0usize;
    let mut total_steps = 0u64;
    let mut max_depth = 0usize;

    for source in sources {
        let before_steps = ctx.step_count();
        match ctx.eval_str(source) {
            Ok(_) => successful += 1,
            Err(_) => failed += 1,
        }
        if let Some(entry) = ctx.last_result() {
            total_steps += entry.steps;
            if entry.depth_reached > max_depth {
                max_depth = entry.depth_reached;
            }
        }
        // step_count is per-eval, so we don't accumulate via it directly.
        let _ = before_steps;
    }

    let total = successful + failed;
    let avg_steps = if total > 0 {
        total_steps as f64 / total as f64
    } else {
        0.0
    };

    ExpressionStats {
        total_expressions: total,
        successful,
        failed,
        total_steps,
        max_depth,
        avg_steps,
    }
}

// ---------------------------------------------------------------------------
// EnvironmentSnapshot — save/restore context state
// ---------------------------------------------------------------------------

/// A snapshot of an [`EvalContext`]'s environment, for save/restore patterns.
///
/// Created by [`snapshot_env`] and applied by [`restore_env`].
#[derive(Debug, Clone)]
pub struct EnvironmentSnapshot {
    /// The saved environment.
    env: Env,
    /// How many history entries existed at snapshot time.
    history_len: usize,
}

/// Take a snapshot of the given context's current environment.
#[must_use]
pub fn snapshot_env(ctx: &EvalContext) -> EnvironmentSnapshot {
    EnvironmentSnapshot {
        env: ctx.env.clone(),
        history_len: ctx.history.len(),
    }
}

/// Restore a context's environment from a snapshot.
///
/// History entries added after the snapshot was taken are preserved.
pub fn restore_env(ctx: &mut EvalContext, snap: &EnvironmentSnapshot) {
    ctx.env = snap.env.clone();
}

// ---------------------------------------------------------------------------
// ExprWalker — generic visitor for expressions
// ---------------------------------------------------------------------------

/// A simple expression visitor that calls a closure on every node.
///
/// The closure receives each [`Expr`] reference and the current depth.
pub fn walk_expr<F: FnMut(&Expr, usize)>(expr: &Expr, depth: usize, visitor: &mut F) {
    visitor(expr, depth);
    match expr {
        Expr::Number(_) | Expr::Bool(_) | Expr::Variable(_) => {}
        Expr::BinaryOp { lhs, rhs, .. } => {
            walk_expr(lhs, depth + 1, visitor);
            walk_expr(rhs, depth + 1, visitor);
        }
        Expr::UnaryOp { operand, .. } => {
            walk_expr(operand, depth + 1, visitor);
        }
        Expr::If { cond, then_, else_ } => {
            walk_expr(cond, depth + 1, visitor);
            walk_expr(then_, depth + 1, visitor);
            walk_expr(else_, depth + 1, visitor);
        }
        Expr::Let { value, body, .. } => {
            walk_expr(value, depth + 1, visitor);
            walk_expr(body, depth + 1, visitor);
        }
    }
}

/// Count nodes of a specific kind in an expression tree.
///
/// `predicate` receives each node; nodes for which it returns `true` are
/// counted.
pub fn count_matching<F: Fn(&Expr) -> bool>(expr: &Expr, predicate: &F) -> usize {
    let mut count = 0usize;
    walk_expr(expr, 0, &mut |node, _depth| {
        if predicate(node) {
            count += 1;
        }
    });
    count
}

/// Count the number of numeric literal nodes in an expression tree.
pub fn count_numbers(expr: &Expr) -> usize {
    count_matching(expr, &|node| matches!(node, Expr::Number(_)))
}

/// Count the number of boolean literal nodes in an expression tree.
pub fn count_bools(expr: &Expr) -> usize {
    count_matching(expr, &|node| matches!(node, Expr::Bool(_)))
}

/// Count the number of variable reference nodes in an expression tree.
pub fn count_variables(expr: &Expr) -> usize {
    count_matching(expr, &|node| matches!(node, Expr::Variable(_)))
}

/// Count the number of binary operation nodes in an expression tree.
pub fn count_binary_ops(expr: &Expr) -> usize {
    count_matching(expr, &|node| matches!(node, Expr::BinaryOp { .. }))
}

/// Count the number of let-binding nodes in an expression tree.
pub fn count_lets(expr: &Expr) -> usize {
    count_matching(expr, &|node| matches!(node, Expr::Let { .. }))
}

/// Count the number of if-then-else nodes in an expression tree.
pub fn count_ifs(expr: &Expr) -> usize {
    count_matching(expr, &|node| matches!(node, Expr::If { .. }))
}

// ---------------------------------------------------------------------------
// Pretty-printing helpers
// ---------------------------------------------------------------------------

/// Format an expression as a tree with indentation.
///
/// Each node is on its own line, indented by its depth in the tree. This
/// produces output suitable for debugging large expression trees.
pub fn format_expr_tree(expr: &Expr) -> String {
    let mut buf = String::new();
    format_tree_inner(expr, 0, &mut buf);
    buf
}

/// Internal helper: recursively format an expression tree.
fn format_tree_inner(expr: &Expr, depth: usize, buf: &mut String) {
    let indent = "  ".repeat(depth);
    match expr {
        Expr::Number(n) => {
            if n.fract() == 0.0 && n.is_finite() {
                buf.push_str(&format!("{indent}Number({})\n", *n as i64));
            } else {
                buf.push_str(&format!("{indent}Number({n})\n"));
            }
        }
        Expr::Bool(b) => {
            buf.push_str(&format!("{indent}Bool({b})\n"));
        }
        Expr::Variable(name) => {
            buf.push_str(&format!("{indent}Variable({name})\n"));
        }
        Expr::BinaryOp { op, lhs, rhs } => {
            buf.push_str(&format!("{indent}BinaryOp({op})\n"));
            format_tree_inner(lhs, depth + 1, buf);
            format_tree_inner(rhs, depth + 1, buf);
        }
        Expr::UnaryOp { op, operand } => {
            buf.push_str(&format!("{indent}UnaryOp({op})\n"));
            format_tree_inner(operand, depth + 1, buf);
        }
        Expr::If { cond, then_, else_ } => {
            buf.push_str(&format!("{indent}If\n"));
            format_tree_inner(cond, depth + 1, buf);
            buf.push_str(&format!("{indent}Then\n"));
            format_tree_inner(then_, depth + 1, buf);
            buf.push_str(&format!("{indent}Else\n"));
            format_tree_inner(else_, depth + 1, buf);
        }
        Expr::Let { name, value, body } => {
            buf.push_str(&format!("{indent}Let({name})\n"));
            format_tree_inner(value, depth + 1, buf);
            buf.push_str(&format!("{indent}In\n"));
            format_tree_inner(body, depth + 1, buf);
        }
    }
}

/// Format an expression as a compact single-line S-expression.
///
/// This is primarily useful for snapshot testing and log output where a
/// multi-line tree would be too verbose.
pub fn format_sexpr(expr: &Expr) -> String {
    match expr {
        Expr::Number(n) => {
            if n.fract() == 0.0 && n.is_finite() {
                format!("{}", *n as i64)
            } else {
                format!("{n}")
            }
        }
        Expr::Bool(b) => format!("{b}"),
        Expr::Variable(name) => name.clone(),
        Expr::BinaryOp { op, lhs, rhs } => {
            format!("({op} {} {})", format_sexpr(lhs), format_sexpr(rhs))
        }
        Expr::UnaryOp { op, operand } => {
            format!("({op} {})", format_sexpr(operand))
        }
        Expr::If { cond, then_, else_ } => {
            format!(
                "(if {} {} {})",
                format_sexpr(cond),
                format_sexpr(then_),
                format_sexpr(else_)
            )
        }
        Expr::Let { name, value, body } => {
            format!(
                "(let ({} {}) {})",
                name,
                format_sexpr(value),
                format_sexpr(body)
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

/// Check whether an expression string is syntactically valid.
///
/// Returns `true` if the string can be parsed into an [`Expr`], `false`
/// otherwise.
pub fn is_valid_expression(source: &str) -> bool {
    let mut parser = Parser::new(source);
    parser.parse_expr().is_ok()
}

/// Validate a batch of expressions and return the indices of invalid ones.
pub fn find_invalid_expressions(sources: &[&str]) -> Vec<usize> {
    sources
        .iter()
        .enumerate()
        .filter(|(_, src)| !is_valid_expression(src))
        .map(|(i, _)| i)
        .collect()
}

/// Check whether an expression can be evaluated without errors in a clean
/// context.
///
/// This does NOT guarantee the expression will succeed in every context ---
/// it only checks that it succeeds with an empty environment.
pub fn is_evaluable(source: &str) -> bool {
    let mut ctx = EvalContext::new();
    ctx.eval_str(source).is_ok()
}

// ---------------------------------------------------------------------------
// Value comparison helpers
// ---------------------------------------------------------------------------

/// Compare two values for approximate equality.
///
/// For numbers, uses an epsilon-based comparison.  For booleans, exact match.
pub fn values_approx_equal(a: &Value, b: &Value, epsilon: f64) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => (x - y).abs() < epsilon,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        _ => false,
    }
}

/// Compare two values for exact equality.
///
/// For numbers, uses IEEE-754 bitwise equality (NaN != NaN, -0 == +0).
pub fn values_exact_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Expression builder helpers (for test convenience)
// ---------------------------------------------------------------------------

/// Create a number literal expression.
#[must_use]
pub fn num_expr(n: f64) -> Expr {
    Expr::Number(n)
}

/// Create a boolean literal expression.
#[must_use]
pub fn bool_expr(b: bool) -> Expr {
    Expr::Bool(b)
}

/// Create a variable reference expression.
#[must_use]
pub fn var_expr(name: &str) -> Expr {
    Expr::Variable(name.to_owned())
}

/// Create a binary operation expression.
#[must_use]
pub fn binary_expr(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
    Expr::BinaryOp {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    }
}

/// Create a unary operation expression.
#[must_use]
pub fn unary_expr(op: UnOp, operand: Expr) -> Expr {
    Expr::UnaryOp {
        op,
        operand: Box::new(operand),
    }
}

/// Create an if-then-else expression.
#[must_use]
pub fn if_expr(cond: Expr, then_: Expr, else_: Expr) -> Expr {
    Expr::If {
        cond: Box::new(cond),
        then_: Box::new(then_),
        else_: Box::new(else_),
    }
}

/// Create a let-binding expression.
#[must_use]
pub fn let_expr(name: &str, value: Expr, body: Expr) -> Expr {
    Expr::Let {
        name: name.to_owned(),
        value: Box::new(value),
        body: Box::new(body),
    }
}

// ---------------------------------------------------------------------------
// MultiContext — run the same expression through multiple contexts
// ---------------------------------------------------------------------------

/// Result of running a single expression through multiple contexts.
#[derive(Debug)]
pub struct MultiResult {
    /// The source expression.
    pub source: String,
    /// Results from each context, in order.
    pub results: Vec<Result<Value, CtxError>>,
}

/// Run a single expression through a list of contexts and collect results.
///
/// Useful for testing the same expression under different variable bindings
/// or limit configurations.
pub fn eval_multi(source: &str, contexts: &mut [EvalContext]) -> MultiResult {
    let mut results = Vec::with_capacity(contexts.len());
    for ctx in contexts.iter_mut() {
        results.push(ctx.eval_str(source));
    }
    MultiResult {
        source: source.to_owned(),
        results,
    }
}

// ---------------------------------------------------------------------------
// RegressionSuite — a named set of expression/expected-value pairs
// ---------------------------------------------------------------------------

/// A named test case pairing an expression with its expected result.
#[derive(Debug, Clone)]
pub struct RegressionCase {
    /// A human-readable name for the test case.
    pub name: String,
    /// The expression source.
    pub source: String,
    /// The expected value, or `None` if the expression should error.
    pub expected: Option<Value>,
}

/// A collection of regression test cases.
#[derive(Debug, Clone)]
pub struct RegressionSuite {
    /// Name of the suite.
    pub name: String,
    /// The test cases.
    pub cases: Vec<RegressionCase>,
}

impl RegressionSuite {
    /// Create a new empty suite.
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            cases: Vec::new(),
        }
    }

    /// Add a case that is expected to succeed with the given value.
    pub fn add_ok(&mut self, name: &str, source: &str, expected: Value) {
        self.cases.push(RegressionCase {
            name: name.to_owned(),
            source: source.to_owned(),
            expected: Some(expected),
        });
    }

    /// Add a case that is expected to fail.
    pub fn add_err(&mut self, name: &str, source: &str) {
        self.cases.push(RegressionCase {
            name: name.to_owned(),
            source: source.to_owned(),
            expected: None,
        });
    }

    /// Run all cases through a fresh [`EvalContext`] and return the number
    /// of failures.
    pub fn run(&self) -> Vec<RegressionFailure> {
        let mut ctx = EvalContext::new();
        let mut failures = Vec::new();

        for case in &self.cases {
            let result = ctx.eval_str(&case.source);
            match (&case.expected, &result) {
                (Some(expected), Ok(actual)) => {
                    if !values_approx_equal(expected, actual, 1e-9) {
                        failures.push(RegressionFailure {
                            case_name: case.name.clone(),
                            expected: format!("{expected}"),
                            actual: format!("{actual}"),
                        });
                    }
                }
                (Some(expected), Err(e)) => {
                    failures.push(RegressionFailure {
                        case_name: case.name.clone(),
                        expected: format!("{expected}"),
                        actual: format!("ERROR: {e}"),
                    });
                }
                (None, Ok(val)) => {
                    failures.push(RegressionFailure {
                        case_name: case.name.clone(),
                        expected: "ERROR".to_owned(),
                        actual: format!("{val}"),
                    });
                }
                (None, Err(_)) => {
                    // Expected an error, got an error. Pass.
                }
            }
        }

        failures
    }

    /// Return the total number of cases.
    #[must_use]
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// Return whether the suite is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }
}

/// A single regression failure with details.
#[derive(Debug, Clone)]
pub struct RegressionFailure {
    /// Name of the failing case.
    pub case_name: String,
    /// What was expected.
    pub expected: String,
    /// What was actually produced.
    pub actual: String,
}

impl fmt::Display for RegressionFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "FAIL {}: expected {}, got {}",
            self.case_name, self.expected, self.actual
        )
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::Value;

    // -----------------------------------------------------------------------
    // EvalContext::eval_str — arithmetic
    // -----------------------------------------------------------------------

    #[test]
    fn ctx_eval_addition() {
        let mut ctx = EvalContext::new();
        let result = ctx.eval_str("3 + 4").unwrap();
        assert_eq!(result, Value::Number(7.0));
    }

    #[test]
    fn ctx_eval_subtraction() {
        let mut ctx = EvalContext::new();
        let result = ctx.eval_str("10 - 3").unwrap();
        assert_eq!(result, Value::Number(7.0));
    }

    #[test]
    fn ctx_eval_multiplication() {
        let mut ctx = EvalContext::new();
        let result = ctx.eval_str("6 * 7").unwrap();
        assert_eq!(result, Value::Number(42.0));
    }

    #[test]
    fn ctx_eval_division() {
        let mut ctx = EvalContext::new();
        let result = ctx.eval_str("20 / 4").unwrap();
        assert_eq!(result, Value::Number(5.0));
    }

    #[test]
    fn ctx_eval_modulo() {
        let mut ctx = EvalContext::new();
        let result = ctx.eval_str("17 % 5").unwrap();
        assert_eq!(result, Value::Number(2.0));
    }

    #[test]
    fn ctx_eval_power() {
        let mut ctx = EvalContext::new();
        let result = ctx.eval_str("2 ** 10").unwrap();
        assert_eq!(result, Value::Number(1024.0));
    }

    #[test]
    fn ctx_eval_complex_arithmetic() {
        let mut ctx = EvalContext::new();
        let result = ctx.eval_str("(3 + 4) * 2 - 1").unwrap();
        assert_eq!(result, Value::Number(13.0));
    }

    // -----------------------------------------------------------------------
    // EvalContext::eval_str — let bindings
    // -----------------------------------------------------------------------

    #[test]
    fn ctx_eval_let_simple() {
        let mut ctx = EvalContext::new();
        let result = ctx.eval_str("let x = 10 in x + 5").unwrap();
        assert_eq!(result, Value::Number(15.0));
    }

    #[test]
    fn ctx_eval_let_nested() {
        let mut ctx = EvalContext::new();
        let result = ctx.eval_str("let x = 2 in let y = x * 3 in x + y").unwrap();
        assert_eq!(result, Value::Number(8.0));
    }

    #[test]
    fn ctx_eval_let_shadowing() {
        let mut ctx = EvalContext::new();
        let result = ctx.eval_str("let x = 1 in let x = 99 in x").unwrap();
        assert_eq!(result, Value::Number(99.0));
    }

    // -----------------------------------------------------------------------
    // EvalContext::eval_str — if/then/else
    // -----------------------------------------------------------------------

    #[test]
    fn ctx_eval_if_true() {
        let mut ctx = EvalContext::new();
        let result = ctx.eval_str("if true then 42 else 0").unwrap();
        assert_eq!(result, Value::Number(42.0));
    }

    #[test]
    fn ctx_eval_if_false() {
        let mut ctx = EvalContext::new();
        let result = ctx.eval_str("if false then 42 else 0").unwrap();
        assert_eq!(result, Value::Number(0.0));
    }

    #[test]
    fn ctx_eval_if_computed_condition() {
        let mut ctx = EvalContext::new();
        let result = ctx.eval_str("if 3 < 5 then 1 else 2").unwrap();
        assert_eq!(result, Value::Number(1.0));
    }

    // -----------------------------------------------------------------------
    // EvalContext::define — inject variables
    // -----------------------------------------------------------------------

    #[test]
    fn ctx_define_then_reference() {
        let mut ctx = EvalContext::new();
        ctx.define("x", Value::Number(42.0));
        let result = ctx.eval_str("x").unwrap();
        assert_eq!(result, Value::Number(42.0));
    }

    #[test]
    fn ctx_define_then_use_in_expression() {
        let mut ctx = EvalContext::new();
        ctx.define("radius", Value::Number(5.0));
        let result = ctx.eval_str("radius * radius").unwrap();
        assert_eq!(result, Value::Number(25.0));
    }

    #[test]
    fn ctx_define_overwrite() {
        let mut ctx = EvalContext::new();
        ctx.define("x", Value::Number(1.0));
        ctx.define("x", Value::Number(2.0));
        let result = ctx.eval_str("x").unwrap();
        assert_eq!(result, Value::Number(2.0));
    }

    #[test]
    fn ctx_define_multiple_variables() {
        let mut ctx = EvalContext::new();
        ctx.define("a", Value::Number(10.0));
        ctx.define("b", Value::Number(20.0));
        ctx.define("c", Value::Number(30.0));
        let result = ctx.eval_str("a + b + c").unwrap();
        assert_eq!(result, Value::Number(60.0));
    }

    // -----------------------------------------------------------------------
    // Error: DepthExceeded
    // -----------------------------------------------------------------------

    #[test]
    fn ctx_depth_exceeded_tiny_limit() {
        // With max_call_depth=1, "1 + 2" fails because the BinaryOp at
        // depth 1 tries to recurse into Number children at depth 2, which
        // exceeds the limit.
        let mut ctx = EvalContext::with_limits(1, 100_000);
        let err = ctx.eval_str("1 + 2").unwrap_err();
        match err {
            CtxError::DepthExceeded { max } => assert_eq!(max, 1),
            other => panic!("expected DepthExceeded, got: {other}"),
        }
    }

    #[test]
    fn ctx_depth_exceeded_recorded_in_history() {
        let mut ctx = EvalContext::with_limits(1, 100_000);
        let _ = ctx.eval_str("1 + 2");
        let entry = ctx.last_result().expect("should have history");
        assert!(entry.result.is_err());
    }

    // -----------------------------------------------------------------------
    // Error: StepLimitExceeded
    // -----------------------------------------------------------------------

    #[test]
    fn ctx_step_limit_exceeded() {
        // With max_steps=3, evaluating "1 + 2 + 3" should exceed the limit
        // because it needs at least 5 steps (BinOp, BinOp, Num, Num, Num).
        let mut ctx = EvalContext::with_limits(64, 3);
        let err = ctx.eval_str("1 + 2 + 3").unwrap_err();
        match err {
            CtxError::StepLimitExceeded { max } => assert_eq!(max, 3),
            other => panic!("expected StepLimitExceeded, got: {other}"),
        }
    }

    #[test]
    fn ctx_step_limit_barely_passes() {
        // "42" needs exactly 1 step (a Number literal).
        let mut ctx = EvalContext::with_limits(64, 1);
        let result = ctx.eval_str("42").unwrap();
        assert_eq!(result, Value::Number(42.0));
    }

    // -----------------------------------------------------------------------
    // History tracking
    // -----------------------------------------------------------------------

    #[test]
    fn ctx_history_records_successes() {
        let mut ctx = EvalContext::new();
        ctx.eval_str("1 + 1").unwrap();
        ctx.eval_str("2 * 3").unwrap();
        assert_eq!(ctx.history().len(), 2);
        assert!(ctx.history()[0].result.is_ok());
        assert!(ctx.history()[1].result.is_ok());
    }

    #[test]
    fn ctx_history_records_failures() {
        let mut ctx = EvalContext::new();
        let _ = ctx.eval_str("undefined_var");
        assert_eq!(ctx.history().len(), 1);
        assert!(ctx.history()[0].result.is_err());
    }

    #[test]
    fn ctx_history_source_preserved() {
        let mut ctx = EvalContext::new();
        ctx.eval_str("42").unwrap();
        assert_eq!(ctx.history()[0].source, "42");
    }

    #[test]
    fn ctx_history_steps_recorded() {
        let mut ctx = EvalContext::new();
        ctx.eval_str("1 + 2").unwrap();
        let entry = ctx.last_result().unwrap();
        // "1 + 2" has 3 nodes: BinaryOp, Number(1), Number(2), so 3 steps.
        assert!(entry.steps > 0, "steps should be positive");
    }

    #[test]
    fn ctx_history_depth_recorded() {
        let mut ctx = EvalContext::new();
        ctx.eval_str("1 + 2").unwrap();
        let entry = ctx.last_result().unwrap();
        assert!(entry.depth_reached > 0, "depth_reached should be positive");
    }

    #[test]
    fn ctx_last_result_is_most_recent() {
        let mut ctx = EvalContext::new();
        ctx.eval_str("1").unwrap();
        ctx.eval_str("2").unwrap();
        let last = ctx.last_result().unwrap();
        assert_eq!(last.source, "2");
    }

    #[test]
    fn ctx_reset_history_clears() {
        let mut ctx = EvalContext::new();
        ctx.eval_str("1").unwrap();
        ctx.eval_str("2").unwrap();
        ctx.reset_history();
        assert!(ctx.history().is_empty());
        assert!(ctx.last_result().is_none());
    }

    // -----------------------------------------------------------------------
    // Tracer
    // -----------------------------------------------------------------------

    #[test]
    fn tracer_records_when_enabled() {
        let mut tracer = Tracer::new(true);
        tracer.enter("Number", 0);
        tracer.exit("Number", 0, "42");
        assert_eq!(tracer.events().len(), 2);
    }

    #[test]
    fn tracer_silent_when_disabled() {
        let mut tracer = Tracer::new(false);
        tracer.enter("Number", 0);
        tracer.exit("Number", 0, "42");
        assert!(tracer.events().is_empty());
    }

    #[test]
    fn tracer_clear_removes_events() {
        let mut tracer = Tracer::new(true);
        tracer.enter("Number", 0);
        tracer.clear();
        assert!(tracer.events().is_empty());
    }

    #[test]
    fn tracer_error_records() {
        let mut tracer = Tracer::new(true);
        tracer.error("something went wrong", 1);
        assert_eq!(tracer.events().len(), 1);
        match &tracer.events()[0] {
            TraceEvent::Error { message, depth } => {
                assert!(message.contains("something went wrong"));
                assert_eq!(*depth, 1);
            }
            _ => panic!("expected Error event"),
        }
    }

    #[test]
    fn tracer_format_trace_non_empty_for_nontrivial() {
        let mut tracer = Tracer::new(true);
        tracer.enter("BinaryOp(+)", 0);
        tracer.enter("Number", 1);
        tracer.exit("Number", 1, "3");
        tracer.enter("Number", 1);
        tracer.exit("Number", 1, "4");
        tracer.exit("BinaryOp(+)", 0, "7");
        let text = tracer.format_trace();
        assert!(!text.is_empty());
        assert!(text.contains("ENTER"));
        assert!(text.contains("EXIT"));
        assert!(text.contains("BinaryOp(+)"));
    }

    #[test]
    fn tracer_format_trace_has_indentation() {
        let mut tracer = Tracer::new(true);
        tracer.enter("BinaryOp(+)", 0);
        tracer.enter("Number", 1);
        tracer.exit("Number", 1, "1");
        tracer.exit("BinaryOp(+)", 0, "1");
        let text = tracer.format_trace();
        // Depth-1 events should have 2 spaces of indentation.
        assert!(text.contains("  ENTER Number"));
    }

    #[test]
    fn tracer_summary() {
        let mut tracer = Tracer::new(true);
        tracer.enter("X", 0);
        tracer.enter("Y", 3);
        tracer.exit("Y", 3, "v");
        tracer.exit("X", 0, "v");
        let s = tracer.summary();
        assert!(s.contains("4 events"));
        assert!(s.contains("max depth 3"));
    }

    // -----------------------------------------------------------------------
    // TracingContext
    // -----------------------------------------------------------------------

    #[test]
    fn tracing_ctx_round_trip_success() {
        let mut tc = TracingContext::new();
        let (result, trace) = tc.eval_traced("3 + 4");
        assert_eq!(result.unwrap(), Value::Number(7.0));
        assert!(!trace.is_empty(), "trace should be non-empty");
    }

    #[test]
    fn tracing_ctx_round_trip_error() {
        let mut tc = TracingContext::new();
        let (result, trace) = tc.eval_traced("undefined");
        assert!(result.is_err());
        assert!(!trace.is_empty());
    }

    #[test]
    fn tracing_ctx_define() {
        let mut tc = TracingContext::new();
        tc.define("x", Value::Number(100.0));
        let (result, _) = tc.eval_traced("x + 1");
        assert_eq!(result.unwrap(), Value::Number(101.0));
    }

    #[test]
    fn tracing_ctx_history_populated() {
        let mut tc = TracingContext::new();
        tc.eval_traced("1 + 1");
        tc.eval_traced("2 * 3");
        assert_eq!(tc.history().len(), 2);
    }

    #[test]
    fn tracing_ctx_trace_events_populated() {
        let mut tc = TracingContext::new();
        tc.eval_traced("1 + 2");
        assert!(
            !tc.trace_events().is_empty(),
            "trace events should be populated"
        );
    }

    // -----------------------------------------------------------------------
    // Interpreter
    // -----------------------------------------------------------------------

    #[test]
    fn interpreter_eval_simple() {
        let mut interp = Interpreter::new();
        assert_eq!(interp.eval("2 + 3"), "5");
    }

    #[test]
    fn interpreter_eval_float() {
        let mut interp = Interpreter::new();
        assert_eq!(interp.eval("1 / 3"), "0.3333333333333333");
    }

    #[test]
    fn interpreter_eval_error() {
        let mut interp = Interpreter::new();
        let output = interp.eval("undefined_var");
        assert!(output.starts_with("ERROR:"), "got: {output}");
    }

    #[test]
    fn interpreter_prelude_pi() {
        let mut interp = Interpreter::new();
        let output = interp.eval("pi");
        assert!(
            output.contains("3.14159"),
            "pi should be available, got: {output}"
        );
    }

    #[test]
    fn interpreter_prelude_e() {
        let mut interp = Interpreter::new();
        let output = interp.eval("e");
        assert!(
            output.contains("2.71828"),
            "e should be available, got: {output}"
        );
    }

    #[test]
    fn interpreter_prelude_tau() {
        let mut interp = Interpreter::new();
        let output = interp.eval("tau");
        assert!(
            output.contains("6.28318"),
            "tau should be available, got: {output}"
        );
    }

    #[test]
    fn interpreter_history_summary_line_count() {
        let mut interp = Interpreter::new();
        interp.eval("1 + 1");
        interp.eval("2 * 3");
        interp.eval("10 / 2");
        let summary = interp.history_summary();
        let lines: Vec<&str> = summary.lines().collect();
        assert_eq!(lines.len(), 3, "should have 3 history lines");
    }

    #[test]
    fn interpreter_history_summary_empty_when_no_evals() {
        let interp = Interpreter::new();
        assert!(interp.history_summary().is_empty());
    }

    #[test]
    fn interpreter_with_prompt() {
        let interp = Interpreter::with_prompt(">> ");
        assert_eq!(interp.prompt(), ">> ");
    }

    #[test]
    fn interpreter_load_prelude() {
        let mut interp = Interpreter::new();
        interp.load_prelude(&[("x", "42"), ("y", "x + 1")]);
        // "x" should be 42 (the literal was evaluated and stored).
        let output = interp.eval("x");
        assert_eq!(output, "42");
    }

    #[test]
    fn interpreter_define_custom() {
        let mut interp = Interpreter::new();
        interp.define("answer", Value::Number(42.0));
        assert_eq!(interp.eval("answer"), "42");
    }

    // -----------------------------------------------------------------------
    // CtxError Display
    // -----------------------------------------------------------------------

    #[test]
    fn ctx_error_display_parse() {
        let e = CtxError::Parse("unexpected token".to_owned());
        let msg = e.to_string();
        assert!(msg.contains("parse error"), "got: {msg}");
    }

    #[test]
    fn ctx_error_display_depth() {
        let e = CtxError::DepthExceeded { max: 64 };
        let msg = e.to_string();
        assert!(msg.contains("64"), "got: {msg}");
    }

    #[test]
    fn ctx_error_display_steps() {
        let e = CtxError::StepLimitExceeded { max: 100_000 };
        let msg = e.to_string();
        assert!(msg.contains("100000"), "got: {msg}");
    }

    // -----------------------------------------------------------------------
    // HistoryEntry formatting
    // -----------------------------------------------------------------------

    #[test]
    fn history_entry_format_line_ok() {
        let entry = HistoryEntry {
            source: "1 + 2".to_owned(),
            result: Ok(Value::Number(3.0)),
            steps: 3,
            depth_reached: 2,
        };
        let line = entry.format_line();
        assert!(line.contains("1 + 2"));
        assert!(line.contains("3"));
        assert!(line.contains("3 steps"));
    }

    #[test]
    fn history_entry_format_line_err() {
        let entry = HistoryEntry {
            source: "bad".to_owned(),
            result: Err("unbound variable".to_owned()),
            steps: 1,
            depth_reached: 1,
        };
        let line = entry.format_line();
        assert!(line.contains("ERROR"));
        assert!(line.contains("unbound variable"));
    }

    // -----------------------------------------------------------------------
    // batch_eval
    // -----------------------------------------------------------------------

    #[test]
    fn batch_eval_processes_multiple() {
        let results = batch_eval(&["1 + 1", "2 * 3", "bad_var"]);
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].1, "2");
        assert_eq!(results[1].1, "6");
        assert!(results[2].1.starts_with("ERROR:"));
    }

    #[test]
    fn batch_eval_ok_filters_errors() {
        let values = batch_eval_ok(&["1 + 1", "bad_var", "3 * 3"]);
        assert_eq!(values.len(), 2);
        assert_eq!(values[0], Value::Number(2.0));
        assert_eq!(values[1], Value::Number(9.0));
    }

    // -----------------------------------------------------------------------
    // Expression analysis
    // -----------------------------------------------------------------------

    #[test]
    fn count_nodes_simple() {
        // "1 + 2" = BinOp, Num, Num = 3 nodes
        assert_eq!(count_nodes("1 + 2"), Some(3));
    }

    #[test]
    fn count_nodes_invalid() {
        assert_eq!(count_nodes("@@@"), None);
    }

    #[test]
    fn expr_depth_simple() {
        // "1 + 2" has depth 2 (BinOp at top, leaves below).
        assert_eq!(expr_depth("1 + 2"), Some(2));
    }

    #[test]
    fn referenced_variables_finds_all() {
        let vars = referenced_variables("x + y * z").unwrap();
        assert_eq!(vars, vec!["x", "y", "z"]);
    }

    // -----------------------------------------------------------------------
    // Validation helpers
    // -----------------------------------------------------------------------

    #[test]
    fn is_valid_expression_true() {
        assert!(is_valid_expression("1 + 2"));
    }

    #[test]
    fn is_valid_expression_false() {
        assert!(!is_valid_expression("@@@"));
    }

    #[test]
    fn find_invalid_expressions_works() {
        let invalid = find_invalid_expressions(&["1 + 2", "@@@", "3", "!!!"]);
        assert_eq!(invalid, vec![1, 3]);
    }

    #[test]
    fn is_evaluable_true() {
        assert!(is_evaluable("1 + 2"));
    }

    #[test]
    fn is_evaluable_false_unbound() {
        assert!(!is_evaluable("x + 1"));
    }

    // -----------------------------------------------------------------------
    // Value comparison
    // -----------------------------------------------------------------------

    #[test]
    fn values_approx_equal_numbers() {
        let a = Value::Number(1.0);
        let b = Value::Number(1.0 + 1e-12);
        assert!(values_approx_equal(&a, &b, 1e-9));
    }

    #[test]
    fn values_approx_not_equal_different_types() {
        let a = Value::Number(1.0);
        let b = Value::Bool(true);
        assert!(!values_approx_equal(&a, &b, 1e-9));
    }

    // -----------------------------------------------------------------------
    // Expression builders
    // -----------------------------------------------------------------------

    #[test]
    fn builders_compose_correctly() {
        let expr = binary_expr(BinOp::Add, num_expr(1.0), num_expr(2.0));
        let mut ctx = EvalContext::new();
        let result = ctx.eval_expr(&expr).unwrap();
        assert_eq!(result, Value::Number(3.0));
    }

    #[test]
    fn builder_let_expr_works() {
        let expr = let_expr(
            "x",
            num_expr(10.0),
            binary_expr(BinOp::Add, var_expr("x"), num_expr(5.0)),
        );
        let mut ctx = EvalContext::new();
        let result = ctx.eval_expr(&expr).unwrap();
        assert_eq!(result, Value::Number(15.0));
    }

    #[test]
    fn builder_if_expr_works() {
        let expr = if_expr(bool_expr(true), num_expr(1.0), num_expr(2.0));
        let mut ctx = EvalContext::new();
        let result = ctx.eval_expr(&expr).unwrap();
        assert_eq!(result, Value::Number(1.0));
    }

    // -----------------------------------------------------------------------
    // Format helpers
    // -----------------------------------------------------------------------

    #[test]
    fn format_expr_tree_produces_output() {
        let mut parser = Parser::new("1 + 2");
        let expr = parser.parse_expr().unwrap();
        let tree = format_expr_tree(&expr);
        assert!(tree.contains("BinaryOp"));
        assert!(tree.contains("Number"));
    }

    #[test]
    fn format_sexpr_simple() {
        let mut parser = Parser::new("1 + 2");
        let expr = parser.parse_expr().unwrap();
        let s = format_sexpr(&expr);
        assert_eq!(s, "(+ 1 2)");
    }

    #[test]
    fn format_sexpr_nested() {
        let mut parser = Parser::new("(1 + 2) * 3");
        let expr = parser.parse_expr().unwrap();
        let s = format_sexpr(&expr);
        assert_eq!(s, "(* (+ 1 2) 3)");
    }

    // -----------------------------------------------------------------------
    // Node counting helpers
    // -----------------------------------------------------------------------

    #[test]
    fn count_numbers_works() {
        let mut parser = Parser::new("1 + 2 + 3");
        let expr = parser.parse_expr().unwrap();
        assert_eq!(count_numbers(&expr), 3);
    }

    #[test]
    fn count_binary_ops_works() {
        let mut parser = Parser::new("1 + 2 * 3");
        let expr = parser.parse_expr().unwrap();
        assert_eq!(count_binary_ops(&expr), 2);
    }

    #[test]
    fn count_variables_works() {
        let mut parser = Parser::new("x + y + z");
        let expr = parser.parse_expr().unwrap();
        assert_eq!(count_variables(&expr), 3);
    }

    #[test]
    fn count_lets_works() {
        let mut parser = Parser::new("let x = 1 in let y = 2 in x + y");
        let expr = parser.parse_expr().unwrap();
        assert_eq!(count_lets(&expr), 2);
    }

    // -----------------------------------------------------------------------
    // Snapshot / restore
    // -----------------------------------------------------------------------

    #[test]
    fn snapshot_restore_preserves_env() {
        let mut ctx = EvalContext::new();
        ctx.define("x", Value::Number(1.0));

        let snap = snapshot_env(&ctx);

        ctx.define("x", Value::Number(99.0));
        assert_eq!(ctx.eval_str("x").unwrap(), Value::Number(99.0));

        restore_env(&mut ctx, &snap);
        assert_eq!(ctx.eval_str("x").unwrap(), Value::Number(1.0));
    }

    // -----------------------------------------------------------------------
    // RegressionSuite
    // -----------------------------------------------------------------------

    #[test]
    fn regression_suite_all_pass() {
        let mut suite = RegressionSuite::new("basic");
        suite.add_ok("add", "1 + 2", Value::Number(3.0));
        suite.add_ok("mul", "3 * 4", Value::Number(12.0));
        suite.add_err("unbound", "undefined_var");
        let failures = suite.run();
        assert!(failures.is_empty(), "all should pass, got: {:?}", failures);
    }

    #[test]
    fn regression_suite_detects_failure() {
        let mut suite = RegressionSuite::new("failing");
        suite.add_ok("wrong", "1 + 2", Value::Number(999.0));
        let failures = suite.run();
        assert_eq!(failures.len(), 1);
        assert!(failures[0].case_name == "wrong");
    }

    // -----------------------------------------------------------------------
    // ExpressionStats
    // -----------------------------------------------------------------------

    #[test]
    fn compute_stats_basic() {
        let stats = compute_stats(&["1 + 2", "3 * 4", "bad_var"]);
        assert_eq!(stats.total_expressions, 3);
        assert_eq!(stats.successful, 2);
        assert_eq!(stats.failed, 1);
        assert!(stats.total_steps > 0);
        assert!(stats.max_depth > 0);
    }

    #[test]
    fn compute_stats_display() {
        let stats = compute_stats(&["42"]);
        let s = stats.to_string();
        assert!(s.contains("ExpressionStats"));
        assert!(s.contains("total: 1"));
    }

    // -----------------------------------------------------------------------
    // walk_expr
    // -----------------------------------------------------------------------

    #[test]
    fn walk_expr_visits_all_nodes() {
        let mut parser = Parser::new("1 + 2");
        let expr = parser.parse_expr().unwrap();
        let mut count = 0usize;
        walk_expr(&expr, 0, &mut |_node, _depth| {
            count += 1;
        });
        assert_eq!(count, 3); // BinOp, Num, Num
    }

    // -----------------------------------------------------------------------
    // eval_multi
    // -----------------------------------------------------------------------

    #[test]
    fn eval_multi_runs_across_contexts() {
        let mut ctx1 = EvalContext::new();
        ctx1.define("x", Value::Number(10.0));
        let mut ctx2 = EvalContext::new();
        ctx2.define("x", Value::Number(20.0));

        let mr = eval_multi("x", &mut [ctx1, ctx2]);
        assert_eq!(mr.results.len(), 2);
        assert_eq!(mr.results[0].as_ref().unwrap(), &Value::Number(10.0));
        assert_eq!(mr.results[1].as_ref().unwrap(), &Value::Number(20.0));
    }

    // -----------------------------------------------------------------------
    // Default impls
    // -----------------------------------------------------------------------

    #[test]
    fn eval_context_default() {
        let ctx = EvalContext::default();
        assert_eq!(ctx.max_call_depth(), 64);
        assert_eq!(ctx.max_steps(), 100_000);
    }

    #[test]
    fn tracing_context_default() {
        let tc = TracingContext::default();
        assert!(tc.history().is_empty());
    }

    #[test]
    fn interpreter_default() {
        let interp = Interpreter::default();
        assert_eq!(interp.prompt(), "> ");
    }

    #[test]
    fn tracer_default_is_disabled() {
        let tracer = Tracer::default();
        assert!(!tracer.is_enabled());
    }
}
