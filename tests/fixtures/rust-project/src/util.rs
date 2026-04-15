//! Expression evaluator for the `rust-project` fixture.
//!
//! The evaluator walks an [`Expr`] tree and computes a [`Value`].  Variables
//! are resolved through an [`Env`] (a simple stack of name–value bindings).
//!
//! # Design notes
//!
//! - Evaluation is purely recursive with no heap-allocated call-stack tricks.
//!   For the sizes of expressions this fixture handles that is fine.
//! - Errors are surfaced through [`EvalError`], which implements the standard
//!   library's [`std::error::Error`] trait without depending on `thiserror`.

use std::collections::HashMap;
use std::fmt;

use crate::parser::{BinOp, Expr, UnOp};

// ---------------------------------------------------------------------------
// Value
// ---------------------------------------------------------------------------

/// A runtime value produced by evaluating an expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A IEEE-754 double-precision floating-point number.
    Number(f64),
    /// A boolean.
    Bool(bool),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Number(n) => {
                if n.fract() == 0.0 && n.is_finite() {
                    write!(f, "{}", *n as i64)
                } else {
                    write!(f, "{n}")
                }
            }
            Value::Bool(b) => write!(f, "{b}"),
        }
    }
}

// ---------------------------------------------------------------------------
// EvalError
// ---------------------------------------------------------------------------

/// Errors that can occur during expression evaluation.
#[derive(Debug, Clone)]
pub enum EvalError {
    /// A variable was referenced but not found in the current environment.
    UnboundVariable(String),
    /// Division or modulo by zero.
    DivisionByZero,
    /// A binary operator received operands of incompatible types.
    TypeMismatch {
        op: &'static str,
        expected: &'static str,
        got: &'static str,
    },
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EvalError::UnboundVariable(name) => write!(f, "unbound variable: `{name}`"),
            EvalError::DivisionByZero => write!(f, "division or modulo by zero"),
            EvalError::TypeMismatch { op, expected, got } => {
                write!(
                    f,
                    "type mismatch for `{op}`: expected {expected}, got {got}"
                )
            }
        }
    }
}

impl std::error::Error for EvalError {}

// ---------------------------------------------------------------------------
// Environment
// ---------------------------------------------------------------------------

/// A lexical environment mapping variable names to runtime values.
///
/// Implemented as an immutable chain: the inner `HashMap` holds the *current
/// frame's* bindings; lookups fall through to `parent` when a name is not
/// found.  Cloning is cheap because inner maps are small.
#[derive(Debug, Default, Clone)]
pub struct Env {
    bindings: HashMap<String, Value>,
}

impl Env {
    /// Create a new, empty environment.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Return a new environment that extends `self` with the given binding.
    #[must_use]
    pub fn extend(&self, name: String, val: Value) -> Self {
        let mut inner = self.bindings.clone();
        inner.insert(name, val);
        Env { bindings: inner }
    }

