//! CEQP — Continuous Evaluation and Query Planning, free-text reason parser.
//!
//! Master-plan §8.3 lines 772-774 freezes the contract for the
//! deterministic-fallback `reason` parser: extract `intent`,
//! `domains`, `planned_action`, and `knowledge_gaps` from the
//! free-text `reason` parameter that every CEQP-aware MCP tool
//! accepts.  When the LLM `QueryInterpreter` agent
//! (`P3.5-W12-F02`) is unavailable — i.e. `provider = "none"` per
//! §7.1 lines 693-695 — UCIL falls back to this pure-keyword parser
//! to populate the same fields.
//!
//! This module is pure: no IO, no async, no logging, no plugins.  It
//! does NOT depend on the `regex` crate by design — `ucil-core` is
//! the workspace's lowest-level Rust crate; pulling `regex` here
//! would propagate a 200KB binary cost and a transitive
//! `aho-corasick` dep across every downstream build.  Phrase
//! recognition uses `str::contains` and a hand-rolled noun-phrase
//! scan over byte indices.

use serde::{Deserialize, Serialize};

// ── Intent ────────────────────────────────────────────────────────────────────

/// One of the 5 canonical CEQP intent classes per master-plan §8.3
/// line 773 (`add_feature`, `fix_bug`, `refactor`, `understand`,
/// `review`).
///
/// `Default` is [`Intent::Understand`] — the most-permissive bonus-
/// context default per master-plan §8.6 lines 817-822.
///
/// `serde(rename_all = "snake_case")` produces wire labels
/// `"add_feature"`, `"fix_bug"`, `"refactor"`, `"understand"`,
/// `"review"` exactly matching the §8.3 enumeration.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    /// New user-visible capability — `"add feature"`, `"new feature"`,
    /// `"implement"`.
    AddFeature,
    /// Bug fix — `"fix"`, `"bug"`, `"broken"`, `"crash"`, `"error"`.
    FixBug,
    /// Refactor or rename — `"refactor"`, `"clean up"`, `"rename"`,
    /// `"restructure"`.
    Refactor,
    /// Understand existing code — `"explain"`, `"understand"`,
    /// `"how does"`, `"what is"`, `"why"`.  The default per §8.6.
    #[default]
    Understand,
    /// Code review — `"review"`, `"audit"`, `"check"`, `"diff"`,
    /// `"pr"`.
    Review,
}

// ── PlannedAction ─────────────────────────────────────────────────────────────

/// Coarse planned-action classification per master-plan §8.3 line
/// 773 (`edit`, `read`, `explain`).  `Other` covers anything that
/// doesn't fit the three primary actions.
///
/// `Default` is [`PlannedAction::Read`] — read-only is the safest
/// fallback for the bonus-context selector when the action is not
/// explicit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlannedAction {
    /// Edit, modify, update, add, create, or write source.
    Edit,
    /// Read-only inspection — read, look, check, inspect, see.
    /// The default per §8.6.
    #[default]
    Read,
    /// Explain or document — explain, describe, summarise, document.
    Explain,
    /// None of the three primary actions.
    Other,
}

// ── ParsedReason ──────────────────────────────────────────────────────────────

/// Output of [`parse_reason`] — the four fields per master-plan §8.3
/// line 773 that drive group-weight boosting, bonus-context
/// selection, and synthesis narrative tone.
///
/// `Default::default()` is `{ intent: Understand, domains: vec![],
/// planned_action: Read, knowledge_gaps: vec![] }` — the safe
/// "no-signal" baseline that the rest of UCIL treats identically to
/// a missing `reason` parameter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedReason {
    /// Classified user intent.
    pub intent: Intent,
    /// Domain tokens that match the canonical UCIL vocabulary,
    /// preserving first-occurrence order with no duplicates.
    pub domains: Vec<String>,
    /// Coarse planned-action classification.
    pub planned_action: PlannedAction,
    /// Extracted "I don't know X" / "unsure about X" / "need to
    /// learn X" / "unfamiliar with X" phrases — verbatim
    /// (lowercased), trimmed, no duplicates, first-occurrence order.
    pub knowledge_gaps: Vec<String>,
}

