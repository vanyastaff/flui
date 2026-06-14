# Painted-scene introspection & serialization contract — design spec (Diagnosticable-everywhere)

> Decision record: [`ADR-0005`](../adr/ADR-0005-diagnosticable-everywhere-introspection.md) (supersedes ADR-0004). This spec is the implementation-facing companion; the *why* + rejected alternatives live in the ADR. Implementation plan: `2026-06-14-scene-snapshot-contract-plan.md`.

## Summary

Make flui's existing `Diagnosticable`/`DiagnosticsNode` system the **one** introspection substrate for everything, including the painted scene, by:

1. **Typed-value upgrade** — `DiagnosticsProperty.value: String → DiagnosticsValue` (a serde+schemars sum), back-compatible so all ~49 existing impls compile unchanged. Inspector JSON becomes faithful (full precision); normalization lives only in the text renderer.
2. **`impl Diagnosticable for DrawCommand`** (31 variants) + `Layer::to_diagnostics_node` descending a `PictureLayer` into per-command **child** nodes (opaque handles → id/descriptor). The painted command stream enters the tree.
3. **One tree, three sinks** via `SnapshotStrategy<DiagnosticsNode, Format>`: `.text` (golden), `.json` (faithful inspector + synthetic first consumer), debug.
4. **Committed `schemars` JSON Schema** over `DiagnosticsNode`/`DiagnosticsValue` + `format_version` + `#[non_exhaustive]` = the years-long, framework-wide contract.

No parallel snapshot taxonomy (the ADR-0004 `SceneSnapshot`/`DrawCommandSummary` is not built). The PR #213 string serializer is retired; the six `.snap` goldens are **regenerated** to the `DiagnosticsNode` text format (content-equivalence verified by review).

## Context (verified substrate, flui-foundation/src/debug.rs)

- `Diagnosticable` (≈1047): `debug_fill_properties(&self, &mut DiagnosticsBuilder)` + overridable `to_diagnostics_node()`.
- `DiagnosticsNode` (≈602): real tree `{ name, properties, children, level, style }`; serde; `#[non_exhaustive]`; `Display` = `format_deep`; `value_of(name)` for structured asserts.
- `DiagnosticsProperty` (≈321): `{ name, value: String, level, kind: DiagnosticsPropertyKind, … }`; serde; `#[non_exhaustive]`. `kind` already discriminates `Rect`/`Color`/`Double{unit}`/… — only `value` is stringly.
- `DiagnosticsBuilder` (≈1081): `add(name, impl Display)` (stringifies), `add_flag`, `add_optional`.
- ~49 `Diagnosticable` impls; `Layer::to_diagnostics_node` already overridden (`flui-layer/src/layer/mod.rs:395`); **no `impl Diagnosticable for DrawCommand`** yet.
- The PR #213 `summarize_command` (31 arms, `crates/flui-rendering/src/testing/snapshot.rs`) is the authoritative **field-set** per command (which fields each command should expose) — port it into `DrawCommand::debug_fill_properties` as typed properties.
- Foundation retained from ADR-0004: schemars dep (commit `4c47bf2b`), `#[non_exhaustive]` on Node/Property (`239853b7`), hoisted fmt helpers in flui-painting (`5f7bf9a2`).

## Design

### Layer 1 — typed `DiagnosticsValue` (flui-foundation)

```rust
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum DiagnosticsValue {
    Null, Bool(bool), Int(i64), Float(f64), Str(String),
    Color { r: u8, g: u8, b: u8, a: u8 },
    Rect { x: f64, y: f64, w: f64, h: f64 },
    Offset { x: f64, y: f64 },
    Size { w: f64, h: f64 },
    List(Vec<DiagnosticsValue>),
    Nested(Vec<DiagnosticsProperty>),
}
impl core::fmt::Display for DiagnosticsValue { /* Float→2-dec, Color→#RRGGBBAA, … (reuse the hoisted flui-painting fmt helpers where natural) */ }
impl From<&str> for DiagnosticsValue { /* Str */ }   // + From<String>, f64, i64, bool
```

`DiagnosticsProperty.value: DiagnosticsValue`. **Back-compat (mandatory):** `DiagnosticsProperty::new(name, value: impl Display)` → `DiagnosticsValue::Str(value.to_string())`; `DiagnosticsProperty::value()` returns the `Display` string. So existing callers + doctests (`assert_eq!(prop.value(), "100")`) are unchanged. New typed builder methods: `DiagnosticsBuilder::{add_f64, add_int, add_bool, add_color, add_rect, add_typed}`. serde serializes the typed value (faithful); text normalizes in `Display`.

