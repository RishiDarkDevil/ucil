//! AST transformer and constant-folder for the expression language.
//!
//! The [`Transformer`] provides a collection of pure, non-mutating operations
//! over [`crate::parser::Expr`] trees:
//!
//! - **Constant folding** — evaluate sub-expressions whose operands are fully
//!   known at compile time, producing a smaller, equivalent tree.
//! - **Free-variable collection** — enumerate every [`Expr::Variable`] that is
//!   not bound by a surrounding [`Expr::Let`].
//! - **Node counting** — measure the size of an expression tree.
//! - **Substitution** — replace every free occurrence of a variable with a
//!   concrete expression.
//! - **Pretty-printing** — render an expression tree back to human-readable
//!   source text.
//!
//! All public methods on [`Transformer`] are stateless associated functions —
//! no `&self` receiver — because the transformer carries no configuration.
//!
//! # Folding rules
//!
//! Arithmetic folds (both operands must be numeric literals):
//! - `a + b`, `a - b`, `a * b`, `a / b` (with div-by-zero guard), `a % b`, `a ** b`
//!
//! Comparison folds (both operands must be numeric literals):
//! - `a == b`, `a != b`, `a < b`, `a <= b`, `a > b`, `a >= b`
//!
//! Boolean short-circuit folds:
//! - `false && _` → `false`
//! - `true  || _` → `true`
//! - `true  && rhs` → `rhs`
//! - `false || rhs` → `rhs`
//! - `Bool(a) && Bool(b)` → `Bool(a && b)`
//! - `Bool(a) || Bool(b)` → `Bool(a || b)`
//!
//! Unary folds:
//! - `-Number(n)` → `Number(-n)`
//! - `!Bool(b)`   → `Bool(!b)`
//! - `--x`        → `x`  (double-negation elimination)
//! - `!!x`        → `x`  (double-not elimination)
//!
//! If folds:
//! - `if true  then t else _` → `t`
//! - `if false then _ else e` → `e`
//!
//! Algebraic identities (one operand is a known constant):
//! - `x + 0`, `0 + x` → `x`
//! - `x - 0` → `x`
//! - `x * 1`, `1 * x` → `x`
//! - `x * 0`, `0 * x` → `Number(0.0)`
//! - `x / 1` → `x`
//! - `x ** 0` → `Number(1.0)`
//! - `x ** 1` → `x`

use std::collections::BTreeSet;

use crate::parser::{BinOp, Expr, UnOp};

// ---------------------------------------------------------------------------
// Transformer
// ---------------------------------------------------------------------------

/// A collection of stateless AST transformations.
///
/// Every method is an associated function so callers never need to
/// instantiate a `Transformer`.
pub struct Transformer;

impl Transformer {
    // -----------------------------------------------------------------------
    // simplify — constant-fold an expression tree (bottom-up)
    // -----------------------------------------------------------------------

