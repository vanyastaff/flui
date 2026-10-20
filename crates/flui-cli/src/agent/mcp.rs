//! `flui mcp`: an MCP server over stdio that forwards to a running app's
//! development endpoint (ADR-0095 §3).
//!
//! The agent sees one MCP server whichever FLUI app it drives. Four tools map
//! one to one onto the endpoint's operations:
//!
//! | Tool | Endpoint op | Does |
//! |---|---|---|
//! | `flui_windows` | `windows` | lists the windows that have content |
//! | `flui_read` | `read` | reads a window's semantics tree |
//! | `flui_act` | `act` | performs an advertised action on an element |
//! | `flui_motion` | `motion` | reads, pauses or steps the animation clock |
//!
//! A tool that takes a `window` may leave it out when the app has exactly one.
//! Replies carry the endpoint's JSON as text; a failure carries ADR-0080's
//! error object (`code`, `message`, `retry`, …) so the agent branches on the
//! code, not the prose.
//!
//! The server connects on the first tool call, so a client can register
//! `flui mcp` before the app exists and the app can restart under it: a
//! broken connection is dropped and made again once per call.
//!
//! Messages are JSON-RPC 2.0, one per line on stdin and stdout (MCP's stdio
//! transport). Nothing but protocol goes to stdout.

use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Value, json};

use super::client::{Connection, Failure};
use super::{Credentials, ENDPOINT_ENV, TOKEN_ENV, find_session_file};

/// The MCP revisions this server speaks, newest first. A client that asks for
/// another gets the newest; the tools are plain and the same in each.
const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// How long the first call waits for an app that has not bound its endpoint
/// yet (still building, or restarting).
const CONNECT_WAIT: Duration = Duration::from_secs(10);

const INSTRUCTIONS: &str = "\
Drives a running FLUI app through its accessibility tree. Start the app with \
`flui run --agent`, then: flui_windows lists its windows; flui_read returns a \
window's tree (nodes carry a `role`, an `id` handle such as `e12`, and the \
`actions` they accept); flui_act performs one of those actions on a handle. \
A handle is valid for the window and tree that reported it: read again after \
the app changes. flui_motion pauses or steps the animation clock so a read \
sees a settled frame. A failure carries a stable `code` and a `retry` hint; \
`busy` and `timeout` with retry `soon` are worth repeating.";

/// What a tool call talks to. [`Live`] is the running app; tests supply a
/// canned one.
pub(crate) trait Agent {
    /// Runs one endpoint operation with `fields` merged into its request.
    fn call(&mut self, op: &str, fields: Value) -> Result<Value, Failure>;
}

/// Serves MCP on `input` and `output` until `input` ends.
pub(crate) fn serve(
    input: impl BufRead,
    mut output: impl Write,
    agent: &mut impl Agent,
) -> io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(message) => handle(&message, agent),
            Err(e) => Some(rpc_error(Value::Null, -32700, &format!("parse error: {e}"))),
        };
        if let Some(reply) = reply {
            writeln!(output, "{reply}")?;
            output.flush()?;
        }
    }
    Ok(())
}

/// The reply to one JSON-RPC message; `None` for a notification.
fn handle(message: &Value, agent: &mut impl Agent) -> Option<Value> {
    let id = message.get("id").cloned();
    let method = message.get("method").and_then(Value::as_str);
    let (Some(id), Some(method)) = (id, method) else {
        // A notification, or a response to a request we never sent.
        return None;
    };
    let params = message.get("params").unwrap_or(&Value::Null);
    let outcome = match method {
        "initialize" => Ok(initialize(params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools() })),
        "tools/call" => call_tool(params, agent),
        other => Err((-32601, format!("method not found: {other}"))),
    };
    Some(match outcome {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, message)) => rpc_error(id, code, &message),
    })
}

fn rpc_error(id: Value, code: i32, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn initialize(params: &Value) -> Value {
    let wanted = params.get("protocolVersion").and_then(Value::as_str);
    let version = PROTOCOL_VERSIONS
        .iter()
        .find(|v| Some(**v) == wanted)
        .unwrap_or(&PROTOCOL_VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "flui", "version": env!("CARGO_PKG_VERSION") },
        "instructions": INSTRUCTIONS,
    })
}

