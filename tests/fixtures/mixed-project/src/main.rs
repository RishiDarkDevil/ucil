//! Mixed-project Rust component.
//!
//! This file intentionally contains several lint defects used to test
//! UCIL's diagnostic capabilities. Do not clean up these defects.

// DEFECT 1: unused import — triggers `unused_imports` compiler warning.
use std::collections::BTreeSet;

// DEFECT 2: #[allow(dead_code)] suppresses the warning for this specific
// dead function, but the suppression is "unused" in the sense that the
// fix is cosmetic rather than addressing the root problem (function is
// never called anywhere).
#[allow(dead_code)]
fn count_words(text: &str) -> usize {
    text.split_whitespace().count()
}

// DEFECT 3: dead function WITHOUT allow — generates `dead_code` warning.
fn format_report(title: &str, body: &str) -> String {
    format!("=== {title} ===\n{body}\n")
}

/// Looks up a user record by name from a slice.
///
/// # Defects
///
/// DEFECT 4: `.unwrap()` on the return of `Iterator::find`, which returns
/// `Option<T>`. If no user matches, this panics at runtime.
/// SQL-INJECTION-STYLE COMMENT below: the concatenation in `build_query`
/// is the idiomatic source of injection bugs.
fn get_user(users: &[String], name: &str) -> String {
    // NOTE: This will panic if `name` is not found in `users`.
    users.iter().find(|u| u.as_str() == name).unwrap().clone()
}

/// Builds a raw query string by concatenating user-controlled input.
///
/// # Security
///
/// FIXME: SQL-injection risk — do not use in production without
/// parameterised queries. The concatenation below is intentional
/// for fixture purposes only.
fn build_query(table: &str, user_input: &str) -> String {
    // Intentionally unsafe string concatenation (SQL-injection pattern).
    "SELECT * FROM ".to_owned() + table + " WHERE name = '" + user_input + "'"
}

fn main() {
    let users = vec!["alice".to_string(), "bob".to_string()];
    let first = get_user(&users, "alice");
    let query = build_query("users", &first);
    println!("Query: {query}");
    println!("Report:\n{}", format_report("Users", &first));
}
