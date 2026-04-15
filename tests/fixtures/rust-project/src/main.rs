//! Entry point for the `rust-project` UCIL test fixture.
//!
//! This fixture is a small but real expression parser / evaluator that
//! exercises multiple Rust language features so that UCIL's tree-sitter
//! symbol extraction and quality analysis tools have meaningful input.

mod eval_ctx;
mod parser;
mod transform;
mod util;

use std::io::{self, BufRead};

use parser::Parser;
use transform::Transformer;

fn main() {
    let stdin = io::stdin();
    let mut total_lines = 0usize;
    let mut errors = 0usize;

    println!("rust-project expression evaluator — enter expressions, one per line.");
    println!("Type 'quit' or 'exit' to stop.\n");

    for line_result in stdin.lock().lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(e) => {
                eprintln!("I/O error: {e}");
                break;
            }
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "quit" || trimmed == "exit" {
            break;
        }

        total_lines += 1;

        let mut p = Parser::new(trimmed);
        match p.parse_expr() {
            Ok(expr) => {
                let simplified = Transformer::simplify(&expr);
                match util::evaluate(&simplified) {
                    Ok(val) => println!("  = {val}"),
                    Err(e) => {
                        eprintln!("  eval error: {e}");
                        errors += 1;
                    }
                }
            }
            Err(e) => {
                eprintln!("  parse error: {e}");
                errors += 1;
            }
        }
    }

    println!("\nProcessed {total_lines} expression(s), {errors} error(s).");
}
