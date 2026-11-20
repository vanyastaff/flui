//! Where a method's parameters and output hold a window or element handle, read from the
//! protocol's own schema of that method: the positions a replay renumbers, and no other.
//!
//! A handle is a field whose schema is `WindowId`, `ElementId` or `ElementRef`, alone, optional
//! or in a list. The same field name can be a handle in one type and text in another (a node's
//! `id` is an element handle, an input source's `id` is the caller's name for it), so the
//! position is found by walking the value along its method's schema, never by the field's name.
//! Where the schema offers alternatives (an optional value, a tagged enum), every one is
//! followed: no two alternatives of one choice type a field of the same name as a handle in one
//! and as anything else in the other (`trace::no_alternative_types_a_field_two_ways` holds the
//! schema to it), so a field a value carries is a handle in every alternative or in none.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

/// The definitions a handle's schema refers to.
const HANDLE_DEFINITIONS: [&str; 3] = [
    "#/$defs/WindowId",
    "#/$defs/ElementId",
    "#/$defs/ElementRef",
];

/// How deep a walk follows a value before it stops: a tree read is far shallower, and a value
/// past it names no handle a replay pairs.
const MAX_DEPTH: usize = 256;

/// Each method's parameter and output schemas and the definitions they refer to.
pub(crate) struct WireSchema {
    methods: BTreeMap<&'static str, (Value, Value)>,
    definitions: Map<String, Value>,
}

impl WireSchema {
    /// The schema of every method of the protocol's table.
    pub(crate) fn of_the_protocol() -> Self {
        let mut generator = schemars::generate::SchemaSettings::draft2020_12().into_generator();
        let mut methods = BTreeMap::new();
        let to_value = |schema: schemars::Schema| serde_json::to_value(schema).unwrap_or_default();
        macro_rules! method_schemas {
            ($(
                $(#[$meta:meta])*
                $ty:ident = $name:literal, $served:ident $(($scope:ident))?, $effect:ident, $tier:ident,
                    $params:ty => $output:ty;
            )+) => {$(
                methods.insert(
                    $name,
                    (
                        to_value(generator.subschema_for::<$params>()),
                        to_value(generator.subschema_for::<$output>()),
                    ),
                );
            )+};
        }
        flui_protocol::for_each_method!(method_schemas);
        Self {
            methods,
            definitions: generator.take_definitions(true),
        }
    }

    /// The JSON pointers of the handles in `value`, `method`'s output when `output`, else its
    /// parameters; none for a method the table does not have.
    pub(crate) fn handles(&self, method: &str, output: bool, value: &Value) -> BTreeSet<String> {
        let mut found = BTreeSet::new();
        if let Some((params, result)) = self.methods.get(method) {
            let schema = if output { result } else { params };
            self.walk(value, schema, String::new(), 0, &mut found);
        }
        found
    }

    fn resolve<'a>(&'a self, schema: &'a Value) -> &'a Value {
        let mut schema = schema;
        for _ in 0..MAX_DEPTH {
            let Some(reference) = schema.get("$ref").and_then(Value::as_str) else {
                break;
            };
            let Some(definition) = reference
                .strip_prefix("#/$defs/")
                .and_then(|name| self.definitions.get(name))
            else {
                break;
            };
            schema = definition;
        }
        schema
    }

    fn walk(
        &self,
        value: &Value,
        schema: &Value,
        pointer: String,
        depth: usize,
        found: &mut BTreeSet<String>,
    ) {
        if depth > MAX_DEPTH {
            return;
        }
        if schema
            .get("$ref")
            .and_then(Value::as_str)
            .is_some_and(|reference| HANDLE_DEFINITIONS.contains(&reference))
        {
            if value.is_string() {
                found.insert(pointer);
            }
            return;
        }
        let schema = self.resolve(schema);
        for key in ["anyOf", "oneOf", "allOf"] {
            if let Some(options) = schema.get(key).and_then(Value::as_array) {
                for option in options {
                    self.walk(value, option, pointer.clone(), depth + 1, found);
                }
            }
        }
        match value {
            Value::Object(fields) => {
                let properties = schema.get("properties").and_then(Value::as_object);
                let additional = schema
                    .get("additionalProperties")
                    .filter(|extra| extra.is_object());
                for (key, child) in fields {
                    let child_schema = properties
                        .and_then(|properties| properties.get(key))
                        .or(additional);
                    if let Some(child_schema) = child_schema {
                        let escaped = key.replace('~', "~0").replace('/', "~1");
                        self.walk(
                            child,
                            child_schema,
                            format!("{pointer}/{escaped}"),
                            depth + 1,
                            found,
                        );
                    }
                }
            }
            Value::Array(items) => {
                if let Some(item_schema) = schema.get("items").filter(|items| items.is_object()) {
                    for (index, item) in items.iter().enumerate() {
                        self.walk(
                            item,
                            item_schema,
                            format!("{pointer}/{index}"),
                            depth + 1,
                            found,
                        );
                    }
                }
            }
            _ => {}
        }
    }
}
