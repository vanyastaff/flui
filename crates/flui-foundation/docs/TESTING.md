# Typed inspection for test assertions (`flui_foundation::inspect`)

`flui_foundation` does not ship a `testing` feature module. Instead it provides the
**inspection substrate** every rendering-stack harness uses for structured assertions: objects
describe themselves as typed fields ([`Inspect`](../src/inspect/fields.rs)), a tree walk takes an
owned [`SnapshotNode`](../src/inspect/snapshot.rs), and tests read it by type instead of
substring-matching a printed dump.

## Where this fits

```text
Inspect::inspect (render object, layer)  →  the object's own fields (color, padding, …)
PipelineOwner / LayerTree (InspectTree)  →  + facts: committed offset / size / geometry
snapshot(&tree, SnapshotQuery)           →  SnapshotNode tree
node.get::<T>("field") / get_fact::<T>   →  typed CI assertions
```

Field names use **snake_case** (Rust idiom, not Dart camelCase).

## `SnapshotNode` — query API

| Member | Purpose |
|--------|---------|
| `kind` | Node type (e.g. `"RenderPadding"`), without type arguments (`kind_args`) |
| `get::<T>("field")` | The object's own field read as `T`, or a `FieldError` |
| `get_fact::<T>("size")` | A fact the tree adds (committed size, offset, dirty flags) |
| `field("name")` / `fact("name")` | The untyped `OwnedField` |
| `find_by_kind("RenderFlex")` | The nodes of a kind, in tree order; `.unique()` demands one |
| `find_by_link(link)` | The counterpart of a node in another tree |
| `iter()` | This node and its descendants, parents before children |
| `debug_fields` | How many fields could only be described by `Debug` (untyped) |

`T` is `bool`, `i64`, `u64`, `f64`, `&str`, `Size`, `Offset`, `Rect`, `Rgba`, `EdgeInsets`,
`Duration`, `Link`, `Variant` (a fieldless enum's variant name) and the other kinds of
[`OwnedValue`](../src/inspect/snapshot.rs).

### Example — structured assertion

```rust
use flui_foundation::inspect::Rgba;

let tree = run.render_snapshot();
let leaf = tree.find_by_kind("RenderColoredBox").unique().expect("one leaf");
assert_eq!(leaf.get::<Rgba>("color"), Ok(Rgba::new(1.0, 0.0, 0.0, 1.0)));
assert_eq!(leaf.get_fact::<Size>("size"), Ok(Size::new(10.0, 10.0)));
```

Via [`flui_rendering::testing::Probe`](../../flui-rendering/docs/TESTING.md), whose
`node_snapshot(id)` takes one node alone:

```rust
assert_eq!(run.node_snapshot(child_id).get::<f64>("opacity"), Ok(0.5));
```

## `Inspect` — authoring API

A render object or layer implements [`Inspect`](../src/inspect/fields.rs): a `kind` and the
fields that describe it, written borrowed into a `Fields` sink. Derive it when the struct's
fields are the description.

| Attribute / method | Purpose |
|--------------------|---------|
| `#[inspect(skip)]` | Leave a field out |
| `#[inspect(default = EXPR)]` / `.default(&v)` | The value at which a summary hides the field |
| `#[inspect(unit = logical_px)]` / `.unit(Unit::LogicalPx)` | What a number measures |
| `#[inspect(level = detail)]` | Hide the field from summaries |
| `#[inspect(with = path)]` | Map a field with a `fn(&T) -> Value<'_>` |
| `#[inspect(debug)]` / `out.value(name, Value::Debug(&v))` | Describe by `Debug`, counted as untyped |

### Example — render object self-description

```rust
#[derive(flui_foundation::inspect::Inspect)]
struct RenderPadding {
    #[inspect(unit = logical_px)]
    padding: EdgeInsets,
}
```

Prefer a typed field over `Value::Debug`: a typed field is what a test reads with `get::<T>`;
a `Debug` field can only be compared as text.

## Harness assertion helpers

[`flui_rendering::testing::assertions`](../../flui-rendering/src/testing/assertions.rs)
wrap common `SnapshotNode` checks:

- `assert_properties(node, &["color", "padding"])`
- `assert_descendant_properties(&tree, "RenderFlex", &["direction"])`
- `assert_has_committed_size(node)` — runtime `size` from pipeline
- `assert_has_committed_geometry(node)` — runtime sliver `geometry`

## See also

- Render harness: [`flui-rendering/docs/TESTING.md`](../../flui-rendering/docs/TESTING.md)
- Layer-tree walkers: [`flui-layer/README.md`](../../flui-layer/README.md) (`testing::inspect`)
- Workspace overview: [`docs/testing.md`](../../../docs/testing.md)
