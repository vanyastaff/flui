//! The server speaks MCP over stdio and advertises every tool with an input
//! schema, a title, annotations and (but for the image tool) an output
//! schema. Needs no desktop: only `initialize`, `tools/list` and calls that
//! fail before touching it run.

mod support;

use serde_json::Value;
use support::{Client, PROTOCOL};

const TOOLS: &[&str] = &[
    "list_windows",
    "launch",
    "wait_for_window",
    "kill",
    "screenshot",
    "accessibility_tree",
    "find",
    "wait_for",
    "invoke",
    "toggle",
    "set_value",
    "focus",
    "select",
    "expand",
    "collapse",
    "scroll_into_view",
    "click",
    "move_mouse",
    "drag",
    "scroll",
    "type_text",
    "key",
    "activate_window",
];

/// The tools that only read: what a client may auto-approve.
const READ_ONLY: &[&str] = &[
    "list_windows",
    "wait_for_window",
    "screenshot",
    "accessibility_tree",
    "find",
    "wait_for",
];

#[test]
fn initialize_then_list_every_tool_with_its_contract() {
    let (mut client, init) = Client::start();
    assert_eq!(init["protocolVersion"], PROTOCOL);
    assert_eq!(init["serverInfo"]["name"], "flui-desktop-mcp");
    assert!(init["capabilities"]["tools"].is_object(), "{init}");
    let instructions = init["instructions"].as_str().unwrap_or_default();
    for rule in ["require a target", "`gone`", "retry", "effect"] {
        assert!(
            instructions.contains(rule),
            "the instructions state the contract ({rule}): {instructions}"
        );
    }

    let listed = client.request("tools/list", serde_json::json!({}));
    let tools = listed["tools"].as_array().expect("BUG: tools is an array");
    let mut names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    names.sort_unstable();
    let mut expected = TOOLS.to_vec();
    expected.sort_unstable();
    assert_eq!(names, expected);

    for tool in tools {
        let name = tool["name"].as_str().unwrap_or("?");
        assert!(
            tool["description"].as_str().is_some_and(|d| d.len() > 20),
            "{name} has a description"
        );
        assert!(tool["title"].is_string(), "{name} has a title: {tool}");
        let schema = &tool["inputSchema"];
        assert_eq!(schema["type"], "object", "{name} input schema: {schema}");
        let annotations = &tool["annotations"];
        assert_eq!(
            annotations["readOnlyHint"],
            READ_ONLY.contains(&name),
            "{name} annotations: {annotations}"
        );
        if name == "screenshot" {
            assert!(
                tool.get("outputSchema").is_none(),
                "an image reply carries no structured content, so no output schema: {tool}"
            );
        } else {
            let out = &tool["outputSchema"];
            assert_eq!(out["type"], "object", "{name} output schema: {out}");
            assert!(
                out["properties"]["error"].is_object(),
                "{name}'s output schema admits a failure: {out}"
            );
        }
    }

    let schema_of = |name: &str| -> Value {
        tools
            .iter()
            .find(|t| t["name"] == name)
            .map(|t| t["inputSchema"].clone())
            .expect("BUG: the tool is listed")
    };
    // Spot-check that argument shapes reach the schema agents read.
    let key = schema_of("key");
    assert!(key["properties"]["combo"].is_object(), "{key}");
    assert!(key["properties"]["window"].is_object(), "{key}");
    let required = key["required"].as_array().expect("BUG: combo is required");
    assert!(required.iter().any(|r| r == "combo"));
    let click = schema_of("click");
    for field in [
        "element",
        "x",
        "y",
        "screenshot",
        "button",
        "double",
        "window",
        "pid",
    ] {
        assert!(
            click["properties"][field].is_object(),
            "click.{field}: {click}"
        );
    }
    let wait = schema_of("wait_for");
    for field in ["element", "state", "gone", "root"] {
        assert!(
            wait["properties"][field].is_object(),
            "wait_for.{field}: {wait}"
        );
    }
}

/// A refusal is a tool error with a code, a retry policy and the readable
/// message, as structured content and as text.
#[test]
fn refusals_carry_a_code_and_a_message() {
    let (mut client, _) = Client::start();
    let reply = client.call("click", serde_json::json!({ "x": 1, "y": 2 }));
    assert_eq!(reply["isError"], true, "{reply}");
    let error = &reply["structuredContent"]["error"];
    assert_eq!(error["code"], "invalid_argument", "{reply}");
    assert_eq!(error["retry"], "never", "{reply}");
    let message = error["message"].as_str().unwrap_or_default();
    assert!(message.contains("safety target"), "{message}");
    let text: Value = serde_json::from_str(
        reply["content"][0]["text"]
            .as_str()
            .expect("BUG: text envelope"),
    )
    .expect("BUG: text error is JSON");
    assert_eq!(text, reply["structuredContent"]);

    let reply = client.call("kill", serde_json::json!({ "pid": 1 }));
    assert_eq!(reply["isError"], true, "{reply}");
    let text = reply["content"][0]["text"].as_str().unwrap_or_default();
    assert_eq!(
        reply["structuredContent"]["error"]["code"], "unknown_handle",
        "{reply}"
    );
    assert!(text.contains("launch"), "{text}");

    // A handle never issued is unknown, with its kind as data.
    let reply = client.call(
        "type_text",
        serde_json::json!({ "text": "x", "window": "w9" }),
    );
    let error = &reply["structuredContent"]["error"];
    assert!(
        error["code"] == "unknown_handle" || error["code"] == "not_supported",
        "no input device on this host is the other honest answer: {reply}"
    );
    if error["code"] == "unknown_handle" {
        assert_eq!(error["kind"], "window", "{reply}");
        assert_eq!(error["handle"], "w9", "{reply}");
    }
}

/// Arguments that do not even deserialize (an unknown field, a wrong type,
/// a missing field) are tool errors too, not JSON-RPC errors: the agent
/// sees the tool's answer, and it names the argument.
#[test]
fn malformed_arguments_are_tool_errors_not_protocol_errors() {
    let (mut client, _) = Client::start();
    for (tool, args, names) in [
        (
            "key",
            serde_json::json!({ "combo": "enter", "windowId": 3 }),
            "windowId",
        ),
        ("kill", serde_json::json!({ "pid": -1 }), "-1"),
        ("key", serde_json::json!({}), "combo"),
        (
            "wait_for",
            serde_json::json!({ "pid": 1, "name": "x", "state": { "checked": "on" } }),
            "mixed",
        ),
    ] {
        let reply = client.call(tool, args.clone());
        assert_eq!(reply["isError"], true, "{tool} {args}: {reply}");
        let text = reply["content"][0]["text"].as_str().unwrap_or_default();
        assert!(
            text.contains("invalid arguments") && text.contains(names),
            "{tool} {args}: {text}"
        );
        assert_eq!(
            reply["structuredContent"]["error"]["code"], "invalid_argument",
            "{tool} {args}: {reply}"
        );
    }
}
