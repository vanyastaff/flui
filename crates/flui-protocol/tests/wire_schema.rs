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
    ActionRequest, ErrorCode, ErrorObject, Event, Greeting, Hello, HelloReply, METHODS,
    MethodEffect, MotionRequest, MotionState, Node, PROTOCOL_VERSION, ProtocolVersion, ReadQuery,
    Request, RequestId, Retry, ServedBy, ServerLine, Tier, Tree,
};
use serde_json::Value;

fn schema_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/schema")
}

fn golden_path(version: ProtocolVersion) -> PathBuf {
    schema_dir().join(format!("protocol-{version}.json"))
}

/// The schema of every type an agent reads or sends, without its prose: a
/// reworded doc comment is not a schema change. Only a schema's
/// `description` annotation is prose; a property named `description` is a
/// field and stays.
///
/// The types ADR-0080 published stand alone, keyed by type, as every version
/// before 0.5 published them. Since 0.5 the method table is there too: each
/// method's side, effect class, tier and the references of its parameters and
/// output, into one `$defs` shared by every method, the error object, the
/// hello and the events.
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
    let mut generator = schemars::generate::SchemaSettings::draft2020_12().into_generator();
    let mut envelope = serde_json::Map::new();
    envelope.insert(
        "ErrorObject".into(),
        of(generator.subschema_for::<ErrorObject>()),
    );
    envelope.insert("Event".into(), of(generator.subschema_for::<Event>()));
    envelope.insert("Hello".into(), of(generator.subschema_for::<Hello>()));
    envelope.insert(
        "HelloReply".into(),
        of(generator.subschema_for::<HelloReply>()),
    );
    // The lines themselves (ADR-0238 §2): a request's params and a result are each method's own,
    // listed under `methods`.
    envelope.insert("Greeting".into(), of(generator.subschema_for::<Greeting>()));
    envelope.insert(
        "Request".into(),
        of(generator.subschema_for::<Request<Value>>()),
    );
    envelope.insert(
        "RequestId".into(),
        of(generator.subschema_for::<RequestId>()),
    );
    envelope.insert(
        "ServerLine".into(),
        of(generator.subschema_for::<ServerLine<Value>>()),
    );
    let mut methods = serde_json::Map::new();
    macro_rules! method_rows {
        ($(
            $(#[$meta:meta])*
            $ty:ident = $name:literal, $served:ident $(($scope:ident))?, $effect:ident, $tier:ident,
                $params:ty => $output:ty;
        )+) => {$(
            methods.insert($name.to_owned(), serde_json::json!({
                "served_by": ServedBy::$served.name(),
                "effect": MethodEffect::$effect.name(),
                "tier": Tier::$tier.name(),
                "params": of(generator.subschema_for::<$params>()),
                "output": of(generator.subschema_for::<$output>()),
            }));
        )+};
    }
    flui_protocol::for_each_method!(method_rows);
    assert_eq!(
        methods.keys().map(String::as_str).collect::<Vec<_>>(),
        METHODS.iter().map(|row| row.name).collect::<Vec<_>>(),
        "the schema describes every method of the table"
    );
    let mut definitions = Value::Object(generator.take_definitions(true));
    strip_prose(&mut definitions);
    serde_json::json!({
        "ActionRequest": of(schemars::schema_for!(ActionRequest)),
        "ErrorCode": of(schemars::schema_for!(ErrorCode)),
        "MotionRequest": of(schemars::schema_for!(MotionRequest)),
        "MotionState": of(schemars::schema_for!(MotionState)),
        "Node": of(schemars::schema_for!(Node)),
        "ReadQuery": of(schemars::schema_for!(ReadQuery)),
        "Retry": of(schemars::schema_for!(Retry)),
        "Tree": of(schemars::schema_for!(Tree)),
        "envelope": envelope,
        "methods": methods,
        "$defs": definitions,
    })
}

/// schemars names a type whose name another type already took by appending a number, so a
/// reader could not tell which of the two a reference means, and the type that gets the plain
/// name reads to the additivity check as the other one's shape changing.
#[test]
fn no_two_wire_types_share_a_schema_name() {
    let schema = current_schema();
    let definitions = schema["$defs"].as_object().expect("the schema has `$defs`");
    let shared: Vec<&String> = definitions
        .keys()
        .filter(|name| {
            let stem = name.trim_end_matches(|c: char| c.is_ascii_digit());
            stem.len() < name.len() && definitions.contains_key(stem)
        })
        .collect();
    assert!(
        shared.is_empty(),
        "types that share a schema name with another, numbered by schemars: {shared:?}"
    );
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
                        let in_union = path.ends_with("/oneOf") || path.ends_with("/anyOf");
                        walk(
                            item,
                            &format!("{path}/{}", variant_segment(item, in_union)),
                            out,
                        );
                    }
                }
                _ => {}
            }
        }
        let mut out = Self::default();
        walk(schema, "", &mut out);
        if let Some(Value::Object(methods)) = schema.get("methods") {
            for name in methods.keys() {
                out.strings.insert(format!("/methods={name}"));
            }
        }
        out
    }
}

