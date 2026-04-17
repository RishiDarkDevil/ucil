//! Mock MCP plugin binary used by the `ucil-daemon` plugin-manager
//! integration tests.
//!
//! This binary speaks a tiny subset of JSON-RPC 2.0 over stdio, mirroring
//! the handshake sequence a real MCP server must accept:
//!
//! * Reads newline-terminated JSON-RPC frames from stdin in a loop,
//!   exiting cleanly on EOF.
//! * Requests carrying an `id` field receive a response frame; bare
//!   notifications (no `id`) are processed silently — per the JSON-RPC 2.0
//!   spec §4.1.
//! * `tools/list` is answered with a `result.tools` array of two static
//!   entries (`echo` and `reverse`).
//! * `initialize` is answered with a minimal MCP-shaped `result`
//!   (`protocolVersion`, empty `capabilities`, `serverInfo`) so the plugin
//!   manager's handshake can succeed.  The mock's capability surface is
//!   intentionally trivial — it exists to exercise the UCIL plumbing, not
//!   the MCP negotiation logic.
//! * Any other method (e.g. `resources/list`) returns a JSON-RPC error
//!   with code `-32601` (Method not found).
//! * Parse errors on a request produce a JSON-RPC error with code
//!   `-32700` (Parse error) and the loop continues.
//!
//! The binary is intentionally synchronous and uses `std::io` (not `tokio`)
//! so that it has no transitive dependencies on the daemon's async runtime
//! — the test harness spawns it as a subprocess.

use std::io::{self, BufRead, Write};

use serde_json::{json, Value};

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdin = stdin.lock();
    let mut stdout = stdout.lock();

    loop {
        let mut line = String::new();
        let n = stdin.read_line(&mut line)?;
        if n == 0 {
            // EOF — client closed its end of the pipe.  Nothing left to
            // respond to, so exit cleanly.
            return Ok(());
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(response) = build_response(trimmed) {
            let encoded = serde_json::to_string(&response)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            stdout.write_all(encoded.as_bytes())?;
            stdout.write_all(b"\n")?;
            stdout.flush()?;
        }
    }
}

/// Build the JSON-RPC 2.0 response to a single request line.
///
/// Returns `None` when the request is a notification (no `id` field) —
/// per JSON-RPC §4.1 a notification MUST NOT receive a response.  Pure
/// function: no I/O, no side effects.
fn build_response(request_line: &str) -> Option<Value> {
    let request: Value = match serde_json::from_str(request_line) {
        Ok(v) => v,
        Err(_) => {
            return Some(json!({
                "jsonrpc": "2.0",
                "id": Value::Null,
                "error": { "code": -32700, "message": "Parse error" }
            }));
        }
    };

    // Notifications (no `id` field) receive no response per the spec.
    let raw_id = request.get("id").cloned();
    let is_notification = raw_id.is_none();
    if is_notification {
        return None;
    }

    let id = raw_id.unwrap_or(Value::Null);
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");

    match method {
        "initialize" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": "2025-06-18",
                "capabilities": { "tools": {} },
                "serverInfo": {
                    "name": "mock-mcp-plugin",
                    "version": "0.1.0"
                }
            }
        })),
        "tools/list" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "tools": [
                    {
                        "name": "echo",
                        "description": "Echo back the given text.",
                        "inputSchema": { "type": "object" }
                    },
                    {
                        "name": "reverse",
                        "description": "Return the input text reversed.",
                        "inputSchema": { "type": "object" }
                    }
                ]
            }
        })),
        _ => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": "Method not found" }
        })),
    }
}