// ── Static keyword tables ─────────────────────────────────────────────────────

/// Refactor-intent keyword patterns.  Highest precedence — see
/// [`parse_reason`] for the ladder.
const REFACTOR_PATTERNS: &[&str] = &["refactor", "clean up", "rename", "restructure"];

/// Bug-fix-intent keyword patterns.  Second-highest precedence.
const FIXBUG_PATTERNS: &[&str] = &["fix", "bug", "broken", "crash", "error"];

/// Add-feature-intent keyword patterns.  Third precedence.  Includes
/// the article-bearing variant `"add a feature"` because natural
/// English commonly inserts an article between the verb and the noun
/// (`I want to add a feature ...`); a bare `"add"` rule would be too
/// permissive and clash with the Edit-action pattern.
const ADDFEATURE_PATTERNS: &[&str] = &["add a feature", "add feature", "new feature", "implement"];

/// Review-intent keyword patterns.  Fourth precedence.
const REVIEW_PATTERNS: &[&str] = &["review", "audit", "diff", " pr ", "pull request"];

/// Understand-intent keyword patterns.  Lowest precedence; matches
/// trigger an explicit `Understand` return rather than a default.
const UNDERSTAND_PATTERNS: &[&str] = &["explain", "understand", "how does", "what is", "why"];

/// Edit-action keyword patterns.  Highest precedence in the action
/// ladder.
const EDIT_PATTERNS: &[&str] = &["edit", "modif", "update", "add", "create", "write"];

/// Read-action keyword patterns.  Second precedence.
const READ_PATTERNS: &[&str] = &["read", "look", "check", "inspect", "see"];

/// Explain-action keyword patterns.  Third precedence.
const EXPLAIN_PATTERNS: &[&str] = &["explain", "describ", "summar", "document"];

/// Canonical UCIL domain vocabulary — extensible via an `ADR`.
/// Tokens are matched as exact lowercased strings after splitting
/// the input on whitespace + the punctuation set
/// `, . ; : ! ? ( )`.
const DOMAIN_VOCABULARY: &[&str] = &[
    "rust",
    "python",
    "typescript",
    "async",
    "tokio",
    "http",
    "sql",
    "sqlite",
    "lancedb",
    "mcp",
    "lsp",
    "serena",
    "embeddings",
    "cargo",
    "git",
    "docker",
    "plugin",
    "daemon",
    "fusion",
    "ranking",
    "search",
    "vector",
    "knowledge",
    "agent",
    "ceqp",
];

/// Knowledge-gap trigger phrases — the parser scans for these in the
/// lower-cased input and extracts the trailing noun phrase.
const GAP_TRIGGERS: &[&str] = &[
    "don't know ",
    "unsure about ",
    "need to learn ",
    "unfamiliar with ",
];

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns `true` if any pattern in `patterns` is a substring of
/// `lowered`.  All patterns MUST already be lowercase.
fn any_matches(lowered: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|p| lowered.contains(p))
}

/// Classify intent over a pre-padded, lower-cased input.
///
/// Precedence ladder per master-plan §8.3 + `scope_in` #17 of WO-0067:
/// `Refactor > FixBug > AddFeature > Review > Understand`.  The
/// default when no scan matches is [`Intent::Understand`].
fn classify_intent(padded: &str) -> Intent {
    if any_matches(padded, REFACTOR_PATTERNS) {
        return Intent::Refactor;
    }
    if any_matches(padded, FIXBUG_PATTERNS) {
        return Intent::FixBug;
    }
    if any_matches(padded, ADDFEATURE_PATTERNS) {
        return Intent::AddFeature;
    }
    if any_matches(padded, REVIEW_PATTERNS) {
        return Intent::Review;
    }
    if any_matches(padded, UNDERSTAND_PATTERNS) {
        return Intent::Understand;
    }
    Intent::Understand
}

