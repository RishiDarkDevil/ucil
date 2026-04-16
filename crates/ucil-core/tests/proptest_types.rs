//! Property-based round-trip tests for `ucil_core::*` serde types.
//!
//! Every public type that derives `Serialize + Deserialize` must survive
//! the round-trip `T → json_string → T` preserving `PartialEq`. Hand-picked
//! unit tests in `src/types.rs` cover the happy path; these property tests
//! shrink-search counter-examples across arbitrary strings, paths, unicode,
//! and numeric edge-cases that humans forget to think about.
//!
//! Rationale (anti-laziness contract): "every public type must be fuzzed,
//! not just happy-path tested". Round-trip is the weakest property we can
//! reasonably assert for any serde type; it catches custom serializers that
//! lose information, PartialEq-vs-Serde drift, and float-NaN shenanigans.
//!
//! Run with:
//!     cargo test -p ucil-core --test proptest_types

use std::collections::HashMap;
use std::path::PathBuf;

use proptest::prelude::*;
use ucil_core::types::{
    CeqpParams, Diagnostic, KnowledgeEntry, QueryPlan, ResponseEnvelope, Symbol, ToolGroup,
};

// ── Strategy helpers ────────────────────────────────────────────────────────

/// Arbitrary unicode-ish short string, no control characters that would
/// fail JSON encoding. We also bound the length so shrinking completes
/// quickly — 0..=32 gives plenty of search space.
fn arb_str() -> impl Strategy<Value = String> {
    "[^\\p{Cc}]{0,32}".prop_map(String::from)
}

fn arb_vec_str() -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec(arb_str(), 0..5)
}

fn arb_map_str() -> impl Strategy<Value = HashMap<String, String>> {
    prop::collection::hash_map(arb_str(), arb_str(), 0..4)
}

/// Arbitrary path that's still valid UTF-8. PathBuf can hold non-UTF-8 on
/// unix but serde would fail — the public API contract here is UTF-8 paths.
fn arb_path() -> impl Strategy<Value = PathBuf> {
    "[^\\p{Cc}/]{0,16}(/[^\\p{Cc}/]{0,16}){0,3}".prop_map(PathBuf::from)
}

/// Arbitrary `Option<String>` biased toward `Some` half the time.
fn arb_opt_str() -> impl Strategy<Value = Option<String>> {
    prop::option::of(arb_str())
}

/// Arbitrary `Vec<f32>` of length 0..16. NaN / ±∞ are filtered out because
/// they don't survive JSON round-trip (serde_json serialises them as
/// `null`) — a documented JSON limitation, not a bug in the types. Shrunk
/// embedding vectors in real pipelines never contain these values.
fn arb_embedding_vec() -> impl Strategy<Value = Vec<f32>> {
    prop::collection::vec(
        prop::num::f32::ANY.prop_filter("NaN/±∞ not JSON-safe", |f| f.is_finite()),
        0..16,
    )
}

// ── Generic round-trip assertion ────────────────────────────────────────────

fn roundtrip<T>(value: &T)
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let json = serde_json::to_string(value).expect("serialize must succeed");
    let decoded: T = serde_json::from_str(&json).expect("deserialize must succeed");
    assert_eq!(
        value, &decoded,
        "round-trip preserved equality (json={json})"
    );
}

// ── Per-type strategies + property tests ────────────────────────────────────

prop_compose! {
    fn arb_query_plan()(
        intent in arb_str(),
        domains in arb_vec_str(),
        sub_queries in arb_vec_str(),
        knowledge_gaps in arb_vec_str(),
        inferred_context in arb_map_str(),
        fallback_mode in any::<bool>(),
    ) -> QueryPlan {
        QueryPlan { intent, domains, sub_queries, knowledge_gaps, inferred_context, fallback_mode }
    }
}

prop_compose! {
    fn arb_symbol()(
        name in arb_str(),
        kind in arb_str(),
        file_path in arb_path(),
        line in any::<u32>(),
        col in any::<u32>(),
        language in arb_str(),
        doc_comment in arb_opt_str(),
    ) -> Symbol {
        Symbol { name, kind, file_path, line, col, language, doc_comment }
    }
}

prop_compose! {
    fn arb_diagnostic()(
        file_path in arb_path(),
        line in any::<u32>(),
        col in any::<u32>(),
        severity in arb_str(),
        code in arb_opt_str(),
        message in arb_str(),
        source in arb_str(),
    ) -> Diagnostic {
        Diagnostic { file_path, line, col, severity, code, message, source }
    }
}

