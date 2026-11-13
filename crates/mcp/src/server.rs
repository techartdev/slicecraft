//! The MCP transport: JSON-RPC 2.0 messages, one per line, over a reader and
//! a writer (standard input and output for `slicecraft-cli mcp`).
//!
//! Handles `initialize`, `ping`, `tools/list` and `tools/call`; notifications
//! get no answer. Every tool call goes through [`crate::dispatch`], so the
//! agent reaches exactly the engine commands the window uses.

use crate::json::{self, Value};
use crate::{Call, Reply, Session, TOOLS, ToolSpec, dispatch};
use std::io::{BufRead, Write};

/// Longest request line read.
pub const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;
/// The protocol revision answered when the client names none we know.
pub const PROTOCOL_VERSION: &str = "2025-06-18";
const KNOWN_VERSIONS: [&str; 3] = ["2024-11-05", "2025-03-26", "2025-06-18"];

fn error(id: Value, code: i64, message: &str) -> Value {
    Value::obj(vec![
        ("jsonrpc", Value::str("2.0")),
        ("id", id),
        (
            "error",
            Value::obj(vec![("code", Value::Num(code as f64)), ("message", Value::str(message))]),
        ),
    ])
}

fn result(id: Value, result: Value) -> Value {
    Value::obj(vec![("jsonrpc", Value::str("2.0")), ("id", id), ("result", result)])
}

/// The JSON schema of a tool's arguments.
pub fn input_schema(spec: &ToolSpec) -> Value {
    let mut props = Vec::new();
    let mut required = Vec::new();
    for a in spec.args {
        let (a, req) = match a.strip_suffix('!') {
            Some(a) => (a, true),
            None => (*a, false),
        };
        let (name, kind) = a.split_once(':').unwrap_or((a, "string"));
        props.push((name.to_string(), Value::obj(vec![("type", Value::str(kind))])));
        if req {
            required.push(Value::str(name));
        }
    }
    let mut schema = vec![("type", Value::str("object")), ("properties", Value::Obj(props))];
    if !required.is_empty() {
        schema.push(("required", Value::Arr(required)));
    }
    Value::obj(schema)
}

fn tool_list() -> Value {
    let tools = TOOLS
        .iter()
        .map(|t| {
            Value::obj(vec![
                ("name", Value::str(t.name)),
                ("description", Value::str(t.description)),
                ("inputSchema", input_schema(t)),
            ])
        })
        .collect();
    Value::obj(vec![("tools", Value::Arr(tools))])
}

/// Answer one message; `None` for notifications and responses.
pub fn handle(session: &mut Session, line: &str) -> Option<Value> {
    let msg = match json::parse(line) {
        Ok(m) => m,
        Err(e) => return Some(error(Value::Null, -32700, &format!("parse error: {e}"))),
    };
    if let Value::Arr(_) = msg {
        return Some(error(Value::Null, -32600, "batches are not supported"));
    }
    let method = msg.get("method").and_then(Value::as_str);
    let id = msg.get("id").cloned();
    let (Some(method), Some(id)) = (method, id) else {
        // a notification (no id) or a response to us: nothing to say
        return if method.is_none()
            && msg.get("id").is_some()
            && msg.get("result").is_none()
            && msg.get("error").is_none()
        {
            Some(error(
                msg.get("id").cloned().unwrap_or(Value::Null),
                -32600,
                "a request needs a method",
            ))
        } else {
            None
        };
    };
    let params = msg.get("params");
    Some(match method {
        "initialize" => {
            let asked = params.and_then(|p| p.get("protocolVersion")).and_then(Value::as_str);
            let version = asked.filter(|v| KNOWN_VERSIONS.contains(v)).unwrap_or(PROTOCOL_VERSION);
            result(
                id,
                Value::obj(vec![
                    ("protocolVersion", Value::str(version)),
                    ("capabilities", Value::obj(vec![("tools", Value::obj(vec![]))])),
                    (
                        "serverInfo",
                        Value::obj(vec![
                            ("name", Value::str("slicecraft")),
                            ("version", Value::str(env!("CARGO_PKG_VERSION"))),
                        ]),
                    ),
                    (
                        "instructions",
                        Value::str(
                            "SliceCraft slices 3D models for FFF printers. Start with slicecraft.describe; \
                             import models by path, change settings with set_setting (keys from list_settings), \
                             slice, then export_gcode. Models are numbered by index.",
                        ),
                    ),
                ]),
            )
        }
        "ping" => result(id, Value::obj(vec![])),
        "tools/list" => result(id, tool_list()),
        "tools/call" => {
            let Some(name) = params.and_then(|p| p.get("name")).and_then(Value::as_str) else {
                return Some(error(id, -32602, "tools/call needs a tool name"));
            };
            let mut call = Call::new(name);
            if let Some(Value::Obj(members)) = params.and_then(|p| p.get("arguments")) {
                for (k, v) in members {
                    match v.as_argument() {
                        Some(text) => {
                            call.args.insert(k.clone(), text);
                        }
                        None if matches!(v, Value::Null) => {}
                        None => {
                            return Some(error(
                                id,
                                -32602,
                                &format!("argument {k} must be a value or a list"),
                            ));
                        }
                    }
                }
            }
            if !TOOLS.iter().any(|t| t.name == name) {
                return Some(error(id, -32602, &format!("unknown tool: {name}")));
            }
            let (text, is_error) = match dispatch(session, &call) {
                Reply::Text(t) => (t, false),
                Reply::Error(e) => (e, true),
            };
            result(
                id,
                Value::obj(vec![
                    (
                        "content",
                        Value::Arr(vec![Value::obj(vec![
                            ("type", Value::str("text")),
                            ("text", Value::str(text)),
                        ])]),
                    ),
                    ("isError", Value::Bool(is_error)),
                ]),
            )
        }
        other => error(id, -32601, &format!("method not found: {other}")),
    })
}