/// Classify planned-action over a pre-padded, lower-cased input.
///
/// Precedence ladder per `scope_in` #19 of WO-0067:
/// `Edit > Read > Explain`.  The default when no scan matches is
/// [`PlannedAction::Read`].
fn classify_action(padded: &str) -> PlannedAction {
    if any_matches(padded, EDIT_PATTERNS) {
        return PlannedAction::Edit;
    }
    if any_matches(padded, READ_PATTERNS) {
        return PlannedAction::Read;
    }
    if any_matches(padded, EXPLAIN_PATTERNS) {
        return PlannedAction::Explain;
    }
    PlannedAction::Read
}

/// Split `lowered` on whitespace + the punctuation set
/// `, . ; : ! ? ( )` and collect tokens that match
/// [`DOMAIN_VOCABULARY`], preserving first-occurrence order.
fn extract_domains(lowered: &str) -> Vec<String> {
    let mut domains: Vec<String> = Vec::new();
    let mut current = String::new();
    for c in lowered.chars() {
        if c.is_whitespace() || matches!(c, ',' | '.' | ';' | ':' | '!' | '?' | '(' | ')') {
            if !current.is_empty() {
                let token = std::mem::take(&mut current);
                if DOMAIN_VOCABULARY.contains(&token.as_str())
                    && !domains.iter().any(|d| d == &token)
                {
                    domains.push(token);
                }
            }
        } else {
            current.push(c);
        }
    }
    if !current.is_empty()
        && DOMAIN_VOCABULARY.contains(&current.as_str())
        && !domains.iter().any(|d| d == &current)
    {
        domains.push(current);
    }
    domains
}

