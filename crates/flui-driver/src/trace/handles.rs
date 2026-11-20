//! Where a method's parameters and output hold a window or element handle, read from the
//! protocol's own schema of that method: the positions a replay renumbers, and no other.
//!
//! A handle is a field whose schema is `WindowId`, `ElementId` or `ElementRef`, alone, optional
//! or in a list. The same field name can be a handle in one type and text in another (a node's
//! `id` is an element handle, an input source's `id` is the caller's name for it), so the
//! position is found by walking the value along its method's schema, never by the field's name.
//! Where the schema offers alternatives (an optional value, a tagged enum), only those the value
//! fits are followed.

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
                for option in options.iter().filter(|option| self.fits(value, option)) {
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

    /// Whether `value` can be an instance of `schema` as far as choosing among alternatives
    /// needs: its type, a `const` or `enum`, the required fields of an object and the constant
    /// fields (an enum's tag) it carries.
    fn fits(&self, value: &Value, schema: &Value) -> bool {
        if schema
            .get("$ref")
            .and_then(Value::as_str)
            .is_some_and(|reference| HANDLE_DEFINITIONS.contains(&reference))
        {
            return value.is_string();
        }
        let schema = self.resolve(schema);
        if let Some(constant) = schema.get("const") {
            return constant == value;
        }
        if let Some(choices) = schema.get("enum").and_then(Value::as_array) {
            return choices.contains(value);
        }
        if let Some(kind) = schema.get("type")
            && !type_fits(kind, value)
        {
            return false;
        }
        if let (Some(fields), Some(properties)) = (
            value.as_object(),
            schema.get("properties").and_then(Value::as_object),
        ) {
            let required_present =
                schema
                    .get("required")
                    .and_then(Value::as_array)
                    .is_none_or(|required| {
                        required
                            .iter()
                            .filter_map(Value::as_str)
                            .all(|key| fields.contains_key(key))
                    });
            let tags_agree = properties.iter().all(|(key, property)| {
                let property = self.resolve(property);
                match (property.get("const"), fields.get(key)) {
                    (Some(constant), Some(present)) => constant == present,
                    _ => true,
                }
            });
            let known = schema.get("additionalProperties") != Some(&Value::Bool(false))
                || fields.keys().all(|key| properties.contains_key(key));
            return required_present && tags_agree && known;
        }
        true
    }
}

/// Whether `value` is of the JSON type or types `kind` names.
fn type_fits(kind: &Value, value: &Value) -> bool {
    let one = |kind: &str| match kind {
        "null" => value.is_null(),
        "boolean" => value.is_boolean(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        _ => true,
    };
    match kind {
        Value::String(kind) => one(kind),
        Value::Array(kinds) => kinds.iter().filter_map(Value::as_str).any(one),
        _ => true,
    }
}
