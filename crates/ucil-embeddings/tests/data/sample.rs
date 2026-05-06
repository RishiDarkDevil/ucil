// Test fixture for `WO-0060` / `P2-W8-F05` —
// `crates/ucil-embeddings/src/chunker.rs::test_embedding_chunker_real_fixture`.
//
// This file is purely a chunker test fixture; it has NO production
// consumer. A future maintainer should NOT refactor it out — see
// the synthetic-tokenizer test contract in
// `crates/ucil-embeddings/src/chunker.rs` (frozen selector
// `chunker::test_embedding_chunker_real_fixture` per
// `feature-list.json:P2-W8-F05.acceptance_tests[0]`).
//
// Each function body is small enough that the synthetic
// `WordLevel + WhitespaceSplit` tokenizer's whitespace token count
// stays under `MAX_CHUNK_TOKENS = 512`.

//! Sample math helpers.

/// Add two integers.
fn add(a: i32, b: i32) -> i32 {
    a + b
}

/// Multiply two integers.
fn multiply(a: i32, b: i32) -> i32 {
    a * b
}

/// Divide two integers, returning `None` on divide-by-zero.
fn divide(a: i32, b: i32) -> Option<i32> {
    if b == 0 {
        None
    } else {
        Some(a / b)
    }
}

/// A trivial calculator type wired over the helpers above.
struct Calculator {
    accumulator: i32,
}

impl Calculator {
    /// Construct a fresh calculator at zero.
    fn new() -> Self {
        Self { accumulator: 0 }
    }

    /// Apply `add` and store the result.
    fn add(&mut self, value: i32) {
        self.accumulator = add(self.accumulator, value);
    }
}
