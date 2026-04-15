/// Integration tests for the mixed-project Rust component.
///
/// The `test_intentionally_failing` test is marked `#[ignore]` because it is
/// SUPPOSED to fail — its failure is the point of the fixture. CI must NOT
/// run it in the normal suite.
///
/// UCIL diagnostic tests verify that the mixed-project fixture contains this
/// pattern: a skipped-but-failing test.

#[test]
fn test_binary_runs() {
    // Smoke test: just confirm the integration test harness itself compiles.
    assert_eq!(2 + 2, 4);
}

// INTENTIONALLY FAILING — marked #[ignore] so CI skips it.
// This test represents a "known broken" scenario the fixture is documenting.
#[test]
#[ignore = "SLOW-TEST: intentionally failing — fixture defect demonstration"]
fn test_intentionally_failing() {
    panic!("This test is intentionally failing. The mixed-project fixture contains broken tests by design.");
}
