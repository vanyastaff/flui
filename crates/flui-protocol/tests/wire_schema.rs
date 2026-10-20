//! The published wire schema: each protocol version's JSON schema is a golden
//! file, the current one must match it, and every published schema must be
//! additive to the next (ADR-0080 "Consequences", ADR-0095 §1).
//!
//! A first run after a version bump writes the new golden with
//! `FLUI_PROTOCOL_BLESS=1`; it never overwrites a published one.

#![cfg(feature = "schemars")]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use flui_protocol::version::BREAKING;
use flui_protocol::{
    ActionRequest, ErrorCode, MotionRequest, MotionState, Node, PROTOCOL_VERSION, ProtocolVersion,
    ReadQuery, Retry, Tree,
};
use serde_json::Value;

fn schema_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/schema")
}

fn golden_path(version: ProtocolVersion) -> PathBuf {
    schema_dir().join(format!("protocol-{version}.json"))
}

/// The schema of every type an agent reads or sends, keyed by type, without
/// its prose: a reworded doc comment is not a schema change. Only a schema's
/// `description` annotation is prose; a property named `description` is a
/// field and stays.
fn current_schema() -> Value {
    fn strip_prose(value: &mut Value) {
        match value {
            Value::Object(map) => {
                map.remove("description");
                for (key, value) in map.iter_mut() {
                    match (key.as_str(), value) {
                        // Keyed by property name: each value is a schema.
                        ("properties", Value::Object(properties)) => {
                            properties.values_mut().for_each(strip_prose);
                        }
                        (_, value) => strip_prose(value),
                    }
                }
            }
            Value::Array(items) => items.iter_mut().for_each(strip_prose),
            _ => {}
        }
    }
    fn of(schema: schemars::Schema) -> Value {
        let mut value = serde_json::to_value(schema).expect("a schema serializes to JSON");
        strip_prose(&mut value);
        value
    }
    serde_json::json!({
        "ActionRequest": of(schemars::schema_for!(ActionRequest)),
        "ErrorCode": of(schemars::schema_for!(ErrorCode)),
        "MotionRequest": of(schemars::schema_for!(MotionRequest)),
        "MotionState": of(schemars::schema_for!(MotionState)),
        "Node": of(schemars::schema_for!(Node)),
        "ReadQuery": of(schemars::schema_for!(ReadQuery)),
        "Retry": of(schemars::schema_for!(Retry)),
        "Tree": of(schemars::schema_for!(Tree)),
    })
}

fn read_schema(path: &Path) -> Value {
    let text =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parsing {}: {e}", path.display()))
}

#[test]
fn the_wire_schema_is_the_one_its_version_published() {
    let path = golden_path(PROTOCOL_VERSION);
    let current = current_schema();
    if !path.exists() && std::env::var_os("FLUI_PROTOCOL_BLESS").is_some() {
        let text = serde_json::to_string_pretty(&current).expect("a schema serializes") + "\n";
        std::fs::write(&path, text).unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
    }
    assert!(
        path.exists(),
        "no published schema for protocol {PROTOCOL_VERSION} at {}; a new version publishes \
         its schema with FLUI_PROTOCOL_BLESS=1",
        path.display()
    );
    assert!(
        read_schema(&path) == current,
        "the wire schema changed but PROTOCOL_VERSION is still {PROTOCOL_VERSION}: bump its \
         minor in crates/flui-protocol/src/version.rs, publish the new schema with \
         FLUI_PROTOCOL_BLESS=1, and list the version in BREAKING if the change is not \
         additive.\ncurrent schema:\n{current:#}",
    );
}

/// Every property name, every enumerated string, and every required field of a
/// schema, each under a path whose array indices are erased, so a variant
/// inserted in the middle of a `oneOf` does not move the others.
#[derive(Debug, Default)]
struct Surface {
    properties: BTreeSet<String>,
    strings: BTreeSet<String>,
    required: BTreeSet<String>,
}

