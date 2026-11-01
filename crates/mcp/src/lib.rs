//! The MCP server (L5): every ChatCraft command is a tool.
//!
//! Speaks JSON-RPC 2.0 over stdio, one message per line (`initialize`, `tools/list`,
//! `tools/call`, `ping`). Tool names are command ids with `.` replaced by `_`
//! (`message_send`), because MCP tool names may not contain dots; `list_commands` describes
//! them all. The caller builds the [`Session`] (headless demo, or connected to a server), so the
//! same tools work against either. See `docs/mcp.md`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::io::{BufRead, Write};
use std::time::Duration;

use chatcraft_engine::Session;
use serde_json::{Value, json};

const PROTOCOL: &str = "2025-06-18";

fn tool_name(id: &str) -> String {
    id.replace('.', "_")
}

fn command_id(tool: &str) -> Option<&'static str> {
    Session::registry().all().iter().find(|s| tool_name(s.id) == tool || s.id == tool).map(|s| s.id)
}

fn tools() -> Value {
    let mut list: Vec<Value> = Session::registry()
        .all()
        .iter()
        .map(|s| {
            json!({
                "name": tool_name(s.id),
                "title": s.label,
                "description": format!("{} ({}). Params: {}{}", s.label, s.location, s.params, if s.mutates { "" } else { " Read-only." }),
                "inputSchema": { "type": "object", "additionalProperties": true },
                "annotations": { "readOnlyHint": !s.mutates },
            })
        })
        .collect();
    list.push(json!({
        "name": "list_commands",
        "description": "Every ChatCraft command with its id, label, location, shortcut and params.",
        "inputSchema": { "type": "object", "additionalProperties": false },
        "annotations": { "readOnlyHint": true },
    }));
    json!({ "tools": list })
}

fn call(session: &mut Session, params: &Value) -> Value {
    let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
    let args = params.get("arguments").cloned().unwrap_or(Value::Null);
    let result = if name == "list_commands" {
        Ok(Session::registry().describe())
    } else {
        match command_id(name) {
            None => Err(format!("unknown tool `{name}`")),
            Some(id) => {
                session.pump();
                match session.run(id, &args) {
                    // Against a server the answer arrives asynchronously.
                    Ok(v) if v.get("pending").is_some() => {
                        let req = v.get("pending").and_then(Value::as_u64).unwrap_or(0);
                        match session.wait_for(req, Duration::from_secs(15)) {
                            Some(r) => r.map_err(|e| e.to_string()),
                            None => Err("the server did not answer within 15 s".into()),
                        }
                    }
                    other => other.map_err(|e| e.to_string()),
                }
            }
        }
    };
    match result {
        Ok(v) => {
            json!({ "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default() }], "structuredContent": { "result": v }, "isError": false })
        }
        Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
    }
}

/// Handles one JSON-RPC line; returns the response line, if any (notifications get none).
pub fn handle(session: &mut Session, line: &str) -> Option<String> {
    let msg: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return Some(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("parse error: {e}")}}).to_string()),
    };
    let id = msg.get("id").cloned()?;
    let method = msg.get("method").and_then(Value::as_str).unwrap_or_default();
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL),
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": "chatcraft", "version": env!("CARGO_PKG_VERSION") },
            "instructions": "ChatCraft: a team chat workspace. Every tool is a ChatCraft command. Conversations are named \"#channel\" or \"@handle\" (a direct message); messages by id (12 or \"M12\"). Start with session_state, conversations_list and message_list.",
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(tools()),
        "tools/call" => Ok(call(session, &params)),
        other => Err(json!({"code": -32601, "message": format!("method `{other}` not found")})),
    };
    Some(
        match result {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": e}),
        }
        .to_string(),
    )
}

/// Serves MCP on stdin/stdout until stdin closes.
pub fn serve_stdio(mut session: Session) -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(resp) = handle(&mut session, &line) {
            writeln!(out, "{resp}")?;
            out.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rpc(s: &mut Session, method: &str, params: Value) -> Value {
        let line = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).to_string();
        serde_json::from_str(&handle(s, &line).unwrap()).unwrap()
    }

    #[test]
    fn lists_and_calls_tools() {
        let mut s = Session::demo();
        let init = rpc(&mut s, "initialize", json!({"protocolVersion": "2025-03-26"}));
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        let tools = rpc(&mut s, "tools/list", json!({}));
        let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
        assert!(names.contains(&"message_send") && names.contains(&"list_commands"));
        assert!(names.iter().all(|n| !n.contains('.')));
        let r = rpc(&mut s, "tools/call", json!({"name": "message_send", "arguments": {"channel": "#random", "text": "from an agent"}}));
        assert_eq!(r["result"]["isError"], false);
        let r = rpc(&mut s, "tools/call", json!({"name": "message_edit", "arguments": {}}));
        assert_eq!(r["result"]["isError"], true);
        assert!(handle(&mut s, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
        assert!(rpc(&mut s, "nope", json!({}))["error"]["code"] == -32601);
        assert!(handle(&mut s, "{not json").unwrap().contains("-32700"));
    }
}
