//! Integration tests for the `rust-project` UCIL fixture.

use rust_project::parser::Parser;
use rust_project::transform::Transformer;
use rust_project::util::{evaluate, Value};

#[test]
fn end_to_end_arithmetic_expression() {
    let input = "let x = 6 in let y = 7 in x * y";
    let expr = Parser::new(input).parse_expr().expect("parse");
    let simplified = Transformer::simplify(&expr);
    let val = evaluate(&simplified).expect("eval");
    assert_eq!(val, Value::Number(42.0));
}

#[test]
fn end_to_end_if_expression() {
    let input = "if 3 > 2 then 100 else 0";
    let expr = Parser::new(input).parse_expr().expect("parse");
    let simplified = Transformer::simplify(&expr);
    let val = evaluate(&simplified).expect("eval");
    assert_eq!(val, Value::Number(100.0));
}

#[test]
fn transformer_free_vars_for_fixture_expression() {
    let input = "let a = 1 in a + b + c";
    let expr = Parser::new(input).parse_expr().expect("parse");
    let fv = Transformer::free_vars(&expr);
    assert!(!fv.contains("a"), "a is bound");
    assert!(fv.contains("b"), "b is free");
    assert!(fv.contains("c"), "c is free");
}