fn tools() -> Value {
    let window = json!({
        "type": "string",
        "description": "A window handle from flui_windows (`w3`). May be left out when the app has one window."
    });
    json!([
        {
            "name": "flui_windows",
            "description": "List the windows of the running FLUI app that have content.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false },
            "annotations": { "readOnlyHint": true },
        },
        {
            "name": "flui_read",
            "description": "Read a window's semantics tree: nodes with role, id handle, label, state and the actions they accept. Bound large trees with max_depth and max_nodes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "window": window,
                    "root": { "type": "string", "description": "Read under this element handle (`e12`) instead of from the window." },
                    "max_depth": { "type": "integer", "minimum": 0, "description": "Levels below the root to read; the root alone is 0." },
                    "max_nodes": { "type": "integer", "minimum": 1, "description": "Elements to report at most; the reply says when it truncated." }
                },
                "additionalProperties": false
            },
            "annotations": { "readOnlyHint": true },
        },
        {
            "name": "flui_act",
            "description": "Perform one action a node advertised in its `actions` (invoke, toggle, set_value, select, focus, expand, collapse, scroll_into_view). The reply means the app received it; read the tree to see its effect. A `timeout` error with effect `may_have_run` means the action is still queued: read before repeating it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "window": window,
                    "element": { "type": "string", "description": "The element handle (`e12`) from a read." },
                    "action": { "type": "string", "description": "One of the node's advertised actions." },
                    "value": { "type": "string", "description": "The new value, for set_value." }
                },
                "required": ["element", "action"],
                "additionalProperties": false
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": false },
        },
        {
            "name": "flui_motion",
            "description": "Read or change a window's animation clock. With no arguments it reports the rate and time. `rate` sets animation seconds per second (0 pauses); `step_ms` advances animation time, also while paused, so a read sees a settled frame.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "window": window,
                    "rate": { "type": "number", "minimum": 0 },
                    "step_ms": { "type": "integer", "minimum": 0 }
                },
                "additionalProperties": false
            },
            "annotations": { "readOnlyHint": false, "idempotentHint": false },
        }
    ])
}

fn call_tool(params: &Value, agent: &mut impl Agent) -> Result<Value, (i32, String)> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or((-32602, "tools/call names no tool".to_owned()))?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let Some(args) = args.as_object() else {
        return Err((-32602, "arguments must be an object".into()));
    };
    let outcome = match name {
        "flui_windows" => agent.call("windows", json!({})),
        "flui_read" => window_of(args, agent).and_then(|window| {
            let mut query = serde_json::Map::new();
            for (field, key) in [
                ("root", "root"),
                ("max_depth", "max_depth"),
                ("max_nodes", "max_nodes"),
            ] {
                if let Some(value) = args.get(field) {
                    query.insert(key.into(), value.clone());
                }
            }
            agent.call("read", json!({ "window": window, "query": query }))
        }),
        "flui_act" => window_of(args, agent).and_then(|window| {
            let element = required_str(args, "element")?;
            let action = required_str(args, "action")?;
            let mut request = json!({ "element": element, "action": action });
            if let Some(value) = args.get("value") {
                request["value"] = value.clone();
            }
            agent.call("act", json!({ "window": window, "request": request }))
        }),
        "flui_motion" => window_of(args, agent).and_then(|window| {
            let mut request = serde_json::Map::new();
            for key in ["rate", "step_ms"] {
                if let Some(value) = args.get(key) {
                    request.insert(key.into(), value.clone());
                }
            }
            agent.call("motion", json!({ "window": window, "request": request }))
        }),
        other => return Err((-32602, format!("unknown tool: {other}"))),
    };
    Ok(tool_result(outcome))
}

/// The window a call names, or the app's only one.
fn window_of(
    args: &serde_json::Map<String, Value>,
    agent: &mut impl Agent,
) -> Result<String, Failure> {
    if let Some(window) = args.get("window") {
        return window
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| invalid("`window` must be a string handle"));
    }
    let listed = agent.call("windows", json!({}))?;
    let windows: Vec<&str> = listed
        .get("windows")
        .and_then(Value::as_array)
        .map(|windows| windows.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    match windows.as_slice() {
        [only] => Ok((*only).to_owned()),
        [] => Err(Failure::Wire(json!({
            "code": "not_found",
            "message": "the app has no window with content yet",
            "retry": "soon",
        }))),
        many => Err(invalid(&format!(
            "the app has {} windows ({}); name one in `window`",
            many.len(),
            many.join(", ")
        ))),
    }
}

fn required_str<'a>(
    args: &'a serde_json::Map<String, Value>,
    key: &str,
) -> Result<&'a str, Failure> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(&format!("`{key}` is required and must be a string")))
}

fn invalid(message: &str) -> Failure {
    Failure::Wire(json!({ "code": "invalid_argument", "message": message, "retry": "never" }))
}