/// The path segment of an item of a schema array. A variant of a tagged union is named by its
/// tag, so a variant added to a union is told apart from a field made required in one already
/// there: an internally or adjacently tagged variant by its tag field and value
/// (`row=store_lock`), an externally tagged one (an object of one required property, in a
/// `oneOf`) by its name (`commit=commit`). Any other item is `*`, so an item inserted in the
/// middle does not move the others; a unit variant is a constant, which the strings hold.
fn variant_segment(item: &Value, in_union: bool) -> String {
    let properties = item.get("properties").and_then(Value::as_object);
    let required: Vec<&str> = item
        .get("required")
        .and_then(Value::as_array)
        .map_or_else(Vec::new, |names| {
            names.iter().filter_map(Value::as_str).collect()
        });
    let tagged = properties.and_then(|properties| {
        properties.iter().find_map(|(name, property)| {
            let value = property.get("const")?.as_str()?;
            required
                .contains(&name.as_str())
                .then(|| format!("{name}={value}"))
        })
    });
    let external = || {
        let properties = properties?;
        let [name] = required.as_slice() else {
            return None;
        };
        (in_union && properties.len() == 1 && properties.contains_key(*name))
            .then(|| format!("{name}={name}"))
    };
    tagged.or_else(external).unwrap_or_else(|| "*".to_owned())
}

/// The name of the union a path is inside: the definition after the last `$defs`, or the
/// top-level type the path starts at.
fn union_name(path: &str) -> Option<&str> {
    let segments: Vec<&str> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    match segments.iter().rposition(|segment| *segment == "$defs") {
        Some(at) => segments.get(at + 1).copied(),
        None => segments.first().copied(),
    }
}

/// The unions open to a new value or variant: those whose reader reads an unknown one as a
/// fallback (`version.rs`, "The rule"; ADR-0342 §2). Each names its definition exactly, the
/// source file of its reader and what in that file is the fallback, which
/// `every_open_union_names_its_fallback` checks is there.
const OPEN_UNIONS: &[(&str, &str, &str)] = &[
    ("Role", "src/wire.rs", "serde(other)"),
    ("ErrorCode", "src/error.rs", "serde(other)"),
    ("TreeDiagnostic", "src/tree.rs", "serde(other)"),
    ("ActionName", "src/tree.rs", "fn known_actions"),
];

/// The published versions that added a variant or a value before the check could see one (it
/// named every array item `*` and checked strings for loss only), each with what it added. They
/// were released, and older apps serve them; the list is closed, and a version after the check
/// learned to see variants (ADR-0342 §3) is never added to it.
const PUBLISHED_UNSEEN: &[(ProtocolVersion, &str)] = &[
    (
        ProtocolVersion::new(0, 7),
        "the `announcement` event and its `EventKind`",
    ),
    (
        ProtocolVersion::new(0, 8),
        "the `non_finite` variant of `InspectValue`",
    ),
    (
        ProtocolVersion::new(0, 9),
        "`DeviceClass::Other` and `HandleKind::Recording`",
    ),
    (ProtocolVersion::new(0, 10), "`RecordLayer::Native`"),
];