impl Surface {
    fn of(schema: &Value) -> Self {
        fn walk(value: &Value, path: &str, out: &mut Surface) {
            match value {
                Value::Object(map) => {
                    for (key, child) in map {
                        let here = format!("{path}/{key}");
                        match (key.as_str(), child) {
                            ("properties", Value::Object(properties)) => {
                                for (name, property) in properties {
                                    out.properties.insert(format!("{here}/{name}"));
                                    walk(property, &format!("{here}/{name}"), out);
                                }
                            }
                            ("required", Value::Array(names)) => {
                                for name in names.iter().filter_map(Value::as_str) {
                                    out.required.insert(format!("{here}/{name}"));
                                }
                            }
                            ("enum", Value::Array(items)) => {
                                for item in items.iter().filter_map(Value::as_str) {
                                    out.strings.insert(format!("{here}={item}"));
                                }
                            }
                            ("const", Value::String(text)) => {
                                out.strings.insert(format!("{here}={text}"));
                            }
                            _ => walk(child, &here, out),
                        }
                    }
                }
                Value::Array(items) => {
                    for item in items {
                        walk(item, &format!("{path}/*"), out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Self::default();
        walk(schema, "", &mut out);
        out
    }
}

/// What makes `newer` not additive to `older`: a property or enumerated string
/// it lost, or a field it made required.
fn additivity_violations(older: &Value, newer: &Value) -> Vec<String> {
    let (old, new) = (Surface::of(older), Surface::of(newer));
    let lost = |a: &BTreeSet<String>, b: &BTreeSet<String>, what: &str| {
        a.difference(b)
            .map(|item| format!("{what} removed: {item}"))
            .collect::<Vec<_>>()
    };
    let mut out = lost(&old.properties, &new.properties, "property");
    out.extend(lost(&old.strings, &new.strings, "name"));
    out.extend(
        new.required
            .difference(&old.required)
            .filter(|path| {
                let root = path
                    .split('/')
                    .nth(1)
                    .expect("schema paths have a type root");
                older.get(root).is_some()
            })
            .map(|item| format!("field made required: {item}")),
    );
    out
}

fn published_versions() -> Vec<(ProtocolVersion, PathBuf)> {
    let mut out: Vec<(ProtocolVersion, PathBuf)> = std::fs::read_dir(schema_dir())
        .expect("the schema directory exists")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let name = path.file_name()?.to_str()?;
            let version = name.strip_prefix("protocol-")?.strip_suffix(".json")?;
            Some((version.parse().ok()?, path))
        })
        .collect();
    out.sort();
    out
}

#[test]
fn every_published_schema_is_additive_to_the_next() {
    let versions = published_versions();
    assert!(
        versions.iter().any(|(v, _)| *v == PROTOCOL_VERSION),
        "the current version's schema is among the published ones: {versions:?}"
    );
    for pair in versions.windows(2) {
        let [(older, older_path), (newer, newer_path)] = pair else {
            unreachable!("windows(2) yields pairs");
        };
        if BREAKING.iter().any(|(v, _)| v == newer) {
            continue;
        }
        let violations = additivity_violations(&read_schema(older_path), &read_schema(newer_path));
        assert!(
            violations.is_empty(),
            "protocol {newer} is not additive to {older}; list it in BREAKING with its ADR, or \
             restore:\n{}",
            violations.join("\n")
        );
    }
}

/// The additivity check itself, against a schema changed the three ways it
/// must refuse, and one way it must allow.
#[test]
fn the_additivity_check_refuses_a_removed_field_a_respelled_name_and_a_new_required_one() {
    let older = current_schema();
    assert_eq!(additivity_violations(&older, &older), [] as [String; 0]);

    let mut added = older.clone();
    added["Node"]["properties"]["tooltip"] = serde_json::json!({"type": "string"});
    assert!(
        additivity_violations(&older, &added).is_empty(),
        "a new optional field is additive"
    );

    let mut added_type = older.clone();
    added_type["NewResponse"] = serde_json::json!({
        "type": "object",
        "properties": {"result": {"type": "number"}},
        "required": ["result"],
    });
    assert!(
        additivity_violations(&older, &added_type).is_empty(),
        "a new response type does not change existing requests or replies"
    );

    let mut removed = older.clone();
    removed["Node"]["properties"]
        .as_object_mut()
        .expect("Node has properties")
        .remove("focusable");
    assert_eq!(
        additivity_violations(&older, &removed),
        ["property removed: /Node/properties/focusable"]
    );

    let respelled = serde_json::from_str::<Value>(
        &older
            .to_string()
            .replace("\"shutting_down\"", "\"shutdown\""),
    )
    .expect("still JSON");
    assert!(
        additivity_violations(&older, &respelled)
            .iter()
            .any(|v| v.starts_with("name removed:") && v.ends_with("=shutting_down")),
        "{:?}",
        additivity_violations(&older, &respelled)
    );

    let mut required = older.clone();
    required["Node"]["required"]
        .as_array_mut()
        .expect("Node has required fields")
        .push(serde_json::json!("name"));
    assert_eq!(
        additivity_violations(&older, &required),
        ["field made required: /Node/required/name"]
    );
}

/// An inspected tree in its wire spelling: values tagged by kind, defaults of the node
/// (no kind arguments, summary level, nothing omitted) left out, and the JSON read back to
/// the same snapshot. Not yet in a published schema: it joins the next version's golden.
#[test]
fn an_inspect_snapshot_keeps_its_wire_spelling() {
    use flui_protocol::{
        InspectField, InspectLevel, InspectNode, InspectRect, InspectUnit, InspectValue,
    };

    let mut padding = InspectField::new(
        "padding",
        InspectValue::EdgeInsets {
            top: 8.0,
            right: 8.0,
            bottom: 8.0,
            left: 8.0,
        },
    );
    padding.unit = Some(InspectUnit::LogicalPx);
    let mut attached = InspectField::new("attached", InspectValue::Bool(true));
    attached.level = InspectLevel::Debug;
    let mut text = InspectNode::new("r19", "RenderParagraph");
    text.fields
        .push(InspectField::new("text", InspectValue::Str("Save".into())));
    text.links.push("s4".into());
    let mut root = InspectNode::new("r12", "RenderPadding");
    root.fields = vec![attached, padding];
    root.rect = Some(InspectRect {
        x: 8.0,
        y: 8.0,
        width: 344.0,
        height: 32.0,
    });
    root.children.push(text);
    root.omitted_children = 2;

    let json = serde_json::to_value(&root).expect("a snapshot serializes");
    assert_eq!(
        json,
        serde_json::json!({
            "id": "r12",
            "kind": "RenderPadding",
            "fields": [
                {"name": "attached", "value": {"type": "bool", "value": true}, "level": "debug"},
                {
                    "name": "padding",
                    "value": {
                        "type": "edge_insets",
                        "value": {"top": 8.0, "right": 8.0, "bottom": 8.0, "left": 8.0}
                    },
                    "unit": "logical_px"
                }
            ],
            "rect": {"x": 8.0, "y": 8.0, "width": 344.0, "height": 32.0},
            "children": [{
                "id": "r19",
                "kind": "RenderParagraph",
                "fields": [{"name": "text", "value": {"type": "str", "value": "Save"}}],
                "links": ["s4"]
            }],
            "omitted_children": 2
        })
    );
    assert_eq!(
        serde_json::from_value::<InspectNode>(json).expect("the spelling reads back"),
        root
    );
    let schema = serde_json::to_value(schemars::schema_for!(InspectNode))
        .expect("the snapshot has a JSON schema");
    assert!(
        schema.to_string().contains("\"edge_insets\""),
        "the schema names every value kind"
    );
}
