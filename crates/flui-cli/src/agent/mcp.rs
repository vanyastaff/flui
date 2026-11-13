//! `flui mcp`: an MCP server over stdio that forwards to a running app's
//! driver agent (ADR-0239).
//!
//! The agent sees one MCP server whichever FLUI app it drives. Each tool maps
//! onto one of the agent's methods:
//!
//! | Tool | Method | Does |
//! |---|---|---|
//! | `flui_windows` | `windows.list` | lists the windows that have content |
//! | `flui_read` | `semantics.read` | reads a window's semantics tree |
//! | `flui_locate` | `semantics.locate` | finds the elements a locator matches |
//! | `flui_act` | `semantics.act` | performs an advertised action on an element |
//! | `flui_wait` | `wait.until` | holds out until a condition holds or its timeout passes |
//! | `flui_motion` | `motion.set` | reads, pauses or steps the animation clock |
//!
//! A tool that takes a `window` may leave it out when the app has exactly one.
//! Each call's parameters are checked against the method's parameter type
//! before they are sent. Replies carry the app's JSON as text; a failure
//! carries ADR-0080's error object (`code`, `message`, `retry`, …) so the
//! agent branches on the code, not the prose.
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

use flui_protocol::method::{
    Addressed, Method, MethodEffect, MotionSet, SemanticsAct, SemanticsLocate, SemanticsRead,
    WaitUntil, WindowsList,
};
use flui_protocol::session::ClientRole;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::Credentials;
use super::client::{Connection, Failure};

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
`actions` they accept); flui_locate finds the elements a locator (role, name, \
state) matches; flui_act performs one of those actions on a handle. A handle \
is valid for the window and tree that reported it: read again after the app \
changes. After an action, flui_wait holds out until something appears, goes, \
matches or the window settles, instead of reading in a loop. flui_motion \
pauses or steps the animation clock so a read sees a settled frame. A \
failure carries a stable `code` and a `retry` hint; \
`busy` and `timeout` with retry `soon` are worth repeating.";

/// What a tool call talks to. [`Live`] is the running app; tests supply a
/// canned one.
pub(crate) trait Agent {
    /// Calls the agent's `method` with `params`; the app may hold the answer `holds` past
    /// serving it (a wait).
    fn call(&mut self, method: &str, params: Value, holds: Duration) -> Result<Value, Failure>;
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
    let locator = json!({
        "type": "object",
        "description": "Which elements, by what a user sees: `role` (`button`, `check_box`, `text_input`, …); `name`, `value` and `description` as a text, {\"match\": \"exact\" | \"contains\" | \"prefix\", \"text\": \"…\", \"ignore_case\": true}; `identifier`; `state` ({\"checked\", \"disabled\", \"focused\", \"expanded\", \"selected\"}); `within` and `has`, locators of an ancestor and of a descendant; `pick` (`strict`, `first`, `last`, {\"nth\": n}). Every field given must match."
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
            "name": "flui_locate",
            "description": "Find the elements a locator matches, without reading the whole tree. Each match carries its element reference (`w3/e12`, which flui_act takes), the tree revision and the node.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "window": window,
                    "locator": locator,
                    "max_matches": { "type": "integer", "minimum": 1, "description": "Matches to report at most; the reply says when it truncated." }
                },
                "required": ["locator"],
                "additionalProperties": false
            },
            "annotations": { "readOnlyHint": true },
        },
        {
            "name": "flui_act",
            "description": "Perform one action a node advertised in its `actions` (invoke, toggle, set_value, select, focus, expand, collapse, scroll_into_view). The reply says how the app delivered it; read the tree to see its effect. A `timeout` error with effect `may_have_run` means the action is still queued: read before repeating it.",
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
            "name": "flui_wait",
            "description": "Hold out until a condition holds in a window, or its timeout passes, instead of reading in a loop. The reply says whether it was satisfied, how many frames presented, what the condition last matched and, for an unsettled window, why. An unsatisfied wait is a result, not an error.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "window": window,
                    "condition": {
                        "type": ["object", "string"],
                        "description": "One of: {\"appears\": locator}, {\"gone\": locator}, {\"matches\": {\"locator\": locator, \"expect\": {\"name\": text} | {\"value\": text} | {\"state\": {…}} | {\"count\": n} | \"focused\"}}, \"settled\" (no frame, callback, timer or image load pending), {\"frames\": n}, {\"event\": {\"kind\": …}}, {\"all\": [conditions]}, {\"any\": [conditions]}. A locator is as in flui_locate; a text is {\"match\": \"exact\", \"text\": \"…\"}."
                    },
                    "timeout_ms": { "type": "integer", "minimum": 0, "description": "How long to hold out; 5000 when left out, a day at most." }
                },
                "required": ["condition"],
                "additionalProperties": false
            },
            "annotations": { "readOnlyHint": true },
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
        "flui_windows" => send::<WindowsList>(agent, json!({})),
        "flui_read" => window_of(args, agent).and_then(|window| {
            let query = picked(args, &["root", "max_depth", "max_nodes"]);
            send::<SemanticsRead>(agent, json!({ "window": window, "query": query }))
        }),
        "flui_locate" => locator_in_window(args, agent).and_then(|locator| {
            let mut params = json!({ "locator": locator });
            if let Some(max) = args.get("max_matches") {
                params["max_matches"] = max.clone();
            }
            send::<SemanticsLocate>(agent, params)
        }),
        "flui_act" => window_of(args, agent).and_then(|window| {
            let element = required_str(args, "element")?;
            let action = required_str(args, "action")?;
            // A handle from a read is scoped to its window (`e12`); a full
            // reference (`w3/e12`) already names it.
            let element = if element.contains('/') {
                element.to_owned()
            } else {
                format!("{window}/{element}")
            };
            let mut params = json!({ "target": { "element": element }, "action": action });
            if let Some(value) = args.get("value") {
                params["value"] = value.clone();
            }
            send::<SemanticsAct>(agent, params)
        }),
        "flui_wait" => window_of(args, agent).and_then(|window| {
            let mut params = picked(args, &["condition", "timeout_ms"]);
            params["window"] = Value::String(window);
            send_held::<WaitUntil>(agent, params)
        }),
        "flui_motion" => window_of(args, agent).and_then(|window| {
            let request = picked(args, &["rate", "step_ms"]);
            send::<MotionSet>(agent, json!({ "window": window, "request": request }))
        }),
        other => return Err((-32602, format!("unknown tool: {other}"))),
    };
    Ok(tool_result(outcome))
}

