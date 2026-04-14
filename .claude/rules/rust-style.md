# Rust style rules (UCIL)

## Edition & toolchain
- `edition = "2021"` (or `"2024"` once stabilised in project toolchain).
- `rust-toolchain.toml` pins an exact stable version per-workspace; no floating.
- `rustfmt` default config; no project-specific overrides without an ADR.

## Clippy
- Baseline: `#![deny(warnings)]` in every crate root when CI runs clippy.
- Enable `clippy::pedantic` warnings but fix them (don't blanket-allow).
- Opt-in per-crate `#![warn(clippy::all, clippy::pedantic, clippy::nursery)]`.

## Crate layout
- One crate per `Cargo.toml` dir.
- `src/lib.rs` is **only** re-exports and module declarations — no logic.
- `#[cfg(test)]` for unit; `tests/` dir for integration; `benches/` for criterion.
- `pub(crate)` is the default visibility; promote to `pub` only when a consumer needs it.

## Errors
- Libraries: `thiserror` + a single `Error` enum per crate, `#[non_exhaustive]`.
- Binaries: `anyhow` with `.context(...)` at every boundary.
- Never `.unwrap()` / `.expect()` outside `#[cfg(test)]`. Use `?` and a typed error.

## Async
- Runtime: `tokio` with `rt-multi-thread`.
- Every `.await` that touches IO is wrapped in `tokio::time::timeout` with a named const.
- Spawning: prefer `tokio::spawn` over blocking threads; use `spawn_blocking` only for CPU-bound.
- Channels: `tokio::sync::mpsc` for typed message passing; avoid `std::sync::mpsc` in async.

## Tracing
- `tracing` crate for all logs.
- Spans named `ucil.<layer>.<op>` per the master plan §15.2.
- No `println!` / `eprintln!` in library code.

## Concurrency
- No global mutable state outside `OnceLock` / `LazyLock` / `Mutex<_>` / `RwLock<_>`.
- Prefer `DashMap` for concurrent maps. `parking_lot::Mutex` when blocking is acceptable.

## Docs
- Every public item has a rustdoc with `# Examples` when non-trivial.
- `#![deny(rustdoc::broken_intra_doc_links)]` per-crate.

## Testing
- `cargo nextest` is the default runner.
- Integration tests against real processes (Serena, LSP, SQLite, LanceDB, Docker).
  Mocking these specific collaborators is forbidden — use the docker fixtures.
- Criterion benches land in `benches/` with named groups matching the feature-list entry.

## Unsafe
- Forbidden by default. Any `unsafe` block requires an ADR justification and `// SAFETY:` comment explaining invariants.
