//! Fixture integrity tests — verifies that the language test fixtures
//! exist on disk and (for Rust) compile successfully.

mod fixture {
    /// Verify that the rust-project fixture is present on disk and compiles.
    ///
    /// File-existence checks always run.
    /// The slow `cargo check` sub-process runs only when the environment
    /// variable `UCIL_SLOW_TESTS=1` is set, to keep `cargo test --workspace`
    /// fast in the normal CI loop.
    ///
    /// Run the full check:
    /// ```text
    /// UCIL_SLOW_TESTS=1 cargo test -p ucil-core --test fixture
    /// ```
    #[test]
    pub fn rust_project_loads() {
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

        // Compilation check is slow — opt-in via env var.
        if std::env::var("UCIL_SLOW_TESTS").as_deref() != Ok("1") {
            return;
        }

        let status = std::process::Command::new("cargo")
            .args(["check", "--manifest-path"])
            .arg(fixture.join("Cargo.toml"))
            .status()
            .expect("cargo check failed to spawn");

        assert!(status.success(), "rust-project fixture does not compile");
    }
}
