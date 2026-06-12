# Diagnostics for test assertions (`flui_foundation::debug`)

`flui_foundation` does not ship a `testing` feature module. Instead it provides the
**diagnostics substrate** every rendering-stack harness uses for structured
assertions: typed property builders on render objects, tree-shaped
[`DiagnosticsNode`](../src/debug.rs) output, and query helpers that avoid
substring-matching `dump()` strings.

All render / layer / display-list harnesses implement or consume
[`Diagnosticable`](../src/debug.rs).

## Where this fits

```text
RenderObject::debug_fill_properties  →  user config (color, padding, …)
PipelineOwner / harness              →  + committed offset / size / geometry
Probe::diagnostics()               →  DiagnosticsNode tree
assert_properties / property()     →  structured CI assertions
```

Property names use **snake_case** (Rust idiom, not Dart camelCase).

## `DiagnosticsNode` — query API

Built by `Diagnosticable::to_diagnostics_node()` and harness tree walks.

| Method | Purpose |
|--------|---------|
| `name()` | Node type label (e.g. `"RenderPadding"`) |
| `get_property("field")` | First property value as `&str` |
| `get_property_f64("field")` | Parsed `f64`, if parseable |
| `find_child("name")` | Direct child by name |
| `find_descendant("RenderColoredBox")` | Depth-first search by node name |
| `properties()` | All properties on this node |
| `to_string()` / `to_string_deep_at_level(level)` | Rendered dump (debug only) |

### Example — structured assertion

```rust
use flui_foundation::DiagnosticsNode;

let tree: DiagnosticsNode = run.diagnostics();
let leaf = tree.find_descendant("RenderColoredBox").expect("leaf");
assert_eq!(leaf.get_property("color"), Some("[1.0, 0.0, 0.0, 1.0]"));
```

Via [`flui_rendering::testing::Probe`](../../flui-rendering/docs/TESTING.md):

```rust
assert_eq!(
    run.property(child_id, "color").as_deref(),
    Some("[1.0, 0.0, 0.0, 1.0]"),
);
assert_eq!(
    run.descendant_property("RenderFlex", "direction").as_deref(),
    Some("Horizontal"),
);
```

## `DiagnosticsBuilder` — authoring API

Render objects implement `debug_fill_properties` with typed helpers (defaults
hidden automatically, kinds format cleanly in dumps).

| Method | Purpose |
|--------|---------|
| `add(name, value)` | Generic display property |
| `add_flag(name, value, if_true)` | Boolean, hidden when false |
| `add_enum(name, value, default)` | Enum / direction labels |
| `add_double` / `add_default_double` | Float with optional unit; hide at default |
| `add_int` | Integer with optional unit |
| `add_size` | `width x height` |
| `add_color` | RGBA display |
| `add_optional` | Skip when `None` |

### Example — render object self-description

```rust
impl Diagnosticable for RenderPadding {
    fn debug_fill_properties(&self, properties: &mut DiagnosticsBuilder) {
        properties.add("padding", format!("{:?}", self.padding()));
    }
}
```

Prefer typed helpers over raw `format!("{:?}")` when a property has a default
or a known kind (`add_default_double` for opacity defaulting to `1.0`, etc.).

## Harness assertion helpers

[`flui_rendering::testing::assertions`](../../flui-rendering/src/testing/assertions.rs)
wrap common `DiagnosticsNode` checks:

- `assert_properties(node, &["color", "padding"])`
- `assert_descendant_properties(&tree, "RenderFlex", &["direction"])`
- `assert_has_committed_size(node)` — runtime `size` from pipeline
- `assert_has_committed_geometry(node)` — runtime sliver `geometry`

## See also

- Render harness: [`flui-rendering/docs/TESTING.md`](../../flui-rendering/docs/TESTING.md)
- Layer harness diagnostics: [`flui-layer/docs/TESTING.md`](../../flui-layer/docs/TESTING.md)
- Workspace overview: [`docs/testing.md`](../../../docs/testing.md)
