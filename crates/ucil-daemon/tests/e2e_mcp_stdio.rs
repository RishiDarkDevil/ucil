//! End-to-end black-box test for the `ucil-daemon mcp --stdio` subcommand.
//!
//! This test spawns the compiled `ucil-daemon` binary (via the
//! cargo-injected `CARGO_BIN_EXE_ucil-daemon` absolute path), replays the
//! same `initialize` + `tools/list` JSON-RPC 2.0 handshake that
//! `scripts/verify/e2e-mcp-smoke.sh` performs, and asserts the result:
//!
//! * `tools/list` returns exactly 22 descriptors.
//! * Every frozen tool name from master-plan §3.2 is present.
//! * Every descriptor's `inputSchema.properties` carries the four CEQP
//!   universal parameters (`reason`, `current_task`, `files_in_context`,
//!   `token_budget`) per master-plan §3.1.
//!
//! It does not start docker, does not rely on Serena, and does not
//! populate a knowledge graph — `McpServer::new()` is the stub path
//! exercised here, matching phase-1 invariant #9.

use std::io::Write;
use std::process::{Command, Stdio};

const FROZEN_TOOLS: &[&str] = &[
    "understand_code",
    "find_definition",
    "find_references",
    "search_code",
    "find_similar",
    "get_context_for_edit",
    "get_conventions",
    "get_architecture",
    "trace_dependencies",
    "blast_radius",
    "explain_history",
    "remember",
    "review_changes",
    "check_quality",
    "run_tests",
    "security_scan",
    "lint_code",
    "type_check",
    "refactor",
    "generate_docs",
    "query_database",
    "check_runtime",
];

const CEQP_KEYS: &[&str] = &["reason", "current_task", "files_in_context", "token_budget"];

#[test]
fn e2e_mcp_stdio_handshake_returns_22_tools_with_ceqp() {
    let daemon = env!("CARGO_BIN_EXE_ucil-daemon");

    let mut child = Command::new(daemon)
        .arg("mcp")
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn ucil-daemon mcp --stdio");

    let init_req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e-mcp-stdio","version":"1.0.0"}}}"#;
    let list_req = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#;

    {
        let stdin = child.stdin.as_mut().expect("child stdin was not piped");
        stdin
            .write_all(init_req.as_bytes())
            .expect("write initialize request");
        stdin.write_all(b"\n").expect("write newline after init");
        stdin
            .write_all(list_req.as_bytes())
            .expect("write tools/list request");
        stdin.write_all(b"\n").expect("write newline after list");
    }
    // Drop stdin so the child's read loop sees EOF and the serve loop
    // resolves cleanly — matches the smoke-script flow.
    drop(child.stdin.take());

    let output = child
        .wait_with_output()
        .expect("daemon did not terminate cleanly");
    assert!(
        output.status.success(),
        "daemon exited non-zero: {:?}",
        output.status
    );

    let stdout = String::from_utf8(output.stdout).expect("daemon stdout was not valid UTF-8");
    assert!(
        !stdout.is_empty(),
        "daemon produced no stdout responses — main.rs dispatch broken"
    );

    let mut list_response: Option<serde_json::Value> = None;
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("invalid JSON line {line:?}: {e}"));
        if v.get("id").and_then(serde_json::Value::as_i64) == Some(2) {
            list_response = Some(v);
            break;
        }
    }
    let list = list_response.expect("no response with id=2 (tools/list) in daemon stdout");

    let tools = list
        .get("result")
        .and_then(|r| r.get("tools"))
        .and_then(serde_json::Value::as_array)
        .expect("tools/list response missing result.tools array");

    assert_eq!(
        tools.len(),
        22,
        "expected 22 tools in tools/list response, got {}",
        tools.len()
    );

    let names: Vec<&str> = tools
        .iter()
        .filter_map(|t| t.get("name").and_then(serde_json::Value::as_str))
        .collect();
    for frozen in FROZEN_TOOLS {
        assert!(
            names.contains(frozen),
            "frozen tool {frozen:?} missing from tools/list response (got: {names:?})"
        );
    }

    for tool in tools {
        let name = tool
            .get("name")
            .and_then(serde_json::Value::as_str)
            .expect("tool descriptor missing name");
        let props = tool
            .get("inputSchema")
            .and_then(|s| s.get("properties"))
            .and_then(serde_json::Value::as_object)
            .unwrap_or_else(|| panic!("tool {name:?} missing inputSchema.properties"));
        for ceqp in CEQP_KEYS {
            assert!(
                props.contains_key(*ceqp),
                "tool {name:?} missing CEQP param {ceqp:?} on inputSchema.properties"
            );
        }
    }
}