/// Serve until the input ends. Lines longer than [`MAX_LINE_BYTES`] are
/// answered with an error and skipped.
pub fn serve<R: BufRead, W: Write>(
    session: &mut Session,
    mut input: R,
    mut output: W,
) -> std::io::Result<()> {
    let mut buf = Vec::new();
    loop {
        buf.clear();
        let mut too_long = false;
        // read one line, keeping at most MAX_LINE_BYTES of it
        loop {
            let chunk = input.fill_buf()?;
            if chunk.is_empty() {
                break;
            }
            let (take, done) = match chunk.iter().position(|&b| b == b'\n') {
                Some(i) => (i + 1, true),
                None => (chunk.len(), false),
            };
            if buf.len() + take <= MAX_LINE_BYTES {
                buf.extend_from_slice(chunk.get(..take).unwrap_or(&[]));
            } else {
                too_long = true;
            }
            input.consume(take);
            if done {
                break;
            }
        }
        if buf.is_empty() && !too_long {
            return Ok(());
        }
        let answer = if too_long {
            Some(error(Value::Null, -32600, "the message is too long"))
        } else {
            let line = String::from_utf8_lossy(&buf);
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            handle(session, line)
        };
        if let Some(a) = answer {
            output.write_all(json::write(&a).as_bytes())?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(session: &mut Session, line: &str) -> Value {
        handle(session, line).unwrap_or(Value::Null)
    }

    #[test]
    fn initialize_list_and_call() {
        let mut s = Session::new();
        let init = ask(
            &mut s,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{}}}"#,
        );
        assert_eq!(
            init.get("result").and_then(|r| r.get("protocolVersion")).and_then(Value::as_str),
            Some("2025-03-26")
        );
        assert!(
            handle(&mut s, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none()
        );
        let list = ask(&mut s, r#"{"jsonrpc":"2.0","id":"a","method":"tools/list"}"#);
        let tools = match list.get("result").and_then(|r| r.get("tools")) {
            Some(Value::Arr(t)) => t.len(),
            _ => 0,
        };
        assert_eq!(tools, TOOLS.len());
        let call = ask(
            &mut s,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"slicecraft.describe","arguments":{}}}"#,
        );
        let text = json::write(&call);
        assert!(text.contains("no models") && text.contains("\"isError\":false"), "{text}");
        let bad = ask(
            &mut s,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"slicecraft.remove","arguments":{"index":7}}}"#,
        );
        assert!(json::write(&bad).contains("\"isError\":true"));
    }

    #[test]
    fn protocol_errors() {
        let mut s = Session::new();
        let code = |v: &Value| match v.get("error").and_then(|e| e.get("code")) {
            Some(Value::Num(n)) => *n as i64,
            _ => 0,
        };
        assert_eq!(code(&ask(&mut s, "{not json")), -32700);
        assert_eq!(code(&ask(&mut s, r#"{"jsonrpc":"2.0","id":1,"method":"nope"}"#)), -32601);
        assert_eq!(
            code(&ask(
                &mut s,
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"x"}}"#
            )),
            -32602
        );
        assert_eq!(code(&ask(&mut s, r#"[1]"#)), -32600);
    }

    #[test]
    fn serve_answers_line_by_line() {
        let mut s = Session::new();
        let input = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n\n{\"jsonrpc\":\"2.0\",\"method\":\"x\"}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}";
        let mut out = Vec::new();
        assert!(serve(&mut s, &input[..], &mut out).is_ok());
        let text = String::from_utf8_lossy(&out);
        assert_eq!(text.lines().count(), 2, "{text}");
        assert!(text.contains("\"id\":2"));
    }

    #[test]
    fn schemas_mark_required_arguments() {
        let Some(place) = TOOLS.iter().find(|t| t.name == "slicecraft.place") else { return };
        let schema = json::write(&input_schema(place));
        assert!(schema.contains("\"required\":[\"index\"]"), "{schema}");
        assert!(schema.contains("\"x\":{\"type\":\"number\"}"));
    }
}