### Layer 2 — `impl Diagnosticable for DrawCommand` (flui-painting)

One `debug_fill_properties` per the 31 variants, pushing **typed** properties that mirror the curated field-set the existing `summarize_command` arm formats (read each arm). E.g. `DrawRect` → `add_rect("rect", …)` + `add_color("color", …)` + flag for fill/stroke + transform omitted-when-identity (use a typed `List` of 16 floats, or a `transform` property the text renderer omits on identity). `Other` commands fall back to a name-only node. The node `name` is the command kind (`"DrawRect"`, `"ClipRect"`, …).

### Layer 3 — `Layer` command-descent (flui-layer)

Extend `Layer::to_diagnostics_node`: a `PictureLayer` gets `children` = one `DrawCommand::to_diagnostics_node()` per command in its `DisplayList`, plus its sublayer children; opaque handles (`TextureId`/`PlatformViewId` → `i64`, `LayerLink` → `u64`) become id properties, never the raw resource. Layer-kind nodes (`OffsetLayer`→`"Offset"` with `dx`/`dy`, `ClipRectLayer`→`"ClipRect"` with `rect`+`clip`, etc.) mirror the existing `write_layer` headers as typed properties.

### Layer 4 — strategies + snapshot + schema (flui-rendering::testing + flui-foundation)

- `fn scene_diagnostics(tree: &LayerTree) -> DiagnosticsNode` walks the painted tree to a node.
- `SnapshotStrategy<DiagnosticsNode, String>::{text, json}` — `.text` = node `Display` (compact tree style); `.json` = `serde_json` of the node. `harness.snapshot()` → `.text`. New `.json` insta test = synthetic inspector.
- `serialize_layer_tree`/`summarize_command` retired → `#[deprecated]` shims for one cycle, then removed; `DrawKind`/`DrawCommandSummary` removed (replaced by Diagnosticable).
- schemars: `DiagnosticsNode`/`DiagnosticsProperty`/`DiagnosticsValue` derive `JsonSchema`; a `DiagnosticsEnvelope { format_version: u32, root: DiagnosticsNode }` (or `format_version` on the snapshot wrapper) carries the version; committed `schema/diagnostics.v1.json` + CI diff-gate.

### Goldens

The six `.snap` are **regenerated** to the `DiagnosticsNode` text format. Acceptance = a reviewer confirms each regenerated golden still asserts the same discriminating facts as its #213 predecessor: clip behavior (`clip_layer`, `lazy_sliver`), per-command paint + the shadow/fill/border ordering (`decorated_box`), the nine distinct per-rect transforms (`lazy_sliver`), opacity (`opacity_layer`). Diff the old vs new golden in review.

## Crate touches

| Crate | Change |
|---|---|
| `flui-foundation` | `DiagnosticsValue`; `DiagnosticsProperty.value` typed + back-compat; typed `DiagnosticsBuilder` methods; `schemars` derive + feature (add dep) |
| `flui-painting` | `impl Diagnosticable for DrawCommand` (31); reuse hoisted fmt helpers in `DiagnosticsValue::Display` if shared |
| `flui-layer` | `Layer::to_diagnostics_node` command-descent + opaque projection |
| `flui-rendering` | `scene_diagnostics` + `SnapshotStrategy`; retire `snapshot.rs` serializer; regenerate goldens; `.json` test |
| `schema/` + CI | `diagnostics.v1.json` + diff-gate |

## Testing

- **Back-compat:** existing `DiagnosticsProperty`/`DiagnosticsNode` doctests + the ~49 impls compile and pass unchanged (the workspace build is the gate for Layer-1).
- **Typed faithfulness:** `add_rect`/`add_color` produce `DiagnosticsValue::Rect`/`Color`; a unit test asserts the serde JSON carries full-precision numerics where the text shows 2-dec.
- **Painted tree:** a small `LayerTree` → `scene_diagnostics` has the expected command child nodes with typed properties.
- **Goldens:** regenerated 6 `.snap` (reviewed for content-equivalence) + a new `.json` synthetic-inspector snapshot.
- **Schema gate:** CI regenerates `diagnostics.v1.json` and fails on unreviewed diff.

## Out of scope

AccessKit `TreeUpdate` live wire (roadmap B — will reuse this node tree); pixel/SVG/trace goldens; a `flui-testing` umbrella.