/// MCP's tool result: the JSON as text, flagged when it is a failure.
fn tool_result(outcome: Result<Value, Failure>) -> Value {
    let (text, is_error) = match outcome {
        Ok(value) => (
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string()),
            false,
        ),
        Err(Failure::Wire(error)) => (
            serde_json::to_string_pretty(&json!({ "error": error })).unwrap_or_default(),
            true,
        ),
        Err(Failure::Transport(why)) => (why, true),
    };
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}

/// The running app, found and connected on demand.
pub(crate) struct Live {
    /// A session file named with `--session`, which wins over discovery.
    session: Option<PathBuf>,
    connection: Option<Connection>,
}

impl Live {
    pub(crate) fn new(session: Option<PathBuf>) -> Self {
        Self {
            session,
            connection: None,
        }
    }

    /// The credentials to use now: the session file named on the command
    /// line, else the environment, else the nearest `target/flui-agent.json`.
    fn credentials(&self) -> Result<Credentials, String> {
        if let Some(path) = &self.session {
            return Credentials::read(path);
        }
        if let Some(credentials) = Credentials::from_env() {
            return Ok(credentials);
        }
        let cwd = std::env::current_dir().map_err(|e| format!("no working directory: {e}"))?;
        match find_session_file(&cwd) {
            Some(path) => Credentials::read(&path),
            None => Err(format!(
                "no running FLUI app found from {}. Start one with `flui run --agent`, \
                 or pass --session <file>, or set {ENDPOINT_ENV} and {TOKEN_ENV}",
                cwd.display()
            )),
        }
    }

    fn connect(&self) -> Result<Connection, Failure> {
        let credentials = self.credentials().map_err(Failure::Transport)?;
        Connection::connect(&credentials, CONNECT_WAIT).map_err(|why| {
            Failure::Transport(format!(
                "{why}. Is the app running? Start it with `flui run --agent`."
            ))
        })
    }
}

impl Agent for Live {
    fn call(&mut self, op: &str, fields: Value) -> Result<Value, Failure> {
        // Once more on a broken connection: the app may have restarted since
        // the last call and left a dead socket behind.
        for attempt in 0..2 {
            if self.connection.is_none() {
                self.connection = Some(self.connect()?);
            }
            let connection = self
                .connection
                .as_mut()
                .expect("BUG: connected one line above");
            match connection.request(op, fields.clone()) {
                Err(Failure::Transport(_)) if attempt == 0 => self.connection = None,
                Err(failure @ Failure::Transport(_)) => {
                    self.connection = None;
                    return Err(failure);
                }
                other => return other,
            }
        }
        unreachable!("BUG: the second attempt returns")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Answers each op from a table and records what it was asked.
    struct Canned {
        windows: Vec<&'static str>,
        asked: Vec<(String, Value)>,
        fail_with: Option<Value>,
    }

    impl Canned {
        fn with_windows(windows: &[&'static str]) -> Self {
            Self {
                windows: windows.to_vec(),
                asked: Vec::new(),
                fail_with: None,
            }
        }
    }

    impl Agent for Canned {
        fn call(&mut self, op: &str, fields: Value) -> Result<Value, Failure> {
            self.asked.push((op.to_owned(), fields));
            if let Some(error) = &self.fail_with {
                return Err(Failure::Wire(error.clone()));
            }
            Ok(match op {
                "windows" => json!({ "windows": self.windows }),
                _ => json!({ "ok": op }),
            })
        }
    }

    fn rpc(agent: &mut Canned, message: Value) -> Value {
        handle(&message, agent).expect("a request is answered")
    }

    fn call(agent: &mut Canned, tool: &str, arguments: Value) -> Value {
        rpc(
            agent,
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                    "params": { "name": tool, "arguments": arguments } }),
        )
    }

