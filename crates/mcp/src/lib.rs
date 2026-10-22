//! A Model Context Protocol server over stdio (L5): JSON-RPC 2.0, one message per line. It
//! exposes the engine's commands to agents as tools: `list_commands`, `execute`, `describe_map`.
//!
//! Whatever a client sends, the server keeps going: bytes that aren't UTF-8 JSON get a parse
//! error, a message that isn't a request an invalid-request error, a client's responses no
//! reply at all (JSON-RPC forbids answering them), and a line over [`MAX_LINE`] is refused
//! without being held whole (#60).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::io::{BufRead, Read, Write};

use mkaegis_engine::Session;
use serde_json::{Value as Json, json};

const PROTOCOL: &str = "2025-06-18";
/// The longest message line read, in bytes.
pub const MAX_LINE: usize = 16 << 20;
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

fn tools() -> Json {
    json!([
        {
            "name": "list_commands",
            "description": "Every MK aeGIS command: id, label, ribbon location and parameters.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "execute",
            "description": "Run a MK aeGIS command (as the ribbon would) with JSON parameters. Examples: map.addSample {}, map.selectByAttributes {layer, where}, analysis.buffer {layer, distance, unit}, management.project {layer, epsg}, share.exportMap {path}.",
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

/// A tool's result, or `None` for a tool that doesn't exist.
fn call(session: &mut Session, name: &str, args: &Json) -> Option<Json> {
    Some(match name {
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
        _ => return None,
    })
}

/// Answers one JSON-RPC message; `None` for notifications and for a client's responses.
pub fn handle(session: &mut Session, msg: &Json) -> Option<Json> {
    let Some(obj) = msg.as_object() else { return Some(error(Json::Null, INVALID_REQUEST, "a message must be a JSON object")) };
    let method = obj.get("method").and_then(Json::as_str);
    let Some(id) = obj.get("id").cloned() else {
        // A notification (or nothing a client may send without an id): never answered.
        return None;
    };
    let Some(method) = method else {
        // A response to a request of ours (we send none): JSON-RPC forbids replying to it.
        if obj.contains_key("result") || obj.contains_key("error") {
            return None;
        }
        return Some(error(id, INVALID_REQUEST, "a request needs a `method`"));
    };
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
            match call(session, &name, &args) {
                Some(r) => r,
                None => return Some(error(id, INVALID_PARAMS, &format!("unknown tool `{name}`"))),
            }
        }
        other => return Some(error(id, METHOD_NOT_FOUND, &format!("method not found: {other}"))),
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

/// Serves MCP on stdin/stdout until stdin closes.
pub fn serve(session: Session) -> std::io::Result<()> {
    serve_on(session, std::io::stdin().lock(), std::io::stdout().lock())
}

/// Serves MCP from `input` to `output` until `input` ends.
pub fn serve_on(mut session: Session, mut input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = input.by_ref().take(MAX_LINE as u64 + 1).read_until(b'\n', &mut line)?;
        if read == 0 {
            return Ok(());
        }
        let reply = if line.len() > MAX_LINE && line.last() != Some(&b'\n') {
            skip_line(&mut input)?;
            Some(error(Json::Null, PARSE_ERROR, &format!("a message over {} MB", MAX_LINE >> 20)))
        } else {
            match std::str::from_utf8(&line) {
                Ok(text) if text.trim().is_empty() => None,
                Ok(text) => match serde_json::from_str::<Json>(text) {
                    Ok(msg) => handle(&mut session, &msg),
                    Err(e) => Some(error(Json::Null, PARSE_ERROR, &e.to_string())),
                },
                Err(_) => Some(error(Json::Null, PARSE_ERROR, "the message is not UTF-8")),
            }
        };
        if let Some(r) = reply {
            writeln!(output, "{r}")?;
            output.flush()?;
        }
    }
}

/// Reads past the rest of a line too long to keep, a buffer at a time.
fn skip_line(input: &mut impl BufRead) -> std::io::Result<()> {
    loop {
        let buf = input.fill_buf()?;
        if buf.is_empty() {
            return Ok(());
        }
        match buf.iter().position(|b| *b == b'\n') {
            Some(i) => {
                input.consume(i + 1);
                return Ok(());
            }
            None => {
                let n = buf.len();
                input.consume(n);
            }
        }
    }
}

fn error(id: Json, code: i64, message: &str) -> Json {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
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

    /// #60: each message, and the reply it gets (`None`: no reply).
    #[test]
    fn every_kind_of_message_gets_the_reply_json_rpc_asks_for() {
        let mut s = Session::default();
        let code = |r: Option<Json>| r.map(|r| (r["id"].clone(), r["error"]["code"].clone()));
        let cases: [(Json, Option<(Json, Json)>); 7] = [
            (json!(5), Some((Json::Null, json!(INVALID_REQUEST)))),
            (json!("x"), Some((Json::Null, json!(INVALID_REQUEST)))),
            (json!([{ "jsonrpc": "2.0", "id": 1, "method": "ping" }]), Some((Json::Null, json!(INVALID_REQUEST)))),
            (json!({ "jsonrpc": "2.0", "id": 7, "result": {} }), None),
            (json!({ "jsonrpc": "2.0", "id": 8, "error": { "code": 1, "message": "x" } }), None),
            (json!({ "jsonrpc": "2.0", "id": 9 }), Some((json!(9), json!(INVALID_REQUEST)))),
            (json!({ "jsonrpc": "2.0", "id": 10, "method": "tools/call", "params": { "name": "nope" } }), Some((json!(10), json!(INVALID_PARAMS)))),
        ];
        for (msg, want) in cases {
            assert_eq!(code(handle(&mut s, &msg)), want, "{msg}");
        }
    }

    #[test]
    fn the_server_keeps_going_past_bad_bytes_and_long_lines() {
        let mut input = Vec::new();
        input.extend_from_slice(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n");
        input.extend_from_slice(b"\xff\xfe not utf-8\n");
        input.extend_from_slice(b"{not json\n");
        input.extend(std::iter::repeat_n(b' ', MAX_LINE + 10));
        input.extend_from_slice(b"\n\n");
        input.extend_from_slice(b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}");
        let mut output = Vec::new();
        serve_on(Session::default(), input.as_slice(), &mut output).unwrap();
        let replies: Vec<Json> = String::from_utf8(output).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        let summary: Vec<(Json, Json)> = replies.iter().map(|r| (r["id"].clone(), r["error"]["code"].clone())).collect();
        assert_eq!(
            summary,
            [
                (json!(1), Json::Null),
                (Json::Null, json!(PARSE_ERROR)),
                (Json::Null, json!(PARSE_ERROR)),
                (Json::Null, json!(PARSE_ERROR)),
                (json!(2), Json::Null),
            ]
        );
        assert!(replies[3]["error"]["message"].as_str().unwrap().contains("over 16 MB"));
    }
}
