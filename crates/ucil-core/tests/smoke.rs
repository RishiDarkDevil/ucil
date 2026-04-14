//! Smoke test: verify the ucil-core crate compiles and exposes VERSION.

#[test]
fn version_is_semver() {
    let v = ucil_core::VERSION;
    // Must be non-empty and start with a digit (semver)
    assert!(!v.is_empty(), "VERSION must not be empty");
    assert!(
        v.chars().next().map_or(false, |c| c.is_ascii_digit()),
        "VERSION must start with a digit, got: {v}"
    );
}

#[test]
fn version_has_two_dots() {
    // A minimal semver X.Y.Z has at least two dots.
    let v = ucil_core::VERSION;
    assert!(
        v.chars().filter(|&c| c == '.').count() >= 2,
        "VERSION must have at least two dots (semver), got: {v}"
    );
}
