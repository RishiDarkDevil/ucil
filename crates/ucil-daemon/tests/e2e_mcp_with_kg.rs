//! End-to-end black-box test for the `ucil-daemon mcp --stdio --repo <PATH>`
//! subcommand.
//!
//! Complements [`e2e_mcp_stdio`] (WO-0040's stub-path regression guard)
//! by asserting the OPPOSITE invariant: when `--repo` is supplied, the
//! binary must bootstrap a real SQLite `KnowledgeGraph` by ingesting
//! the repo via `IngestPipeline`, hand it to `McpServer::with_knowledge_graph`,
//! and return real `file:line` data for `find_definition`.
//!
//! The fixture is `tests/fixtures/rust-project/` (in-workspace), reached
//! via `env!("CARGO_MANIFEST_DIR").parent().parent().join(...)`.  No
//! docker, no Serena, no external LSP.  The KG lives in a `TempDir`
//! owned by the child process for the duration of the handshake.
//!
//! Per DEC-0005, the test is a flat `#[test] fn` at module root — no
//! `mod tests { }` wrapper.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[test]
fn e2e_mcp_stdio_with_repo_returns_real_find_definition() {
    let daemon = env!("CARGO_BIN_EXE_ucil-daemon");

    let fixture: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate parent")
        .parent()
        .expect("workspace root")
        .join("tests/fixtures/rust-project");
    assert!(
        fixture.is_dir(),
        "fixture rust-project dir missing at {}",
        fixture.display()
    );

    let mut child = Command::new(daemon)
        .arg("mcp")
        .arg("--stdio")
        .arg("--repo")
        .arg(&fixture)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn ucil-daemon mcp --stdio --repo");

    let init_req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e-mcp-stdio-with-kg","version":"1.0.0"}}}"#;
    let list_req = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#;
    let call_req = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"find_definition","arguments":{"name":"evaluate"}}}"#;

    {
        let stdin = child.stdin.as_mut().expect("child stdin was not piped");
        for line in [init_req, list_req, call_req] {
            stdin.write_all(line.as_bytes()).expect("write request");
            stdin.write_all(b"\n").expect("write newline");
        }
    }
    // Drop stdin so the child's read loop sees EOF and the serve loop
    // resolves cleanly.
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

    let mut call_response: Option<serde_json::Value> = None;
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("invalid JSON line {line:?}: {e}"));
        if v.get("id").and_then(serde_json::Value::as_i64) == Some(3) {
            call_response = Some(v);
            break;
        }
    }
    let resp =
        call_response.expect("no response with id=3 (tools/call find_definition) in daemon stdout");

    let meta = resp
        .get("result")
        .and_then(|r| r.get("_meta"))
        .and_then(serde_json::Value::as_object)
        .expect("result._meta missing from find_definition response");

    // Real-data wiring assertions — these all fail on the stub path.
    assert_eq!(
        meta.get("tool").and_then(serde_json::Value::as_str),
        Some("find_definition"),
        "_meta.tool wrong: {meta:?}"
    );
    assert_eq!(
        meta.get("source").and_then(serde_json::Value::as_str),
        Some("tree-sitter+kg"),
        "_meta.source wrong (stub path taken?): {meta:?}"
    );
    assert_eq!(
        meta.get("found").and_then(serde_json::Value::as_bool),
        Some(true),
        "_meta.found wrong (evaluate not resolved): {meta:?}"
    );
    assert!(
        meta.get("not_yet_implemented").is_none(),
        "_meta.not_yet_implemented present — stub path was taken: {meta:?}"
    );

    let file_path = meta
        .get("file_path")
        .and_then(serde_json::Value::as_str)
        .expect("_meta.file_path missing");
    assert!(
        file_path.ends_with("src/util.rs"),
        "_meta.file_path does not end with src/util.rs: {file_path}"
    );

    let start_line = meta
        .get("start_line")
        .and_then(serde_json::Value::as_i64)
        .expect("_meta.start_line missing or not integer");
    assert!(
        start_line > 0,
        "_meta.start_line must be a positive integer, got {start_line}"
    );
}