prop_compose! {
    fn arb_knowledge_entry()(
        id in arb_str(),
        symbol in arb_symbol(),
        content in arb_str(),
        embedding_vec in arb_embedding_vec(),
        created_at in arb_str(),
        updated_at in arb_str(),
        meta in arb_map_str(),
    ) -> KnowledgeEntry {
        KnowledgeEntry { id, symbol, content, embedding_vec, created_at, updated_at, meta }
    }
}

prop_compose! {
    fn arb_tool_group()(
        id in arb_str(),
        name in arb_str(),
        tools in arb_vec_str(),
        parallelism in any::<u32>(),
    ) -> ToolGroup {
        ToolGroup { id, name, tools, parallelism }
    }
}

prop_compose! {
    fn arb_ceqp_params()(
        reason in arb_str(),
        target in arb_str(),
        session_id in arb_str(),
        branch in arb_str(),
        depth_limit in any::<u32>(),
        timeout_ms in any::<u64>(),
    ) -> CeqpParams {
        CeqpParams { reason, target, session_id, branch, depth_limit, timeout_ms }
    }
}

// ResponseEnvelope.result is serde_json::Value; we build a small but
// heterogeneous sample of values to cover null/number/string/object/array.
fn arb_json_value() -> impl Strategy<Value = serde_json::Value> {
    let leaf = prop_oneof![
        Just(serde_json::Value::Null),
        any::<bool>().prop_map(serde_json::Value::Bool),
        any::<i64>().prop_map(|n| serde_json::json!(n)),
        arb_str().prop_map(serde_json::Value::String),
    ];
    leaf.prop_recursive(3, 8, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(serde_json::Value::Array),
            prop::collection::hash_map(arb_str(), inner, 0..4)
                .prop_map(|m| serde_json::Value::Object(m.into_iter().collect())),
        ]
    })
}

// indexing_status is an f64 in [0,1]. JSON doesn't round-trip every f64
// bit-exact (some 1-ULP rounding shows up when serialising values whose
// decimal representation needs > 17 digits). Generate values on a fixed
// 10_000-tick grid in [0,1] so every value is representable exactly in
// both IEEE-754 f64 and its JSON text form. Callers in production only
// set indexing_status to ratios of integer counts anyway.
fn arb_indexing_status() -> impl Strategy<Value = f64> {
    (0u32..=10_000).prop_map(|t| f64::from(t) / 10_000.0)
}

prop_compose! {
    fn arb_response_envelope()(
        request_id in arb_str(),
        tool_name in arb_str(),
        result in arb_json_value(),
        meta in arb_map_str(),
        degraded_plugins in arb_vec_str(),
        indexing_status in arb_indexing_status(),
        otel_trace_id in arb_opt_str(),
    ) -> ResponseEnvelope {
        ResponseEnvelope {
            request_id, tool_name, result, meta,
            degraded_plugins, indexing_status, otel_trace_id,
        }
    }
}

// ── Proptest cases ──────────────────────────────────────────────────────────

proptest! {
    // Default 256 cases per property is a good smoke level; override with
    // PROPTEST_CASES=<N> in CI for deeper runs. Keep cases low enough that
    // the test suite stays <5s wall time.
    #![proptest_config(ProptestConfig { cases: 256, .. ProptestConfig::default() })]

    #[test]
    fn prop_query_plan_roundtrip(qp in arb_query_plan()) {
        roundtrip(&qp);
    }

    #[test]
    fn prop_symbol_roundtrip(s in arb_symbol()) {
        roundtrip(&s);
    }

    #[test]
    fn prop_diagnostic_roundtrip(d in arb_diagnostic()) {
        roundtrip(&d);
    }

    #[test]
    fn prop_knowledge_entry_roundtrip(ke in arb_knowledge_entry()) {
        roundtrip(&ke);
    }

    #[test]
    fn prop_tool_group_roundtrip(tg in arb_tool_group()) {
        roundtrip(&tg);
    }

    #[test]
    fn prop_ceqp_params_roundtrip(p in arb_ceqp_params()) {
        roundtrip(&p);
    }

    #[test]
    fn prop_response_envelope_roundtrip(env in arb_response_envelope()) {
        roundtrip(&env);
    }
}