/// What makes `newer` not additive to `older`: a property, enumerated string or method it
/// lost, a field it made required in a type `older` had, or a value or variant it added to a
/// union `older` had that is not among `open` (whatever its shape: an `enum` list, a `oneOf` of
/// constants, an internally, adjacently or externally tagged union). A method `renamed` lists,
/// old name then new, may go.
///
/// A new value or variant of a closed union is not additive (`version.rs`: an older reader
/// refuses it, failing the whole message), so it passes only in a version `BREAKING` lists
/// (`breaking`), which an app refuses to older clients (`ProtocolVersion::serves`). A new method
/// is a new tool, which ADR-0080 makes additive: a client calls only the methods it knows.
fn additivity_violations(
    older: &Value,
    newer: &Value,
    renamed: &[(&str, &str)],
    breaking: bool,
    open: &[&str],
) -> Vec<String> {
    let (old, new) = (Surface::of(older), Surface::of(newer));
    let is_open = |path: &str| union_name(path).is_some_and(|name| open.contains(&name));
    let lost = |a: &BTreeSet<String>, b: &BTreeSet<String>, what: &str| {
        a.difference(b)
            .map(|item| format!("{what} removed: {item}"))
            .collect::<Vec<_>>()
    };
    let mut out = lost(&old.properties, &new.properties, "property");
    out.extend(
        lost(&old.strings, &new.strings, "name")
            .into_iter()
            .filter(|violation| {
                !renamed.iter().any(|(from, to)| {
                    violation.ends_with(&format!("/methods={from}"))
                        && new.strings.contains(&format!("/methods={to}"))
                })
            }),
    );
    let mut added = BTreeSet::new();
    for value in new.strings.difference(&old.strings) {
        let Some((container, _)) = value.rsplit_once('=') else {
            continue;
        };
        let union_existed = old.strings.iter().any(|known| {
            known
                .rsplit_once('=')
                .is_some_and(|(at, _)| at == container)
        });
        if union_existed && container != "/methods" && !breaking && !is_open(container) {
            added.insert(format!("value added: {value}"));
        }
    }
    for path in new.required.difference(&old.required) {
        let segments: Vec<&str> = path.split('/').collect();
        // The type the field is required in: the path up to `required`, or up to the first
        // erased index or variant, whichever comes first. A type `older` lacks changes nothing
        // an older reader read.
        let owner: Vec<&str> = segments
            .iter()
            .copied()
            .take_while(|segment| {
                *segment != "required" && *segment != "*" && !segment.contains('=')
            })
            .collect();
        if older.pointer(&owner.join("/")).is_none() {
            continue;
        }
        // A field of a tagged variant `older` lacks belongs to a variant added to the union.
        if let Some(at) = segments.iter().position(|segment| segment.contains('=')) {
            let tag = segments[at].split('=').next().unwrap_or_default();
            let variant = segments[..=at].join("/");
            if !old.required.contains(&format!("{variant}/required/{tag}")) {
                let union = segments[..at].join("/");
                if !breaking && !is_open(&union) {
                    added.insert(format!("variant added: {variant}"));
                }
                continue;
            }
        }
        out.push(format!("field made required: {path}"));
    }
    out.extend(added);
    out
}

/// The names of [`OPEN_UNIONS`].
fn open_unions() -> Vec<&'static str> {
    OPEN_UNIONS.iter().map(|(name, _, _)| *name).collect()
}

/// Each open union's reader holds the fallback the list names, so an entry cannot outlive it.
#[test]
fn every_open_union_names_its_fallback() {
    for (name, file, fallback) in OPEN_UNIONS {
        let source = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(file))
            .unwrap_or_else(|error| panic!("reading {file}: {error}"));
        assert!(
            source.contains(&format!("enum {name}")) || source.contains(&format!("<{name}>")),
            "{file} holds `{name}`"
        );
        assert!(
            source.contains(fallback),
            "{file} holds `{name}`'s fallback `{fallback}`"
        );
    }
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
        let broke = BREAKING.iter().find(|change| change.version == *newer);
        let renamed = broke.map_or(&[][..], |change| change.renamed);
        let removed = broke.map_or(&[][..], |change| change.removed);
        let violations = without_removed(
            additivity_violations(
                &read_schema(older_path),
                &read_schema(newer_path),
                renamed,
                broke.is_some() || PUBLISHED_UNSEEN.iter().any(|(version, _)| version == newer),
                &open_unions(),
            ),
            removed,
        );
        assert!(
            violations.is_empty(),
            "protocol {newer} is not additive to {older}; list what it renamed or removed in \
             BREAKING with its ADR, or restore:\n{}",
            violations.join("\n")
        );
    }
}

/// `violations` less the losses under a name a break `removed`: a property, string or
/// required field whose path has the name as one segment, or as the string itself.
fn without_removed(violations: Vec<String>, removed: &[&str]) -> Vec<String> {
    violations
        .into_iter()
        .filter(|violation| {
            let Some(path) = violation
                .strip_prefix("property removed: ")
                .or_else(|| violation.strip_prefix("name removed: "))
            else {
                return true;
            };
            !path
                .split(['/', '='])
                .any(|segment| removed.contains(&segment))
        })
        .collect()
}

/// A loss a break lists as removed may go; anything else it lost, and anything it added
/// to a closed union, still fails.
#[test]
fn a_removal_breaking_lists_may_go() {
    let older = current_schema();
    let mut removed = older.clone();
    removed["Node"]["properties"]
        .as_object_mut()
        .expect("Node has properties")
        .remove("focusable");
    let violations = additivity_violations(&older, &removed, &[], false, &open_unions());
    assert_eq!(violations, ["property removed: /Node/properties/focusable"]);
    assert_eq!(
        without_removed(violations.clone(), &["focusable"]),
        [] as [String; 0]
    );
    assert_eq!(without_removed(violations, &["tooltip"]).len(), 1);
}