    /// Look up `name`.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.bindings.get(name)
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Evaluate `expr` in an empty environment.
///
/// # Errors
///
/// Returns [`EvalError`] if the expression contains unbound variables, type
/// mismatches, or division-by-zero.
pub fn evaluate(expr: &Expr) -> Result<Value, EvalError> {
    eval(expr, &Env::new())
}

/// Evaluate `expr` in the provided `env`.
///
/// # Errors
///
/// Same as [`evaluate`].
pub fn eval(expr: &Expr, env: &Env) -> Result<Value, EvalError> {
    match expr {
        // ── literals ─────────────────────────────────────────────────────
        Expr::Number(n) => Ok(Value::Number(*n)),
        Expr::Bool(b) => Ok(Value::Bool(*b)),

        // ── variable lookup ───────────────────────────────────────────────
        Expr::Variable(name) => env
            .get(name)
            .cloned()
            .ok_or_else(|| EvalError::UnboundVariable(name.clone())),

        // ── binary operations ────────────────────────────────────────────
        Expr::BinaryOp { op, lhs, rhs } => {
            // Short-circuit evaluation for And / Or before evaluating `rhs`.
            match op {
                BinOp::And => {
                    let lval = eval(lhs, env)?;
                    match lval {
                        Value::Bool(false) => return Ok(Value::Bool(false)),
                        Value::Bool(true) => {
                            let rval = eval(rhs, env)?;
                            return coerce_bool(rval, "&&");
                        }
                        _ => {
                            return Err(EvalError::TypeMismatch {
                                op: "&&",
                                expected: "bool",
                                got: value_type_name(&lval),
                            });
                        }
                    }
                }
                BinOp::Or => {
                    let lval = eval(lhs, env)?;
                    match lval {
                        Value::Bool(true) => return Ok(Value::Bool(true)),
                        Value::Bool(false) => {
                            let rval = eval(rhs, env)?;
                            return coerce_bool(rval, "||");
                        }
                        _ => {
                            return Err(EvalError::TypeMismatch {
                                op: "||",
                                expected: "bool",
                                got: value_type_name(&lval),
                            });
                        }
                    }
                }
                _ => {}
            }

            let lval = eval(lhs, env)?;
            let rval = eval(rhs, env)?;
            apply_binop(*op, lval, rval)
        }

        // ── unary operations ─────────────────────────────────────────────
        Expr::UnaryOp { op, operand } => {
            let val = eval(operand, env)?;
            apply_unop(*op, val)
        }

        // ── if / then / else ─────────────────────────────────────────────
        Expr::If { cond, then_, else_ } => {
            let cond_val = eval(cond, env)?;
            match cond_val {
                Value::Bool(true) => eval(then_, env),
                Value::Bool(false) => eval(else_, env),
                other => Err(EvalError::TypeMismatch {
                    op: "if",
                    expected: "bool",
                    got: value_type_name(&other),
                }),
            }
        }

        // ── let bindings ─────────────────────────────────────────────────
        Expr::Let { name, value, body } => {
            let val = eval(value, env)?;
            let new_env = env.extend(name.clone(), val);
            eval(body, &new_env)
        }
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn coerce_bool(val: Value, op: &'static str) -> Result<Value, EvalError> {
    match val {
        Value::Bool(b) => Ok(Value::Bool(b)),
        other => Err(EvalError::TypeMismatch {
            op,
            expected: "bool",
            got: value_type_name(&other),
        }),
    }
}

fn value_type_name(val: &Value) -> &'static str {
    match val {
        Value::Number(_) => "number",
        Value::Bool(_) => "bool",
    }
}

fn apply_binop(op: BinOp, lval: Value, rval: Value) -> Result<Value, EvalError> {
    match (op, lval, rval) {
        // ── arithmetic ────────────────────────────────────────────────────
        (BinOp::Add, Value::Number(a), Value::Number(b)) => Ok(Value::Number(a + b)),
        (BinOp::Sub, Value::Number(a), Value::Number(b)) => Ok(Value::Number(a - b)),
        (BinOp::Mul, Value::Number(a), Value::Number(b)) => Ok(Value::Number(a * b)),
        (BinOp::Div, Value::Number(a), Value::Number(b)) => {
            if b == 0.0 {
                Err(EvalError::DivisionByZero)
            } else {
                Ok(Value::Number(a / b))
            }
        }
        (BinOp::Mod, Value::Number(a), Value::Number(b)) => {
            if b == 0.0 {
                Err(EvalError::DivisionByZero)
            } else {
                Ok(Value::Number(a % b))
            }
        }
        (BinOp::Pow, Value::Number(a), Value::Number(b)) => Ok(Value::Number(a.powf(b))),

        // ── comparisons ───────────────────────────────────────────────────
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

        // ── type errors ───────────────────────────────────────────────────
        (op, lval, _) => Err(EvalError::TypeMismatch {
            op: op_name(op),
            expected: "number",
            got: value_type_name(&lval),
        }),
    }
}

fn apply_unop(op: UnOp, val: Value) -> Result<Value, EvalError> {
    match (op, val) {
        (UnOp::Neg, Value::Number(n)) => Ok(Value::Number(-n)),
        (UnOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
        (UnOp::Neg, other) => Err(EvalError::TypeMismatch {
            op: "-",
            expected: "number",
            got: value_type_name(&other),
        }),
        (UnOp::Not, other) => Err(EvalError::TypeMismatch {
            op: "!",
            expected: "bool",
            got: value_type_name(&other),
        }),
    }
}

fn op_name(op: BinOp) -> &'static str {
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::parser::Parser;

    use super::{eval, evaluate, Env, Value};

    fn run(s: &str) -> Value {
        let expr = Parser::new(s).parse_expr().expect("parse");
        evaluate(&expr).expect("eval")
    }

    fn run_err(s: &str) -> String {
        let expr = Parser::new(s).parse_expr().expect("parse");
        evaluate(&expr).expect_err("expected error").to_string()
    }

    #[test]
    fn eval_number_literal() {
        assert_eq!(run("42"), Value::Number(42.0));
    }

    #[test]
    fn eval_arithmetic() {
        assert_eq!(run("3 + 4 * 2"), Value::Number(11.0));
    }

    #[test]
    fn eval_parentheses() {
        assert_eq!(run("(3 + 4) * 2"), Value::Number(14.0));
    }

    #[test]
    fn eval_boolean_literal() {
        assert_eq!(run("true"), Value::Bool(true));
        assert_eq!(run("false"), Value::Bool(false));
    }

    #[test]
    fn eval_comparison() {
        assert_eq!(run("1 < 2"), Value::Bool(true));
        assert_eq!(run("5 >= 5"), Value::Bool(true));
        assert_eq!(run("3 != 3"), Value::Bool(false));
    }

    #[test]
    fn eval_logical() {
        assert_eq!(run("true && false"), Value::Bool(false));
        assert_eq!(run("false || true"), Value::Bool(true));
    }

    #[test]
    fn eval_unary_neg() {
        assert_eq!(run("-7"), Value::Number(-7.0));
    }

    #[test]
    fn eval_unary_not() {
        assert_eq!(run("!false"), Value::Bool(true));
    }

    #[test]
    fn eval_if_true() {
        assert_eq!(run("if true then 1 else 2"), Value::Number(1.0));
    }

    #[test]
    fn eval_if_false() {
        assert_eq!(run("if false then 1 else 2"), Value::Number(2.0));
    }

    #[test]
    fn eval_let_binding() {
        assert_eq!(run("let x = 10 in x + 5"), Value::Number(15.0));
    }

    #[test]
    fn eval_nested_let() {
        assert_eq!(
            run("let x = 2 in let y = x * 3 in x + y"),
            Value::Number(8.0)
        );
    }

    #[test]
    fn eval_let_shadows_outer() {
        let expr = crate::parser::Parser::new("let x = 1 in let x = 99 in x")
            .parse_expr()
            .unwrap();
        assert_eq!(evaluate(&expr).unwrap(), Value::Number(99.0));
    }

    #[test]
    fn eval_variable_from_env() {
        let expr = crate::parser::Parser::new("x + 1").parse_expr().unwrap();
        let env = Env::new().extend("x".to_owned(), Value::Number(41.0));
        assert_eq!(eval(&expr, &env).unwrap(), Value::Number(42.0));
    }

    #[test]
    fn eval_error_unbound() {
        let msg = run_err("undefined_var");
        assert!(msg.contains("unbound"), "got: {msg}");
    }

    #[test]
    fn eval_error_div_by_zero() {
        let msg = run_err("1 / 0");
        assert!(msg.contains("zero"), "got: {msg}");
    }

    #[test]
    fn eval_power() {
        assert_eq!(run("2 ** 10"), Value::Number(1024.0));
    }

    #[test]
    fn eval_modulo() {
        assert_eq!(run("17 % 5"), Value::Number(2.0));
    }
}