    #[test]
    fn initialize_negotiates_a_version_and_advertises_tools_only() {
        let mut agent = Canned::with_windows(&[]);
        let reply = rpc(
            &mut agent,
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize",
                    "params": { "protocolVersion": "2025-03-26" } }),
        );
        assert_eq!(reply["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(reply["result"]["capabilities"], json!({ "tools": {} }));

        let newer = rpc(
            &mut agent,
            json!({ "jsonrpc": "2.0", "id": 2, "method": "initialize",
                    "params": { "protocolVersion": "2099-01-01" } }),
        );
        assert_eq!(newer["result"]["protocolVersion"], PROTOCOL_VERSIONS[0]);
    }

    #[test]
    fn notifications_get_no_reply_and_unknown_methods_get_an_error() {
        let mut agent = Canned::with_windows(&[]);
        let note = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        assert!(handle(&note, &mut agent).is_none());
        let reply = rpc(
            &mut agent,
            json!({ "jsonrpc": "2.0", "id": 7, "method": "resources/list" }),
        );
        assert_eq!(reply["id"], 7);
        assert_eq!(reply["error"]["code"], -32601);
    }

    #[test]
    fn every_listed_tool_is_callable_and_declares_an_object_schema() {
        let listed = tools();
        let names: Vec<&str> = listed
            .as_array()
            .unwrap()
            .iter()
            .map(|t| {
                assert_eq!(t["inputSchema"]["type"], "object");
                t["name"].as_str().unwrap()
            })
            .collect();
        assert_eq!(
            names,
            ["flui_windows", "flui_read", "flui_act", "flui_motion"]
        );

        let mut agent = Canned::with_windows(&["w1"]);
        for name in names {
            let arguments = if name == "flui_act" {
                json!({ "element": "e1", "action": "invoke" })
            } else {
                json!({})
            };
            let reply = call(&mut agent, name, arguments);
            assert_eq!(reply["result"]["isError"], false, "{name}: {reply}");
        }
    }

    #[test]
    fn a_read_forwards_its_bounds_as_the_endpoints_query() {
        let mut agent = Canned::with_windows(&["w3"]);
        call(
            &mut agent,
            "flui_read",
            json!({ "window": "w3", "root": "e4", "max_depth": 2, "max_nodes": 50 }),
        );
        assert_eq!(
            agent.asked,
            [(
                "read".to_owned(),
                json!({ "window": "w3", "query": { "root": "e4", "max_depth": 2, "max_nodes": 50 } })
            )]
        );
    }

    #[test]
    fn an_act_wraps_the_action_request_the_endpoint_reads() {
        let mut agent = Canned::with_windows(&["w3"]);
        call(
            &mut agent,
            "flui_act",
            json!({ "window": "w3", "element": "e9", "action": "set_value", "value": "hi" }),
        );
        assert_eq!(
            agent.asked,
            [(
                "act".to_owned(),
                json!({ "window": "w3",
                        "request": { "element": "e9", "action": "set_value", "value": "hi" } })
            )]
        );
    }

    #[test]
    fn a_missing_window_is_the_only_one_and_ambiguity_is_an_error_naming_them() {
        let mut one = Canned::with_windows(&["w5"]);
        call(&mut one, "flui_motion", json!({ "step_ms": 16 }));
        assert_eq!(one.asked[1].1["window"], "w5");
        assert_eq!(one.asked[1].1["request"], json!({ "step_ms": 16 }));

        let mut two = Canned::with_windows(&["w1", "w2"]);
        let reply = call(&mut two, "flui_read", json!({}));
        assert_eq!(reply["result"]["isError"], true);
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("invalid_argument") && text.contains("w1, w2"),
            "{text}"
        );
        assert_eq!(two.asked.len(), 1, "no read was sent");

        let mut none = Canned::with_windows(&[]);
        let reply = call(&mut none, "flui_read", json!({}));
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("not_found") && text.contains("soon"),
            "{text}"
        );
    }

    #[test]
    fn an_act_without_its_element_is_refused_before_anything_is_sent() {
        let mut agent = Canned::with_windows(&["w1"]);
        let reply = call(
            &mut agent,
            "flui_act",
            json!({ "window": "w1", "action": "invoke" }),
        );
        assert_eq!(reply["result"]["isError"], true);
        assert_eq!(agent.asked.len(), 0);
    }

    #[test]
    fn the_apps_error_object_reaches_the_agent_whole() {
        let mut agent = Canned::with_windows(&["w1"]);
        agent.fail_with = Some(json!({
            "code": "timeout", "message": "slow", "retry": "never",
            "effect": { "kind": "may_have_run" }
        }));
        let reply = call(
            &mut agent,
            "flui_act",
            json!({ "window": "w1", "element": "e1", "action": "invoke" }),
        );
        assert_eq!(reply["result"]["isError"], true);
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["error"]["code"], "timeout");
        assert_eq!(parsed["error"]["effect"]["kind"], "may_have_run");
    }

    #[test]
    fn an_unknown_tool_and_a_malformed_line_are_protocol_errors_and_the_loop_continues() {
        let mut agent = Canned::with_windows(&[]);
        let input = [
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"nope"}}"#,
            "not json",
            "",
            r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
        ]
        .join("\n");
        let mut out = Vec::new();
        serve(input.as_bytes(), &mut out, &mut agent).unwrap();
        let replies: Vec<Value> = String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(replies.len(), 3);
        assert_eq!(replies[0]["error"]["code"], -32602);
        assert_eq!(replies[1]["error"]["code"], -32700);
        assert_eq!(replies[2]["result"], json!({}));
    }
}