/// Protocol 0.4 published no method table: its operations were the agent endpoint's `op` names
/// (ADR-0095), which no schema golden records. So 0.5's `BREAKING` entry is checked against that
/// list here: it renames exactly those operations, each to a method of the table.
///
/// Fails when a rename is dropped from `BREAKING`, which the schema comparison cannot see.
#[test]
fn protocol_0_5_renames_every_0_4_operation_into_the_method_table() {
    const OPERATIONS_0_4: [&str; 4] = ["act", "motion", "read", "windows"];
    let renamed = BREAKING
        .iter()
        .find(|change| change.version == ProtocolVersion::new(0, 5))
        .expect("0.5 lists its renames")
        .renamed;
    let mut from: Vec<&str> = renamed.iter().map(|(from, _)| *from).collect();
    from.sort_unstable();
    assert_eq!(from, OPERATIONS_0_4, "every 0.4 operation is renamed once");
    for (from, to) in renamed {
        assert!(
            flui_protocol::method::find(to).is_some(),
            "`{from}` is renamed to `{to}`, which the method table holds"
        );
    }
}

/// The additivity check itself, against a schema changed the ways it must
/// refuse (a field or method removed, a name respelled, a field made required)
/// and the ways it must allow (a new field, type or definition, a listed rename).
#[test]
fn the_additivity_check_refuses_a_removed_field_a_respelled_name_and_a_new_required_one() {
    let older = current_schema();
    assert_eq!(
        additivity_violations(&older, &older, &[], false, &open_unions()),
        [] as [String; 0]
    );

    let mut added = older.clone();
    added["Node"]["properties"]["tooltip"] = serde_json::json!({"type": "string"});
    assert!(
        additivity_violations(&older, &added, &[], false, &open_unions()).is_empty(),
        "a new optional field is additive"
    );

    let mut added_type = older.clone();
    added_type["NewResponse"] = serde_json::json!({
        "type": "object",
        "properties": {"result": {"type": "number"}},
        "required": ["result"],
    });
    assert!(
        additivity_violations(&older, &added_type, &[], false, &open_unions()).is_empty(),
        "a new response type does not change existing requests or replies"
    );

    let mut removed = older.clone();
    removed["Node"]["properties"]
        .as_object_mut()
        .expect("Node has properties")
        .remove("focusable");
    assert_eq!(
        additivity_violations(&older, &removed, &[], false, &open_unions()),
        ["property removed: /Node/properties/focusable"]
    );

    let respelled = serde_json::from_str::<Value>(
        &older
            .to_string()
            .replace("\"shutting_down\"", "\"shutdown\""),
    )
    .expect("still JSON");
    assert!(
        additivity_violations(&older, &respelled, &[], false, &open_unions())
            .iter()
            .any(|v| v.starts_with("name removed:") && v.ends_with("=shutting_down")),
        "{:?}",
        additivity_violations(&older, &respelled, &[], false, &open_unions())
    );

    let mut required = older.clone();
    required["Node"]["required"]
        .as_array_mut()
        .expect("Node has required fields")
        .push(serde_json::json!("name"));
    assert_eq!(
        additivity_violations(&older, &required, &[], false, &open_unions()),
        ["field made required: /Node/required/name"]
    );

    // A union of each shape, closed and open: a value or variant added to a closed one is a
    // break a `BREAKING` version may make, one added to an open one (its reader has a fallback)
    // is additive, and a field made required in a variant already there is refused even in a
    // breaking version.
    let constant = |value: &str| serde_json::json!({"type": "string", "const": value});
    let internal = |tag: &str, fields: &[&str]| {
        let mut required = vec![serde_json::json!("row")];
        required.extend(fields.iter().map(|field| serde_json::json!(field)));
        serde_json::json!({
            "type": "object",
            "properties": {"row": {"type": "string", "const": tag}},
            "required": required,
        })
    };
    let external = |name: &str| {
        serde_json::json!({
            "type": "object",
            "properties": {name: {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}},
            "required": [name],
        })
    };
    let defined = |name: &str, union: Value| serde_json::json!({"$defs": {name: union}});
    let shapes: [(&str, Value, Value, &str); 4] = [
        (
            "a string enum",
            serde_json::json!({"enum": ["close", "keep_open"]}),
            serde_json::json!({"enum": ["close", "keep_open", "timeout"]}),
            "value added: /$defs/{union}/enum=timeout",
        ),
        (
            "a oneOf of constants",
            serde_json::json!({"oneOf": [constant("close"), constant("keep_open")]}),
            serde_json::json!({"oneOf": [constant("close"), constant("keep_open"), constant("timeout")]}),
            "value added: /$defs/{union}/oneOf/*/const=timeout",
        ),
        (
            "an internally tagged union",
            serde_json::json!({"oneOf": [internal("reading", &["reading"])]}),
            serde_json::json!({"oneOf": [internal("reading", &["reading"]), internal("closed", &["cause"])]}),
            "variant added: /$defs/{union}/oneOf/row=closed",
        ),
        (
            "an externally tagged union",
            serde_json::json!({"oneOf": [constant("enabled"), external("preedit")]}),
            serde_json::json!({"oneOf": [constant("enabled"), external("preedit"), external("commit")]}),
            "variant added: /$defs/{union}/oneOf/commit=commit",
        ),
    ];
    for (shape, older_union, newer_union, added) in shapes {
        let (closed_old, closed_new) = (
            defined("Closed", older_union.clone()),
            defined("Closed", newer_union.clone()),
        );
        assert_eq!(
            additivity_violations(&closed_old, &closed_new, &[], false, &["Opened"]),
            [added.replace("{union}", "Closed")],
            "{shape}: an addition to a closed union is not additive"
        );
        assert!(
            additivity_violations(&closed_old, &closed_new, &[], true, &["Opened"]).is_empty(),
            "{shape}: a version BREAKING lists may add to a closed union"
        );
        assert!(
            additivity_violations(
                &defined("Opened", older_union),
                &defined("Opened", newer_union),
                &[],
                false,
                &["Opened"]
            )
            .is_empty(),
            "{shape}: an addition to a union whose reader has a fallback is additive"
        );
    }
    let one = defined(
        "Closed",
        serde_json::json!({"oneOf": [internal("reading", &["reading"])]}),
    );
    let with_required = defined(
        "Closed",
        serde_json::json!({"oneOf": [internal("reading", &["reading", "cause"])]}),
    );
    assert_eq!(
        additivity_violations(&one, &with_required, &[], true, &[]),
        ["field made required: /$defs/Closed/oneOf/row=reading/required/cause"],
        "a field made required in an existing variant is refused even in a breaking version"
    );
    // The open list names definitions exactly: a union whose name only ends in an open one's is
    // closed.
    assert_eq!(
        additivity_violations(
            &defined("XErrorCode", serde_json::json!({"enum": ["a"]})),
            &defined("XErrorCode", serde_json::json!({"enum": ["a", "b"]})),
            &[],
            false,
            &open_unions()
        ),
        ["value added: /$defs/XErrorCode/enum=b"]
    );

    let mut new_definition = older.clone();
    new_definition["$defs"]["NewParams"] = serde_json::json!({
        "type": "object",
        "properties": {"window": {"type": "string"}},
        "required": ["window"],
    });
    assert!(
        additivity_violations(&older, &new_definition, &[], false, &open_unions()).is_empty(),
        "a new method's parameters do not change an existing method"
    );

    let mut renamed = older.clone();
    let methods = renamed["methods"]
        .as_object_mut()
        .expect("the schema has a method table");
    let row = methods
        .remove("windows.list")
        .expect("windows.list is a method");
    methods.insert("windows.enumerate".into(), row);
    assert_eq!(
        additivity_violations(&older, &renamed, &[], false, &open_unions()),
        ["name removed: /methods=windows.list"],
        "a method renamed without a BREAKING entry is a removal"
    );
    assert!(
        additivity_violations(
            &older,
            &renamed,
            &[("windows.list", "windows.enumerate")],
            false,
            &open_unions()
        )
        .is_empty(),
        "a rename BREAKING lists may go"
    );
    assert_eq!(
        additivity_violations(
            &older,
            &renamed,
            &[("windows.list", "windows.all")],
            false,
            &open_unions()
        ),
        ["name removed: /methods=windows.list"],
        "a rename to a method that does not exist is still a removal"
    );
}

/// An inspected tree in its wire spelling: values tagged by kind, defaults of the node
/// (no kind arguments, summary level, nothing omitted) left out, and the JSON read back to
/// the same snapshot. Published since 0.5, as the output of the `inspect.*` methods.
#[test]
fn an_inspect_snapshot_keeps_its_wire_spelling() {
    use flui_protocol::{
        InspectField, InspectLevel, InspectNode, InspectUnit, InspectValue, LogicalRect,
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
    root.rect = Some(LogicalRect {
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