/// The fields of `args` named in `keys`, as an object.
fn picked(args: &serde_json::Map<String, Value>, keys: &[&str]) -> Value {
    Value::Object(
        keys.iter()
            .filter_map(|key| Some(((*key).to_owned(), args.get(*key)?.clone())))
            .collect::<serde_json::Map<String, Value>>(),
    )
}

/// Calls `M` with `params`, checked against its parameter type first: what
/// the app would refuse is refused here, before anything is sent.
fn send<M>(agent: &mut impl Agent, params: Value) -> Result<Value, Failure>
where
    M: Method,
    M::Params: Serialize + DeserializeOwned,
{
    checked_send::<M>(agent, params, |_| Duration::ZERO)
}

/// [`send`] for a method the app may hold before it answers (a wait): the
/// reply is awaited as much longer as the parameters ask the app to hold it.
fn send_held<M>(agent: &mut impl Agent, params: Value) -> Result<Value, Failure>
where
    M: Method,
    M::Params: Serialize + DeserializeOwned + Addressed,
{
    checked_send::<M>(agent, params, Addressed::holds_for)
}

fn checked_send<M>(
    agent: &mut impl Agent,
    params: Value,
    holds: impl FnOnce(&M::Params) -> Duration,
) -> Result<Value, Failure>
where
    M: Method,
    M::Params: Serialize + DeserializeOwned,
{
    let params = serde_json::from_value::<M::Params>(params)
        .map_err(|e| invalid(&format!("{}: {e}", M::NAME)))?;
    let holds = holds(&params);
    let params = serde_json::to_value(&params)
        .map_err(|e| Failure::Transport(format!("{} did not serialize: {e}", M::NAME)))?;
    agent.call(M::NAME, params, holds)
}

/// The call's `locator`, in the window it names, the call's `window`, or the
/// app's only one.
fn locator_in_window(
    args: &serde_json::Map<String, Value>,
    agent: &mut impl Agent,
) -> Result<Value, Failure> {
    let Some(Value::Object(locator)) = args.get("locator") else {
        return Err(invalid("`locator` is required and must be an object"));
    };
    let mut locator = locator.clone();
    if locator.get("window").is_none_or(Value::is_null) {
        locator.insert("window".to_owned(), Value::String(window_of(args, agent)?));
    }
    Ok(Value::Object(locator))
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
    let listed = send::<WindowsList>(agent, json!({}))?;
    let windows: Vec<&str> = listed
        .get("windows")
        .and_then(Value::as_array)
        .map_or_default(|windows| windows.iter().filter_map(Value::as_str).collect());
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
        Err(Failure::Transport(why) | Failure::Unsent(why)) => (why, true),
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

    fn connect(&self) -> Result<Connection, Failure> {
        let credentials = Credentials::find(self.session.as_deref()).map_err(Failure::Transport)?;
        Connection::connect(&credentials, ClientRole::Driver, CONNECT_WAIT).map_err(|why| {
            Failure::Transport(format!(
                "{why}. Is the app running? Start it with `flui run --agent`."
            ))
        })
    }
}