/// Scan `lowered` for the [`GAP_TRIGGERS`] phrases and extract the
/// trailing noun phrase per `scope_in` #20 of WO-0067.
///
/// Noun-phrase grammar: a non-greedy run of `[a-z0-9_\- ]` of length
/// 1..=40, terminated by a sentence-end byte (`. , ; : ! ? \n`) OR
/// end-of-input.  The phrase is trimmed and stored verbatim
/// (lowercased), with first-occurrence order preserved and
/// duplicates skipped.
fn extract_gaps(lowered: &str) -> Vec<String> {
    let bytes = lowered.as_bytes();
    let mut gaps: Vec<String> = Vec::new();
    let mut search_pos: usize = 0;
    while search_pos < lowered.len() {
        // Find earliest trigger from search_pos.
        let mut earliest: Option<(usize, usize)> = None;
        for trigger in GAP_TRIGGERS {
            if let Some(rel) = lowered[search_pos..].find(trigger) {
                let abs = search_pos + rel;
                let cand = (abs, trigger.len());
                match earliest {
                    None => earliest = Some(cand),
                    Some((cur_abs, _)) if abs < cur_abs => earliest = Some(cand),
                    _ => {}
                }
            }
        }
        let Some((start, trig_len)) = earliest else {
            break;
        };
        let phrase_start = start + trig_len;

        // Scan forward up to 40 ASCII bytes or until a sentence-end /
        // disallowed byte.  Multibyte UTF-8 bytes have the high bit
        // set and therefore fail the `allowed` check, terminating
        // the scan at the start of the multibyte sequence — keeping
        // every slice on a UTF-8-safe boundary.
        let max_end = phrase_start.saturating_add(40).min(bytes.len());
        let mut end = phrase_start;
        while end < max_end {
            let b = bytes[end];
            let allowed = matches!(
                b,
                b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b' '
            );
            if !allowed {
                break;
            }
            end += 1;
        }
        let phrase = lowered[phrase_start..end].trim().to_owned();
        if !phrase.is_empty() && !gaps.iter().any(|g| g == &phrase) {
            gaps.push(phrase);
        }
        // Advance past the consumed phrase to avoid re-matching the
        // same trigger on the next iteration.
        search_pos = if end > start + trig_len {
            end
        } else {
            start + trig_len
        };
    }
    gaps
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Parse a free-text CEQP `reason` string into a [`ParsedReason`].
///
/// Master-plan §8.3 lines 772-774 freeze this as the deterministic
/// fallback path that runs when the LLM `QueryInterpreter` agent
/// (`P3.5-W12-F02`) is unavailable.  The parser is pure: no IO, no
/// async, no logging.  It never panics and always returns a valid
/// [`ParsedReason`] (worst case the `Default` value).
///
/// Implementation steps:
///
/// 1. Lowercase the input ONCE and pad with leading + trailing
///    spaces so word-boundary patterns (`" pr "`) work even when
///    the trigger is at the start/end of the string.
/// 2. Extract `intent` via the precedence ladder
///    `Refactor > FixBug > AddFeature > Review > Understand`.
/// 3. Extract `planned_action` via the precedence ladder
///    `Edit > Read > Explain` (default `Read`).
/// 4. Extract `domains` by splitting on whitespace + the punctuation
///    set `, . ; : ! ? ( )` and matching tokens against
///    `DOMAIN_VOCABULARY` — first-occurrence order preserved.
/// 5. Extract `knowledge_gaps` by scanning for `"don't know "`,
///    `"unsure about "`, `"need to learn "`, `"unfamiliar with "`
///    and capturing the trailing noun phrase up to 40 chars or the
///    next sentence-end punctuation — first-occurrence order
///    preserved, duplicates dropped.
///
/// # Examples
///
/// ```
/// use ucil_core::ceqp::{parse_reason, Intent, PlannedAction};
///
/// let parsed = parse_reason("refactor the storage module in rust");
/// assert_eq!(parsed.intent, Intent::Refactor);
/// assert_eq!(parsed.planned_action, PlannedAction::Read);
/// assert_eq!(parsed.domains, vec!["rust".to_owned()]);
/// assert!(parsed.knowledge_gaps.is_empty());
/// ```
#[must_use]
pub fn parse_reason(reason: &str) -> ParsedReason {
    // Step 1: lowercase once, pad with spaces.
    let lowered: String = reason.chars().flat_map(char::to_lowercase).collect();
    let padded = format!(" {lowered} ");

    // Step 2: intent.
    let intent = classify_intent(&padded);

    // Step 3: action.
    let planned_action = classify_action(&padded);

    // Step 4: domains.
    let domains = extract_domains(&lowered);

    // Step 5: knowledge gaps.
    let knowledge_gaps = extract_gaps(&lowered);

    ParsedReason {
        intent,
        domains,
        planned_action,
        knowledge_gaps,
    }
}

// ── Frozen acceptance test — P3-W9-F02 CEQP reason parser ─────────────────────
//
// Per `DEC-0007`, the frozen selector lives at MODULE ROOT so the
// `cargo test -p ucil-core ceqp::test_reason_parser` selector
// resolves directly.  Sub-assertions inline-rustdoc-numbered SA1..SA8
// per the WO-0048 / WO-0066 convention.

#[cfg(test)]
#[test]
#[allow(clippy::too_many_lines)]
fn test_reason_parser() {
    // ── SA1: intent classification — happy paths ─────────────────────
    assert_eq!(
        parse_reason("I want to add a feature for HTTP retry").intent,
        Intent::AddFeature,
        "(SA1) AddFeature: 'add a feature' phrase must yield AddFeature"
    );
    assert_eq!(
        parse_reason("fix the bug in the parser").intent,
        Intent::FixBug,
        "(SA1) FixBug: 'fix' / 'bug' must yield FixBug"
    );
    assert_eq!(
        parse_reason("refactor the storage module").intent,
        Intent::Refactor,
        "(SA1) Refactor: 'refactor' must yield Refactor"
    );
    assert_eq!(
        parse_reason("please review my pull request").intent,
        Intent::Review,
        "(SA1) Review: 'review' / 'pull request' must yield Review"
    );
    assert_eq!(
        parse_reason("explain how the daemon starts up").intent,
        Intent::Understand,
        "(SA1) Understand: 'explain' / 'how' must yield Understand"
    );
    assert_eq!(
        parse_reason("hello").intent,
        Intent::Understand,
        "(SA1) default: 'hello' must yield Understand (default)"
    );

    // ── SA2: intent precedence Refactor > FixBug > AddFeature ────────
    //
    // 'refactor the buggy add-feature path' contains tokens for
    // Refactor, FixBug, and AddFeature simultaneously — Refactor
    // MUST win.  Load-bearing against M3 verifier mutation
    // (reorder so Understand matches first).
    assert_eq!(
        parse_reason("refactor the buggy add-feature path").intent,
        Intent::Refactor,
        "(SA2) precedence: Refactor MUST win over FixBug + AddFeature \
         in the same sentence"
    );

    // ── SA3: domain tags ─────────────────────────────────────────────
    let parsed = parse_reason("add a tokio HTTP retry helper in rust");
    assert_eq!(
        parsed.domains,
        vec!["tokio".to_owned(), "http".to_owned(), "rust".to_owned()],
        "(SA3) domains: must extract [tokio, http, rust] in first-occurrence order"
    );
    assert!(
        parse_reason("plain english sentence").domains.is_empty(),
        "(SA3) domains: 'plain english sentence' must yield empty domains \
         (no vocabulary matches)"
    );

    // ── SA4: planned_action ──────────────────────────────────────────
    assert_eq!(
        parse_reason("edit the daemon startup").planned_action,
        PlannedAction::Edit,
        "(SA4) action: 'edit' must yield Edit"
    );
    assert_eq!(
        parse_reason("explain the fusion engine").planned_action,
        PlannedAction::Explain,
        "(SA4) action: 'explain' alone (no edit/read keyword) must yield Explain"
    );
    assert_eq!(
        parse_reason("check the storage layout").planned_action,
        PlannedAction::Read,
        "(SA4) action: 'check' must yield Read"
    );
    assert_eq!(
        parse_reason("look at the lifecycle").planned_action,
        PlannedAction::Read,
        "(SA4) action: 'look' must yield Read"
    );

    // ── SA5: knowledge_gaps ──────────────────────────────────────────
    //
    // Trim contract: leading + trailing whitespace stripped; the
    // captured phrase is exactly the run of `[a-z0-9_\- ]` between
    // the trigger and the next sentence-end punctuation (or
    // end-of-input).
    let gaps =
        parse_reason("add backoff but I don't know exponential semantics yet.").knowledge_gaps;
    assert!(
        gaps.iter().any(|g| g.contains("exponential semantics yet")),
        "(SA5) gaps: 'don't know exponential semantics yet.' must \
         capture 'exponential semantics yet'; got {gaps:?}"
    );

    let gaps2 = parse_reason("unsure about the lancedb schema, will check").knowledge_gaps;
    assert!(
        gaps2.iter().any(|g| g.contains("the lancedb schema")),
        "(SA5) gaps: 'unsure about the lancedb schema,' must \
         capture 'the lancedb schema'; got {gaps2:?}"
    );

    assert!(
        parse_reason("clear sentence with no gaps")
            .knowledge_gaps
            .is_empty(),
        "(SA5) gaps: 'clear sentence with no gaps' (no trigger phrase) \
         must yield empty knowledge_gaps"
    );

    // ── SA6: JSON round-trip on ParsedReason ─────────────────────────
    let default = ParsedReason::default();
    let json = serde_json::to_string(&default).expect("serialize ParsedReason");
    let back: ParsedReason = serde_json::from_str(&json).expect("deserialize ParsedReason");
    assert_eq!(
        default, back,
        "(SA6) JSON round-trip on ParsedReason::default() must preserve \
         equality; serialised={json}"
    );

    // ── SA7: empty input ─────────────────────────────────────────────
    let parsed = parse_reason("");
    assert_eq!(
        parsed.intent,
        Intent::Understand,
        "(SA7) empty input: intent must default to Understand"
    );
    assert!(
        parsed.domains.is_empty(),
        "(SA7) empty input: domains must be empty"
    );
    assert_eq!(
        parsed.planned_action,
        PlannedAction::Read,
        "(SA7) empty input: planned_action must default to Read"
    );
    assert!(
        parsed.knowledge_gaps.is_empty(),
        "(SA7) empty input: knowledge_gaps must be empty"
    );

    // ── SA8: case insensitivity ──────────────────────────────────────
    assert_eq!(
        parse_reason("FIX the BUG").intent,
        Intent::FixBug,
        "(SA8) case-insensitivity: 'FIX the BUG' must classify as FixBug"
    );
    assert_eq!(
        parse_reason("REFACTOR the MODULE").intent,
        Intent::Refactor,
        "(SA8) case-insensitivity: uppercase 'REFACTOR' must classify as Refactor"
    );
}
