//! OpenTelemetry initialisation helpers for UCIL — Phase 0 skeleton.
//!
//! Phase-log invariant 8: **stdout-only**.  No Jaeger, no OTLP collector
//! wiring ships before Phase 6.  All spans are emitted to stdout via
//! [`opentelemetry_stdout::SpanExporter`] using a `SimpleSpanProcessor`.
//!
//! # Usage
//!

//! ```no_run
//! use ucil_core::otel::{init_tracer, shutdown_tracer};
//! use opentelemetry::trace::{Span as _, Tracer as _};
//!
//! let tracer = init_tracer();
//! let mut span = tracer.start("my-operation");
//! // … do work …
//! span.end();
//! shutdown_tracer();
//! ```

use opentelemetry::global;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::trace::TracerProvider;
use opentelemetry_stdout::SpanExporter;

// ── Public API ────────────────────────────────────────────────────────────────

/// Installs a stdout-backed global tracer provider and returns a [`Tracer`]
/// named `ucil.core`.
///
/// The provider uses a `SimpleSpanProcessor` wired to
/// [`opentelemetry_stdout::SpanExporter`], which emits completed spans as
/// JSON to stdout.  Call [`shutdown_tracer`] at process exit to flush any
/// buffered spans.
///
/// Calling this function more than once in the same process is safe; the
/// second call replaces the global provider and returns a fresh tracer bound
/// to the new provider.
///
/// [`Tracer`]: opentelemetry_sdk::trace::Tracer
#[must_use]
pub fn init_tracer() -> opentelemetry_sdk::trace::Tracer {
    let exporter = SpanExporter::default();

    let provider = TracerProvider::builder()
        .with_simple_exporter(exporter)
        .build();

    // `set_tracer_provider` installs the provider globally and returns the
    // previous provider (which we do not need here).
    let _ = global::set_tracer_provider(provider.clone());

    provider.tracer("ucil.core")
}

/// Shuts down the global tracer provider, flushing any in-flight spans.
///
/// This is a thin wrapper around [`opentelemetry::global::shutdown_tracer_provider`].
/// Call it once at process exit (or at the end of integration tests) to
/// ensure all span data has been written to stdout.
pub fn shutdown_tracer() {
    global::shutdown_tracer_provider();
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use opentelemetry::trace::{Span as _, Tracer as _};
    use opentelemetry::KeyValue;

    use super::{init_tracer, shutdown_tracer};

    /// Verifies the full init → span → attribute → end → shutdown lifecycle
    /// completes without a panic.  This is the primary acceptance test for
    /// feature P0-W1-F09.
    #[test]
    fn init_span_shutdown_no_panic() {
        let tracer = init_tracer();

        let mut span = tracer.start("ucil-core-test-span");
        span.set_attribute(KeyValue::new("ucil.test.attr", "hello"));
        span.end();

        shutdown_tracer();
    }
}
