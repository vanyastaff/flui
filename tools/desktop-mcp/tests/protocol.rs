//! The server speaks MCP over stdio and advertises every tool with an input
//! schema. Needs no desktop: only `initialize` and `tools/list` run.

mod support;

use serde_json::Value;
use support::{Client, PROTOCOL};

const TOOLS: &[&str] = &[
    "list_windows",
    "launch",
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
    "click",
    "move_mouse",
    "drag",
    "scroll",
    "type_text",
    "key",
    "activate_window",
];

#[test]
fn initialize_then_list_every_tool_with_a_schema() {
    let (mut client, init) = Client::start();
    assert_eq!(init["protocolVersion"], PROTOCOL);
    assert_eq!(init["serverInfo"]["name"], "flui-desktop-mcp");
    assert!(init["capabilities"]["tools"].is_object(), "{init}");
    assert!(
        init["instructions"]
            .as_str()
            .is_some_and(|s| s.contains("always pass window_id or pid")),
        "the safety rule is part of the server instructions"
    );

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
        let schema = &tool["inputSchema"];
        assert_eq!(schema["type"], "object", "{name} input schema: {schema}");
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
    assert!(key["properties"]["window_id"].is_object(), "{key}");
    let required = key["required"].as_array().expect("BUG: combo is required");
    assert!(required.iter().any(|r| r == "combo"));
    let click = schema_of("click");
    for field in ["element", "x", "y", "button", "double", "window_id", "pid"] {
        assert!(
            click["properties"][field].is_object(),
            "click.{field}: {click}"
        );
    }
}

#[test]
fn argument_errors_are_tool_errors_the_agent_can_read() {
    let (mut client, _) = Client::start();
    let reply = client.call("click", serde_json::json!({ "x": 1 }));
    assert_eq!(reply["isError"], true, "{reply}");
    let text = reply["content"][0]["text"].as_str().unwrap_or_default();
    assert!(text.contains("`x` and `y`"), "{text}");

    let reply = client.call("kill", serde_json::json!({ "pid": 1 }));
    assert_eq!(reply["isError"], true, "{reply}");
    let text = reply["content"][0]["text"].as_str().unwrap_or_default();
    assert!(text.contains("not launched by this session"), "{text}");
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
    ] {
        let reply = client.call(tool, args.clone());
        assert_eq!(reply["isError"], true, "{tool} {args}: {reply}");
        let text = reply["content"][0]["text"].as_str().unwrap_or_default();
        assert!(
            text.contains("invalid arguments") && text.contains(names),
            "{tool} {args}: {text}"
        );
    }
}
