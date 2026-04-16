//! Mock MCP plugin binary used by the `ucil-daemon` plugin-manager
//! integration tests.
//!
//! This binary speaks a tiny subset of JSON-RPC 2.0 over stdio:
//!
//! * Reads one newline-terminated JSON-RPC request from stdin.
//! * If the `method` field is `"tools/list"`, writes a newline-terminated
//!   JSON-RPC 2.0 response whose `result.tools` array contains two static
//!   entries (`echo` and `reverse`).
//! * If the method is anything else, writes a JSON-RPC error response
//!   with code `-32601` (Method not found).
//! * Parse errors on the request produce a JSON-RPC error with code
//!   `-32700` (Parse error).
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

    let mut line = String::new();
    let n = stdin.read_line(&mut line)?;
    if n == 0 {
        // EOF before any request — nothing to reply to.
        return Ok(());
    }

    let response = build_response(line.trim_end());
    let encoded = serde_json::to_string(&response)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    stdout.write_all(encoded.as_bytes())?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    Ok(())
}

/// Build the JSON-RPC 2.0 response to a single request line.
///
/// Pure function: no I/O, no side effects.  Separated from `main` so
/// that a focused unit test could exercise it directly if needed.
fn build_response(request_line: &str) -> Value {
    let request: Value = match serde_json::from_str(request_line) {
        Ok(v) => v,
        Err(_) => {
            return json!({
                "jsonrpc": "2.0",
                "id": Value::Null,
                "error": { "code": -32700, "message": "Parse error" }
            });
        }
    };

    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");

    if method == "tools/list" {
        json!({
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
        })
    } else {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": "Method not found" }
        })
    }
}