impl Agent for Live {
    fn call(&mut self, method: &str, params: Value, holds: Duration) -> Result<Value, Failure> {
        // Once more when the connection broke: the app may have restarted
        // since the last call and left a dead socket behind. A request that
        // never reached it goes again; one that did goes again only when the
        // method reads, since one that acts may have run.
        let reads = flui_protocol::method::find(method)
            .is_some_and(|info| info.effect == MethodEffect::ReadOnly);
        for attempt in 0..2 {
            if self.connection.is_none() {
                self.connection = Some(self.connect()?);
            }
            let connection = self
                .connection
                .as_mut()
                .expect("BUG: connected one line above");
            match connection.request(method, params.clone(), holds) {
                Err(Failure::Unsent(_)) if attempt == 0 => self.connection = None,
                Err(Failure::Transport(_)) if attempt == 0 && reads => self.connection = None,
                Err(failure @ (Failure::Transport(_) | Failure::Unsent(_))) => {
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
    use flui_protocol::wait::WaitParams;

    use super::*;

    /// Answers each method from a table and records what it was asked, and
    /// how long each call let the app hold its answer.
    struct Canned {
        windows: Vec<&'static str>,
        asked: Vec<(String, Value)>,
        held: Vec<Duration>,
        fail_with: Option<Value>,
    }

    impl Canned {
        fn with_windows(windows: &[&'static str]) -> Self {
            Self {
                windows: windows.to_vec(),
                asked: Vec::new(),
                held: Vec::new(),
                fail_with: None,
            }
        }
    }

    impl Agent for Canned {
        fn call(&mut self, method: &str, params: Value, holds: Duration) -> Result<Value, Failure> {
            self.asked.push((method.to_owned(), params));
            self.held.push(holds);
            if let Some(error) = &self.fail_with {
                return Err(Failure::Wire(error.clone()));
            }
            Ok(match method {
                "windows.list" => json!({ "windows": self.windows }),
                _ => json!({ "ok": method }),
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
            [
                "flui_windows",
                "flui_read",
                "flui_locate",
                "flui_act",
                "flui_wait",
                "flui_motion"
            ]
        );

        let mut agent = Canned::with_windows(&["w1"]);
        for name in names {
            let arguments = match name {
                "flui_act" => json!({ "element": "e1", "action": "invoke" }),
                "flui_locate" => json!({ "locator": { "role": "button" } }),
                "flui_wait" => json!({ "condition": "settled" }),
                _ => json!({}),
            };
            let reply = call(&mut agent, name, arguments);
            assert_eq!(reply["result"]["isError"], false, "{name}: {reply}");
        }
    }

    #[test]
    fn a_read_forwards_its_bounds_as_the_methods_query() {
        let mut agent = Canned::with_windows(&["w3"]);
        call(
            &mut agent,
            "flui_read",
            json!({ "window": "w3", "root": "e4", "max_depth": 2, "max_nodes": 50 }),
        );
        assert_eq!(
            agent.asked,
            [(
                "semantics.read".to_owned(),
                json!({ "window": "w3", "query": { "root": "e4", "max_depth": 2, "max_nodes": 50 } })
            )]
        );
    }

    #[test]
    fn an_act_names_its_element_by_window_and_handle() {
        let mut agent = Canned::with_windows(&["w3"]);
        call(
            &mut agent,
            "flui_act",
            json!({ "window": "w3", "element": "e9", "action": "set_value", "value": "hi" }),
        );
        assert_eq!(
            agent.asked,
            [(
                "semantics.act".to_owned(),
                json!({ "target": { "element": "w3/e9" }, "action": "set_value", "value": "hi" })
            )]
        );
    }

    #[test]
    fn a_locate_searches_the_window_its_call_or_its_locator_names() {
        let name = json!({ "match": "exact", "text": "Settings" });
        let mut agent = Canned::with_windows(&["w3"]);
        call(
            &mut agent,
            "flui_locate",
            json!({ "locator": { "role": "button", "name": name }, "max_matches": 2 }),
        );
        call(
            &mut agent,
            "flui_locate",
            json!({ "window": "w1", "locator": { "window": "w5", "role": "button" } }),
        );
        call(
            &mut agent,
            "flui_locate",
            json!({ "window": "w2", "locator": { "window": null, "role": "button" } }),
        );
        assert_eq!(
            agent.asked,
            [
                ("windows.list".to_owned(), json!({})),
                (
                    "semantics.locate".to_owned(),
                    json!({ "locator": { "window": "w3", "role": "button", "name": name },
                            "max_matches": 2 })
                ),
                (
                    "semantics.locate".to_owned(),
                    json!({ "locator": { "window": "w5", "role": "button" } })
                ),
                (
                    "semantics.locate".to_owned(),
                    json!({ "locator": { "window": "w2", "role": "button" } })
                ),
            ]
        );
    }

    #[test]
    fn a_wait_lets_the_app_hold_its_answer_for_the_timeout_it_names() {
        let appears = json!({ "appears": { "role": "button" } });
        let mut agent = Canned::with_windows(&["w3"]);
        call(
            &mut agent,
            "flui_wait",
            json!({ "window": "w3", "condition": appears, "timeout_ms": 30_000 }),
        );
        call(&mut agent, "flui_wait", json!({ "condition": "settled" }));
        call(
            &mut agent,
            "flui_wait",
            json!({ "window": "w3", "condition": "settled", "timeout_ms": u64::MAX }),
        );
        let waits: Vec<(&Value, Duration)> = agent
            .asked
            .iter()
            .zip(&agent.held)
            .filter(|((method, _), _)| method == "wait.until")
            .map(|((_, params), held)| (params, *held))
            .collect();
        assert_eq!(
            waits,
            [
                (
                    &json!({ "window": "w3", "condition": appears, "timeout_ms": 30_000 }),
                    Duration::from_secs(30)
                ),
                (
                    &json!({ "window": "w3", "condition": "settled" }),
                    Duration::from_millis(WaitParams::DEFAULT_TIMEOUT_MS)
                ),
                (
                    &json!({ "window": "w3", "condition": "settled", "timeout_ms": u64::MAX }),
                    Duration::from_millis(WaitParams::MAX_TIMEOUT_MS)
                ),
            ]
        );
        // Nothing else holds: a read is answered within the reply timeout.
        let mut agent = Canned::with_windows(&["w3"]);
        call(&mut agent, "flui_read", json!({}));
        assert!(agent.held.iter().all(Duration::is_zero), "{:?}", agent.held);
    }

    #[test]
    fn a_call_the_method_would_refuse_is_refused_before_anything_is_sent() {
        for arguments in [
            json!({ "window": "w1", "action": "invoke" }),
            json!({ "window": "w1", "element": "x9", "action": "invoke" }),
            json!({ "window": "w1", "element": "e9", "action": "dance" }),
            json!({ "window": "window-one", "element": "e9", "action": "invoke" }),
        ] {
            let mut agent = Canned::with_windows(&["w1"]);
            let reply = call(&mut agent, "flui_act", arguments.clone());
            assert_eq!(reply["result"]["isError"], true, "{arguments}: {reply}");
            let text = reply["result"]["content"][0]["text"].as_str().unwrap();
            assert!(text.contains("invalid_argument"), "{arguments}: {text}");
            assert_eq!(agent.asked.len(), 0, "{arguments}");
        }
        let mut agent = Canned::with_windows(&["w1"]);
        let reply = call(
            &mut agent,
            "flui_read",
            json!({ "window": "w1", "max_depth": -1 }),
        );
        assert_eq!(reply["result"]["isError"], true, "{reply}");
        assert_eq!(agent.asked.len(), 0);
        for (tool, arguments) in [
            ("flui_locate", json!({ "window": "w1" })),
            (
                "flui_locate",
                json!({ "window": "w1", "locator": "button" }),
            ),
            (
                "flui_locate",
                json!({ "window": "w1", "locator": { "state": { "disabled": "yes" } } }),
            ),
            ("flui_wait", json!({ "window": "w1" })),
            ("flui_wait", json!({ "window": "w1", "condition": "soon" })),
            (
                "flui_wait",
                json!({ "window": "w1", "condition": { "frames": -1 } }),
            ),
        ] {
            let mut agent = Canned::with_windows(&["w1"]);
            let reply = call(&mut agent, tool, arguments.clone());
            assert_eq!(
                reply["result"]["isError"], true,
                "{tool} {arguments}: {reply}"
            );
            assert_eq!(agent.asked.len(), 0, "{tool} {arguments}");
        }
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
