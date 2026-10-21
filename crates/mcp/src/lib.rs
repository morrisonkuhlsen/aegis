//! A Model Context Protocol server over stdio (L5): JSON-RPC 2.0, one message per line. It
//! exposes the engine's commands to agents as tools: `list_commands`, `execute`, `describe_map`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::io::{BufRead, Write};

use mkaegis_engine::Session;
use serde_json::{Value as Json, json};

const PROTOCOL: &str = "2025-06-18";

fn tools() -> Json {
    json!([
        {
            "name": "list_commands",
            "description": "Every mkaeGIS command: id, label, ribbon location and parameters.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "execute",
            "description": "Run a mkaeGIS command (as the ribbon would) with JSON parameters. Examples: map.addSample {}, map.selectByAttributes {layer, where}, analysis.buffer {layer, distance, unit}, management.project {layer, epsg}, share.exportMap {path}.",
            "inputSchema": {
                "type": "object",
                "properties": { "command": { "type": "string" }, "params": { "type": "object" } },
                "required": ["command"]
            }
        },
        {
            "name": "describe_map",
            "description": "The open map: CRS, extent and layers with their fields and counts.",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}

fn text(content: String, error: bool) -> Json {
    json!({ "content": [{ "type": "text", "text": content }], "isError": error })
}

fn call(session: &mut Session, name: &str, args: &Json) -> Json {
    match name {
        "list_commands" => {
            let list: Vec<Json> =
                Session::commands().iter().map(|c| json!({ "id": c.id, "label": c.label, "ribbon": c.ribbon, "params": c.params })).collect();
            text(Json::Array(list).to_string(), false)
        }
        "execute" => {
            let command = args.get("command").and_then(Json::as_str).unwrap_or_default();
            let params = args.get("params").cloned().unwrap_or(json!({}));
            match session.execute(command, &params) {
                Ok(v) => text(v.to_string(), false),
                Err(e) => text(e.to_string(), true),
            }
        }
        "describe_map" => match session.execute("map.describe", &json!({})) {
            Ok(v) => text(v.to_string(), false),
            Err(e) => text(e.to_string(), true),
        },
        other => text(format!("unknown tool `{other}`"), true),
    }
}

/// Answers one JSON-RPC message; `None` for notifications.
pub fn handle(session: &mut Session, msg: &Json) -> Option<Json> {
    let id = msg.get("id").cloned()?;
    let method = msg.get("method").and_then(Json::as_str).unwrap_or_default();
    let result = match method {
        "initialize" => json!({
            "protocolVersion": PROTOCOL,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "mkaegis", "version": env!("CARGO_PKG_VERSION") }
        }),
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tools() }),
        "tools/call" => {
            let params = msg.get("params").cloned().unwrap_or(json!({}));
            let name = params.get("name").and_then(Json::as_str).unwrap_or_default().to_string();
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            call(session, &name, &args)
        }
        other => return Some(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("method not found: {other}") } })),
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

/// Serves MCP on stdin/stdout until stdin closes.
pub fn serve(mut session: Session) -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Json>(&line) {
            Ok(msg) => handle(&mut session, &msg),
            Err(e) => Some(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": e.to_string() } })),
        };
        if let Some(r) = reply {
            writeln!(out, "{r}")?;
            out.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_list_and_call() {
        let mut s = Session::default();
        let init = handle(&mut s, &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} })).unwrap();
        assert_eq!(init["result"]["serverInfo"]["name"], "mkaegis");
        assert!(handle(&mut s, &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).is_none());
        let list = handle(&mut s, &json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" })).unwrap();
        assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 3);
        let r = handle(&mut s, &json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "execute", "arguments": { "command": "map.addSample" } } })).unwrap();
        assert_eq!(r["result"]["isError"], false);
        let r = handle(&mut s, &json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": { "name": "describe_map" } })).unwrap();
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("Cidades"));
        let bad = handle(&mut s, &json!({ "jsonrpc": "2.0", "id": 5, "method": "nope" })).unwrap();
        assert_eq!(bad["error"]["code"], -32601);
    }
}