    /// Simplify / constant-fold an expression tree.
    ///
    /// Walks the tree bottom-up; internal nodes are first recursively
    /// simplified before the folding rules are applied.  The input is never
    /// mutated — a new tree is returned.
    pub fn simplify(expr: &Expr) -> Expr {
        match expr {
            // ── leaves ───────────────────────────────────────────────────
            Expr::Number(n) => Expr::Number(*n),
            Expr::Bool(b) => Expr::Bool(*b),
            Expr::Variable(s) => Expr::Variable(s.clone()),

            // ── binary operations ────────────────────────────────────────
            Expr::BinaryOp { op, lhs, rhs } => {
                let left = Self::simplify(lhs);
                let right = Self::simplify(rhs);
                Self::fold_binary(*op, left, right)
            }

            // ── unary operations ─────────────────────────────────────────
            Expr::UnaryOp { op, operand } => {
                let inner = Self::simplify(operand);
                Self::fold_unary(*op, inner)
            }

            // ── if / then / else ─────────────────────────────────────────
            Expr::If { cond, then_, else_ } => {
                let cond_s = Self::simplify(cond);
                match &cond_s {
                    Expr::Bool(true) => Self::simplify(then_),
                    Expr::Bool(false) => Self::simplify(else_),
                    _ => Expr::If {
                        cond: Box::new(cond_s),
                        then_: Box::new(Self::simplify(then_)),
                        else_: Box::new(Self::simplify(else_)),
                    },
                }
            }

            // ── let bindings ─────────────────────────────────────────────
            Expr::Let { name, value, body } => {
                let value_s = Self::simplify(value);
                // Inline only when the value is a known literal.
                if matches!(value_s, Expr::Number(_) | Expr::Bool(_)) {
                    let inlined = Self::substitute(body, name, &value_s);
                    Self::simplify(&inlined)
                } else {
                    Expr::Let {
                        name: name.clone(),
                        value: Box::new(value_s),
                        body: Box::new(Self::simplify(body)),
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // free_vars — collect unbound variable names
    // -----------------------------------------------------------------------

    /// Collect all free variable names appearing in an expression.
    ///
    /// A variable is *free* when it is not bound by a surrounding
    /// [`Expr::Let`].
    pub fn free_vars(expr: &Expr) -> BTreeSet<String> {
        let mut set = BTreeSet::new();
        Self::collect_free(expr, &BTreeSet::new(), &mut set);
        set
    }

    fn collect_free(expr: &Expr, bound: &BTreeSet<String>, out: &mut BTreeSet<String>) {
        match expr {
            Expr::Number(_) | Expr::Bool(_) => {}
            Expr::Variable(name) => {
                if !bound.contains(name) {
                    out.insert(name.clone());
                }
            }
            Expr::BinaryOp { lhs, rhs, .. } => {
                Self::collect_free(lhs, bound, out);
                Self::collect_free(rhs, bound, out);
            }
            Expr::UnaryOp { operand, .. } => Self::collect_free(operand, bound, out),
            Expr::If { cond, then_, else_ } => {
                Self::collect_free(cond, bound, out);
                Self::collect_free(then_, bound, out);
                Self::collect_free(else_, bound, out);
            }
            Expr::Let { name, value, body } => {
                Self::collect_free(value, bound, out);
                let mut new_bound = bound.clone();
                new_bound.insert(name.clone());
                Self::collect_free(body, &new_bound, out);
            }
        }
    }

    // -----------------------------------------------------------------------
    // node_count — count AST nodes
    // -----------------------------------------------------------------------

    /// Count the number of AST nodes in the expression tree.
    ///
    /// Every variant counts as 1; sub-expressions counted recursively.
    pub fn node_count(expr: &Expr) -> usize {
        match expr {
            Expr::Number(_) | Expr::Bool(_) | Expr::Variable(_) => 1,
            Expr::BinaryOp { lhs, rhs, .. } => 1 + Self::node_count(lhs) + Self::node_count(rhs),
            Expr::UnaryOp { operand, .. } => 1 + Self::node_count(operand),
            Expr::If { cond, then_, else_ } => {
                1 + Self::node_count(cond) + Self::node_count(then_) + Self::node_count(else_)
            }
            Expr::Let { value, body, .. } => 1 + Self::node_count(value) + Self::node_count(body),
        }
    }

    // -----------------------------------------------------------------------
    // depth — maximum nesting depth
    // -----------------------------------------------------------------------

    /// Return the maximum nesting depth of the expression tree.
    ///
    /// A leaf has depth 1.  An internal node has depth `1 + max(children)`.
    pub fn depth(expr: &Expr) -> usize {
        match expr {
            Expr::Number(_) | Expr::Bool(_) | Expr::Variable(_) => 1,
            Expr::BinaryOp { lhs, rhs, .. } => 1 + Self::depth(lhs).max(Self::depth(rhs)),
            Expr::UnaryOp { operand, .. } => 1 + Self::depth(operand),
            Expr::If { cond, then_, else_ } => {
                1 + Self::depth(cond)
                    .max(Self::depth(then_))
                    .max(Self::depth(else_))
            }
            Expr::Let { value, body, .. } => 1 + Self::depth(value).max(Self::depth(body)),
        }
    }

    // -----------------------------------------------------------------------
    // substitute — replace a free variable
    // -----------------------------------------------------------------------

    /// Substitute all *free* occurrences of `var` with `replacement`.
    ///
    /// Correctly handles shadowing: if a `Let` re-binds `var`, the
    /// replacement does not propagate into the body.
    pub fn substitute(expr: &Expr, var: &str, replacement: &Expr) -> Expr {
        match expr {
            Expr::Number(n) => Expr::Number(*n),
            Expr::Bool(b) => Expr::Bool(*b),
            Expr::Variable(name) => {
                if name == var {
                    replacement.clone()
                } else {
                    Expr::Variable(name.clone())
                }
            }
            Expr::BinaryOp { op, lhs, rhs } => Expr::BinaryOp {
                op: *op,
                lhs: Box::new(Self::substitute(lhs, var, replacement)),
                rhs: Box::new(Self::substitute(rhs, var, replacement)),
            },
            Expr::UnaryOp { op, operand } => Expr::UnaryOp {
                op: *op,
                operand: Box::new(Self::substitute(operand, var, replacement)),
            },
            Expr::If { cond, then_, else_ } => Expr::If {
                cond: Box::new(Self::substitute(cond, var, replacement)),
                then_: Box::new(Self::substitute(then_, var, replacement)),
                else_: Box::new(Self::substitute(else_, var, replacement)),
            },
            Expr::Let { name, value, body } => {
                let new_value = Self::substitute(value, var, replacement);
                if name == var {
                    // The let re-binds `var`; do not substitute inside body.
                    Expr::Let {
                        name: name.clone(),
                        value: Box::new(new_value),
                        body: body.clone(),
                    }
                } else {
                    Expr::Let {
                        name: name.clone(),
                        value: Box::new(new_value),
                        body: Box::new(Self::substitute(body, var, replacement)),
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // pretty_print — render expression to source text
    // -----------------------------------------------------------------------

    /// Pretty-print an expression tree back to a human-readable string.
    pub fn pretty_print(expr: &Expr) -> String {
        match expr {
            Expr::Number(n) => {
                if n.fract() == 0.0 && n.is_finite() {
                    format!("{}", *n as i64)
                } else {
                    format!("{n}")
                }
            }
            Expr::Bool(b) => b.to_string(),
            Expr::Variable(name) => name.clone(),
            Expr::BinaryOp { op, lhs, rhs } => {
                format!(
                    "({} {} {})",
                    Self::pretty_print(lhs),
                    op,
                    Self::pretty_print(rhs)
                )
            }
            Expr::UnaryOp { op, operand } => {
                format!("({}{})", op, Self::pretty_print(operand))
            }
            Expr::If { cond, then_, else_ } => {
                format!(
                    "if {} then {} else {}",
                    Self::pretty_print(cond),
                    Self::pretty_print(then_),
                    Self::pretty_print(else_)
                )
            }
            Expr::Let { name, value, body } => {
                format!(
                    "let {} = {} in {}",
                    name,
                    Self::pretty_print(value),
                    Self::pretty_print(body)
                )
            }
        }
    }

    // -----------------------------------------------------------------------
    // is_constant — true if no free variables
    // -----------------------------------------------------------------------

    /// Return `true` when the expression contains no free variables.
    pub fn is_constant(expr: &Expr) -> bool {
        Self::free_vars(expr).is_empty()
    }

    // -----------------------------------------------------------------------
    // contains_var — quick membership check
    // -----------------------------------------------------------------------

    /// Return `true` if `var` appears free anywhere in `expr`.
    pub fn contains_var(expr: &Expr, var: &str) -> bool {
        match expr {
            Expr::Number(_) | Expr::Bool(_) => false,
            Expr::Variable(name) => name == var,
            Expr::BinaryOp { lhs, rhs, .. } => {
                Self::contains_var(lhs, var) || Self::contains_var(rhs, var)
            }
            Expr::UnaryOp { operand, .. } => Self::contains_var(operand, var),
            Expr::If { cond, then_, else_ } => {
                Self::contains_var(cond, var)
                    || Self::contains_var(then_, var)
                    || Self::contains_var(else_, var)
            }
            Expr::Let { name, value, body } => {
                Self::contains_var(value, var) || (name != var && Self::contains_var(body, var))
            }
        }
    }

    // -----------------------------------------------------------------------
    // map_numbers — apply a function to every numeric leaf
    // -----------------------------------------------------------------------

    /// Apply `f` to every [`Expr::Number`] leaf, returning a new tree.
    pub fn map_numbers<F: Fn(f64) -> f64>(expr: &Expr, f: &F) -> Expr {
        match expr {
            Expr::Number(n) => Expr::Number(f(*n)),
            Expr::Bool(b) => Expr::Bool(*b),
            Expr::Variable(s) => Expr::Variable(s.clone()),
            Expr::BinaryOp { op, lhs, rhs } => Expr::BinaryOp {
                op: *op,
                lhs: Box::new(Self::map_numbers(lhs, f)),
                rhs: Box::new(Self::map_numbers(rhs, f)),
            },
            Expr::UnaryOp { op, operand } => Expr::UnaryOp {
                op: *op,
                operand: Box::new(Self::map_numbers(operand, f)),
            },
            Expr::If { cond, then_, else_ } => Expr::If {
                cond: Box::new(Self::map_numbers(cond, f)),
                then_: Box::new(Self::map_numbers(then_, f)),
                else_: Box::new(Self::map_numbers(else_, f)),
            },
            Expr::Let { name, value, body } => Expr::Let {
                name: name.clone(),
                value: Box::new(Self::map_numbers(value, f)),
                body: Box::new(Self::map_numbers(body, f)),
            },
        }
    }

    // -----------------------------------------------------------------------
    // simplify_deep — iterate simplify to a fixed point
    // -----------------------------------------------------------------------

    /// Run [`simplify`] repeatedly until the tree stops changing or
    /// `max_iterations` is reached.
    pub fn simplify_deep(expr: &Expr, max_iterations: usize) -> Expr {
        let mut current = expr.clone();
        for _ in 0..max_iterations {
            let next = Self::simplify(&current);
            if next == current {
                break;
            }
            current = next;
        }
        current
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn fold_binary(op: BinOp, left: Expr, right: Expr) -> Expr {
        match (op, &left, &right) {
            // ── arithmetic folds ─────────────────────────────────────────
            (BinOp::Add, Expr::Number(a), Expr::Number(b)) => Expr::Number(a + b),
            (BinOp::Sub, Expr::Number(a), Expr::Number(b)) => Expr::Number(a - b),
            (BinOp::Mul, Expr::Number(a), Expr::Number(b)) => Expr::Number(a * b),
            (BinOp::Div, Expr::Number(a), Expr::Number(b)) if *b != 0.0 => Expr::Number(a / b),
            (BinOp::Mod, Expr::Number(a), Expr::Number(b)) if *b != 0.0 => Expr::Number(a % b),
            (BinOp::Pow, Expr::Number(a), Expr::Number(b)) => Expr::Number(a.powf(*b)),

            // ── comparison folds ─────────────────────────────────────────
            (BinOp::Eq, Expr::Number(a), Expr::Number(b)) => {
                Expr::Bool((a - b).abs() < f64::EPSILON)
            }
            (BinOp::Ne, Expr::Number(a), Expr::Number(b)) => {
                Expr::Bool((a - b).abs() >= f64::EPSILON)
            }
            (BinOp::Lt, Expr::Number(a), Expr::Number(b)) => Expr::Bool(a < b),
            (BinOp::Le, Expr::Number(a), Expr::Number(b)) => Expr::Bool(a <= b),
            (BinOp::Gt, Expr::Number(a), Expr::Number(b)) => Expr::Bool(a > b),
            (BinOp::Ge, Expr::Number(a), Expr::Number(b)) => Expr::Bool(a >= b),

            // ── boolean short-circuit ────────────────────────────────────
            (BinOp::And, Expr::Bool(false), _) => Expr::Bool(false),
            (BinOp::Or, Expr::Bool(true), _) => Expr::Bool(true),
            (BinOp::And, Expr::Bool(true), _) => right,
            (BinOp::Or, Expr::Bool(false), _) => right,
            (BinOp::And, Expr::Bool(a), Expr::Bool(b)) => Expr::Bool(*a && *b),
            (BinOp::Or, Expr::Bool(a), Expr::Bool(b)) => Expr::Bool(*a || *b),

            // ── algebraic identities ──────────────────────────────────────
            (BinOp::Add, Expr::Number(n), _) if *n == 0.0 => right,
            (BinOp::Add, _, Expr::Number(n)) if *n == 0.0 => left,
            (BinOp::Sub, _, Expr::Number(n)) if *n == 0.0 => left,
            (BinOp::Mul, Expr::Number(n), _) if *n == 1.0 => right,
            (BinOp::Mul, _, Expr::Number(n)) if *n == 1.0 => left,
            (BinOp::Mul, Expr::Number(n), _) if *n == 0.0 => Expr::Number(0.0),
            (BinOp::Mul, _, Expr::Number(n)) if *n == 0.0 => Expr::Number(0.0),
            (BinOp::Div, _, Expr::Number(n)) if *n == 1.0 => left,
            (BinOp::Pow, _, Expr::Number(n)) if *n == 0.0 => Expr::Number(1.0),
            (BinOp::Pow, _, Expr::Number(n)) if *n == 1.0 => left,

            // ── no applicable rule ────────────────────────────────────────
            _ => Expr::BinaryOp {
                op,
                lhs: Box::new(left),
                rhs: Box::new(right),
            },
        }
    }

    fn fold_unary(op: UnOp, inner: Expr) -> Expr {
        match (op, &inner) {
            (UnOp::Neg, Expr::Number(n)) => Expr::Number(-n),
            (UnOp::Not, Expr::Bool(b)) => Expr::Bool(!b),
            // Double-negation elimination.
            (
                UnOp::Neg,
                Expr::UnaryOp {
                    op: UnOp::Neg,
                    operand,
                },
            ) => *operand.clone(),
            (
                UnOp::Not,
                Expr::UnaryOp {
                    op: UnOp::Not,
                    operand,
                },
            ) => *operand.clone(),
            _ => Expr::UnaryOp {
                op,
                operand: Box::new(inner),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::parser::{BinOp, Expr, Parser, UnOp};

    use super::Transformer;

    fn num(n: f64) -> Expr {
        Expr::Number(n)
    }

    fn bool_(b: bool) -> Expr {
        Expr::Bool(b)
    }

    fn var(s: &str) -> Expr {
        Expr::Variable(s.to_owned())
    }

    fn binop(op: BinOp, l: Expr, r: Expr) -> Expr {
        Expr::BinaryOp {
            op,
            lhs: Box::new(l),
            rhs: Box::new(r),
        }
    }

    fn unop(op: UnOp, e: Expr) -> Expr {
        Expr::UnaryOp {
            op,
            operand: Box::new(e),
        }
    }

    fn parse(s: &str) -> Expr {
        Parser::new(s).parse_expr().expect("parse ok")
    }

    #[test]
    fn fold_addition() {
        let e = binop(BinOp::Add, num(3.0), num(4.0));
        assert_eq!(Transformer::simplify(&e), num(7.0));
    }

    #[test]
    fn fold_subtraction() {
        let e = binop(BinOp::Sub, num(10.0), num(3.0));
        assert_eq!(Transformer::simplify(&e), num(7.0));
    }

    #[test]
    fn fold_multiplication() {
        let e = binop(BinOp::Mul, num(6.0), num(7.0));
        assert_eq!(Transformer::simplify(&e), num(42.0));
    }

    #[test]
    fn fold_nested_add_mul() {
        // (2 + 3) * 4  →  20
        let e = binop(BinOp::Mul, binop(BinOp::Add, num(2.0), num(3.0)), num(4.0));
        assert_eq!(Transformer::simplify(&e), num(20.0));
    }

    #[test]
    fn fold_comparison_lt() {
        let e = binop(BinOp::Lt, num(1.0), num(2.0));
        assert_eq!(Transformer::simplify(&e), bool_(true));
    }

    #[test]
    fn fold_boolean_short_circuit_and_false() {
        let e = binop(BinOp::And, bool_(false), var("x"));
        assert_eq!(Transformer::simplify(&e), bool_(false));
    }

    #[test]
    fn fold_boolean_short_circuit_or_true() {
        let e = binop(BinOp::Or, bool_(true), var("x"));
        assert_eq!(Transformer::simplify(&e), bool_(true));
    }

    #[test]
    fn fold_unary_neg() {
        let e = unop(UnOp::Neg, num(5.0));
        assert_eq!(Transformer::simplify(&e), num(-5.0));
    }

    #[test]
    fn fold_unary_not() {
        let e = unop(UnOp::Not, bool_(true));
        assert_eq!(Transformer::simplify(&e), bool_(false));
    }

    #[test]
    fn fold_double_negation() {
        let e = unop(UnOp::Neg, unop(UnOp::Neg, var("x")));
        assert_eq!(Transformer::simplify(&e), var("x"));
    }

    #[test]
    fn fold_if_true_branch() {
        let e = Expr::If {
            cond: Box::new(bool_(true)),
            then_: Box::new(num(1.0)),
            else_: Box::new(num(2.0)),
        };
        assert_eq!(Transformer::simplify(&e), num(1.0));
    }

    #[test]
    fn fold_if_false_branch() {
        let e = Expr::If {
            cond: Box::new(bool_(false)),
            then_: Box::new(num(1.0)),
            else_: Box::new(num(2.0)),
        };
        assert_eq!(Transformer::simplify(&e), num(2.0));
    }

    #[test]
    fn fold_let_inlines_literal() {
        // let x = 3 in x + 4  →  7
        let e = parse("let x = 3 in x + 4");
        assert_eq!(Transformer::simplify(&e), num(7.0));
    }

    #[test]
    fn fold_algebraic_add_zero() {
        let e = binop(BinOp::Add, var("x"), num(0.0));
        assert_eq!(Transformer::simplify(&e), var("x"));
    }

    #[test]
    fn fold_algebraic_mul_one() {
        let e = binop(BinOp::Mul, num(1.0), var("x"));
        assert_eq!(Transformer::simplify(&e), var("x"));
    }

    #[test]
    fn free_vars_simple() {
        let fv = Transformer::free_vars(&var("z"));
        assert!(fv.contains("z") && fv.len() == 1);
    }

    #[test]
    fn free_vars_let_binds() {
        let e = parse("let x = 1 in x + y");
        let fv = Transformer::free_vars(&e);
        assert!(!fv.contains("x"));
        assert!(fv.contains("y"));
    }

    #[test]
    fn free_vars_literal_is_empty() {
        assert!(Transformer::free_vars(&num(42.0)).is_empty());
    }

    #[test]
    fn node_count_leaf() {
        assert_eq!(Transformer::node_count(&num(1.0)), 1);
    }

    #[test]
    fn node_count_binary() {
        let e = binop(BinOp::Add, num(1.0), num(2.0));
        assert_eq!(Transformer::node_count(&e), 3);
    }

    #[test]
    fn substitute_replaces_free() {
        let e = binop(BinOp::Add, var("x"), var("y"));
        let result = Transformer::substitute(&e, "x", &num(5.0));
        assert_eq!(result, binop(BinOp::Add, num(5.0), var("y")));
    }

    #[test]
    fn substitute_respects_shadowing() {
        // let y = x + 1 in y + z, substitute x -> 99
        // x appears in the VALUE (x + 1) → that gets replaced.
        // y is bound by the let, so z and y in the body are unaffected.
        // x does NOT appear free in the body (only y and z do).
        let e = Expr::Let {
            name: "y".to_owned(),
            value: Box::new(binop(BinOp::Add, var("x"), num(1.0))),
            body: Box::new(binop(BinOp::Add, var("y"), var("z"))),
        };
        let result = Transformer::substitute(&e, "x", &num(99.0));
        if let Expr::Let { value, body, .. } = result {
            // x in the value must have been replaced with 99.
            assert_eq!(*value, binop(BinOp::Add, num(99.0), num(1.0)));
            // y (the bound name) and z remain in the body.
            assert!(Transformer::contains_var(&body, "y"));
            assert!(Transformer::contains_var(&body, "z"));
        } else {
            panic!("expected Let");
        }
    }

    #[test]
    fn pretty_print_arithmetic() {
        let e = binop(BinOp::Add, num(1.0), num(2.0));
        assert_eq!(Transformer::pretty_print(&e), "(1 + 2)");
    }

    #[test]
    fn pretty_print_let() {
        let e = Expr::Let {
            name: "x".to_owned(),
            value: Box::new(num(3.0)),
            body: Box::new(var("x")),
        };
        assert_eq!(Transformer::pretty_print(&e), "let x = 3 in x");
    }

    #[test]
    fn is_constant_literal() {
        assert!(Transformer::is_constant(&num(7.0)));
    }

    #[test]
    fn is_constant_false_for_var() {
        assert!(!Transformer::is_constant(&var("a")));
    }

    #[test]
    fn map_numbers_doubles_all() {
        let e = binop(BinOp::Add, num(3.0), num(4.0));
        let doubled = Transformer::map_numbers(&e, &|n| n * 2.0);
        assert_eq!(doubled, binop(BinOp::Add, num(6.0), num(8.0)));
    }

    #[test]
    fn simplify_deep_fixed_point() {
        // let x = 2 in let y = x + 1 in y * 3  →  9
        let e = parse("let x = 2 in let y = x + 1 in y * 3");
        let result = Transformer::simplify_deep(&e, 10);
        assert_eq!(result, num(9.0));
    }

    #[test]
    fn depth_flat_binop() {
        let e = binop(BinOp::Add, num(1.0), num(2.0));
        assert_eq!(Transformer::depth(&e), 2);
    }

    #[test]
    fn depth_nested() {
        let e = binop(BinOp::Add, binop(BinOp::Add, num(1.0), num(2.0)), num(3.0));
        assert_eq!(Transformer::depth(&e), 3);
    }
}
