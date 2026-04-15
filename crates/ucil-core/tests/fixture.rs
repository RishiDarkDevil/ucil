//! Fixture integrity tests — verifies that the language test fixtures
//! exist on disk and (for Rust) compile successfully.

mod fixture {
    /// Verify that the rust-project fixture is present on disk and compiles.
    ///
    /// File-existence checks run as part of the compilation check.
    /// The test is marked `#[ignore]` (SLOW-TEST) because it spawns
    /// `cargo check` on the fixture which takes ~10–30 s.
    ///
    /// Run explicitly:
    /// ```text
    /// cargo test -p ucil-core -- --ignored fixture::rust_project_loads
    /// ```
    ///
    /// See `ucil-build/decisions/DEC-0003-slow-test-ignore-allowlist.md`.
    // SLOW-TEST: spawns `cargo check` on a 5.8 K-LOC fixture (~10-30 s).
    // Acceptance test P0-W1-F11 requires `--ignored` per feature-list.json.
    // See DEC-0003 for the SLOW-TEST exemption policy.
    #[ignore]
    #[test]
    fn rust_project_loads() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap() // workspace root
            .join("tests/fixtures/rust-project");

        assert!(
            fixture.join("Cargo.toml").exists(),
            "fixture Cargo.toml missing at {}",
            fixture.join("Cargo.toml").display()
        );
        assert!(
            fixture.join("src/main.rs").exists(),
            "fixture src/main.rs missing"
        );

        let status = std::process::Command::new("cargo")
            .args(["check", "--manifest-path"])
            .arg(fixture.join("Cargo.toml"))
            .status()
            .expect("cargo check failed to spawn");

        assert!(status.success(), "rust-project fixture does not compile");
    }
}
