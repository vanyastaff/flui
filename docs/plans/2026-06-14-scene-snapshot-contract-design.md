# Painted-scene introspection & serialization contract — design spec

> Decision record: [`ADR-0004`](../adr/ADR-0004-painted-scene-introspection-contract.md). This spec is the implementation-facing design; the *why* (and the rejected alternatives) live in the ADR. Implementation plan: `2026-06-14-scene-snapshot-contract-plan.md`.

## Summary

Give the painted scene (composited `LayerTree` + per-`PictureLayer` `DrawCommand` stream) a **curated, faithfully-typed IR** (`SceneSnapshot`) that:

1. holds **faithful** values (full-precision `f32`/`Color`/`Rect`) — normalization is a *renderer* concern, not baked into the data;
2. is rendered to any output through **pluggable snapshot-strategy witnesses** (`SnapshotStrategy<Scene, Format>`): `.text` reproduces the PR #213 insta golden **byte-for-byte**, `.json` emits the faithful inspector form;
3. commits a **versioned, CI-gated JSON Schema** (`schemars`) as the long-term, language-agnostic contract — `format_version` + `#[non_exhaustive]`.

It is a **sibling** to the existing `DiagnosticsNode` (render-object introspection), not a replacement. The AccessKit live-inspector wire is designed-for but **deferred to roadmap B**; pixel/SVG/trace goldens and a `flui-testing` umbrella are out of scope.

## Context (verified substrate)

- `DrawCommandSummary { kind: DrawKind, line: String }`, `DrawKind`, and the whole normalized walk live in `crates/flui-rendering/src/testing/snapshot.rs` (PR #213), behind the `testing` surface. Six committed `.snap` files define the `.text` format.
- `DrawCommand`/`DisplayList` derive `serde` (feature-gated) in `flui-painting`. `Layer` does not — and under this design it never will (the IR *projects* it).
- `DiagnosticsNode` (`flui-foundation`) is the render-object IR; it provably cannot hold the painted command stream (no `impl Diagnosticable for DrawCommand`; `DisplayList`/`Layer` emit `commands: <len>` only). See ADR-0004 Context.
- `flui-layer` depends on `flui-painting` (so `flui-layer` can name `DrawCommandSummary` once it moves to `flui-painting`); `flui-rendering` depends on both.

## Goals / Non-goals

**Goals**
- One faithful IR for the painted scene, rendered to text (tests) + JSON (inspector) + Debug from one source.
- Preserve the six PR #213 `.snap` files **byte-for-byte** (the `.text` strategy is the new producer; `cargo insta` is the gate).
- Commit a versioned JSON Schema as the durable, cross-language contract.
- Renderers extensible without touching the IR (witness pattern).

**Non-goals (this spec)**
- AccessKit `TreeUpdate` wire (roadmap B; IR kept compatible).
- Pixel / SVG goldens; a frame-trace artifact (possible debug-only devtools feature later).
- A `flui-testing` re-export umbrella (rejected — ADR-0004).
- Changing `DiagnosticsNode`'s role (only the `#[non_exhaustive]` hygiene gap is closed).

## Design

### Layer 1 — the IR (faithful, curated)

Types are illustrative; the authoritative surface is fixed by the API-GATE in the plan.

```rust
// flui-painting — moved here from flui-rendering::testing, now NON-test, serde-gated.
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DrawCommandSummary {
    Rect { rect: RectF, paint: PaintSummary, transform: Option<Mat4> },
    RRect { rrect: RRectF, paint: PaintSummary, transform: Option<Mat4> },
    DRRect { outer: RRectF, inner: RRectF, paint: PaintSummary, transform: Option<Mat4> },
    Shadow { rrect: RRectF, color: ColorRgba, elevation: f32 },
    Image { dst: RectF, /* opaque-handle id, never the texture */ image: ImageRef },
    Text { bounds: RectF, text: String, /* style summary */ },
    ClipRect { rect: RectF, op: ClipOp, behavior: ClipBehavior, transform: Option<Mat4> },
    ClipRRect { rrect: RRectF, op: ClipOp, behavior: ClipBehavior, transform: Option<Mat4> },
    ClipPath { bounds: RectF, point_count: usize, op: ClipOp, behavior: ClipBehavior },
    // … one arm per DrawCommand variant; faithful fields, NO pre-formatted String.
    Other,
}

#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DrawKind { Rect, RRect, Clip, Shadow, Image, Text, Layer, /* … */ Other }

#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PaintSummary { pub style: PaintStyleSummary, pub color: ColorRgba, pub stroke_width: Option<f32> }
```

```rust
// flui-layer — new.
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SceneSnapshot {
    pub format_version: u32,            // stability anchor, starts at 1
    pub root: Option<LayerSummary>,
}

#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LayerSummary {
    pub kind: &'static str,             // "Picture", "Offset", "ClipRect", …
    pub descriptor: LayerDescriptor,    // variant-specific, serde-safe
    pub commands: Vec<DrawCommandSummary>, // non-empty only for Picture layers
    pub children: Vec<LayerSummary>,
}

#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum LayerDescriptor {
    Picture { bounds: RectF },
    Offset { dx: f32, dy: f32 },
    Transform { matrix: Mat4 },
    Opacity { alpha: f32 },
    ClipRect { rect: RectF, behavior: ClipBehavior },
    ClipRRect { rrect: RRectF, behavior: ClipBehavior },
    ClipPath { bounds: RectF, point_count: usize, behavior: ClipBehavior },
    Texture { id: i64, bounds: RectF },          // opaque id, never the GPU resource
    PlatformView { id: i64, bounds: RectF },
    ShaderMask { bounds: RectF, blend: &'static str, shader_kind: &'static str },
    Leader { link_id: u64 }, Follower { link_id: u64 },
    // … one per Layer variant.
    Other,
}
```

**Faithful, not normalized.** `RectF`/`RRectF`/`Mat4`/`ColorRgba` carry full-precision values. The 2-decimal/`#RRGGBBAA`/identity-omit normalization is applied **only** by the `.text` renderer (Layer 3). This is the single change that makes serde JSON useful to an inspector (full precision) while keeping `.text` stable — and it is why the IR is rich, not `line: String` (ADR-0004 rejected the thin form).

**GPU/opaque handles** are projected to ids/descriptors at the `LayerDescriptor` boundary; the raw resource never enters the IR. If a real opaque handle (e.g. a `wgpu` view) ever lands in `Layer`, it is projected to an identifier-only descriptor — never surfaced.

### Layer 2 — schema commitment & versioning

- `#[non_exhaustive]` on every IR type above, plus close the pre-existing gaps on `DrawKind`, `DiagnosticsNode`, `DiagnosticsProperty`.
- `format_version: u32` on `SceneSnapshot` (start `1`).
- Derive `schemars::JsonSchema` (feature-gated, same gate as serde) on the IR. A small xtask/bin generates the Draft-2020-12 schema to `schema/scene-snapshot.v1.json`, **checked in**. A CI step regenerates and `diff`s it; an unreviewed diff fails the build (schemars output is not semver-stable, so *we* own the gate — see ADR-0004 References, Leventhal).
- **Serialization context (`output_mode`).** Multi-consumer shaping (test-curated vs inspector-faithful) is threaded through a root-level mode, not per-field `skip_if`. v1 ships a single faithful mode; the mechanism (a `SnapshotContext { output_mode, format_version }` wrapper / custom root `Serialize`) is in place so a future curated-inspector mode is additive.
- Additive-evolution rules (recorded in ADR-0004 §Decision 2): new variant / new `#[non_exhaustive]` field = minor; `.text` format or field-type change = major + `format_version` bump.

### Layer 3 — pluggable snapshot-strategy witnesses

```rust
// flui-rendering::testing (or a shared snapshot util) — the renderer layer.
pub struct SnapshotStrategy<Value, Format> {
    pub render: fn(&Value) -> Format,
    // composition via contramap: (A)->B + Strategy<B,F> => Strategy<A,F>
}

impl SnapshotStrategy<SceneSnapshot, String> {
    pub fn text() -> Self;   // reproduces the PR #213 golden BYTE-FOR-BYTE
    pub fn json() -> Self;   // faithful serde_json (the inspector form)
}
```

- `.text` is the **only** place normalization lives (2-dec via the hoisted `f`/`hex_color`/`fmt_rect` helpers, identity-omit, clip-behaviour names, deterministic walk). It must produce the existing `.snap` bytes exactly.
- `.json` serializes the faithful IR via `serde_json` and is exercised by its **own** insta snapshot test — that test is the *synthetic inspector consumer* that de-risks the serde/schema path with no `flui-devtools` dependency.
- `.accesskit` / `.pixel` / `.svg` are future strategies; adding one touches only the strategy layer, never the IR.

### Crate topology & moves

| Type | From | To | Gate |
|---|---|---|---|
| `DrawCommandSummary`, `DrawKind`, `PaintSummary` | `flui-rendering::testing::snapshot` | **`flui-painting`** (non-test) | `serde`/`schemars` feature |
| `SceneSnapshot`, `LayerSummary`, `LayerDescriptor` | — (new) | **`flui-layer`** | `serde`/`schemars` feature |
| `f()`, `hex_color()`, `fmt_rect()`, … | `snapshot.rs` (private) | **`flui-painting`** formatting util | — |
| `SnapshotStrategy`, `.text`/`.json` | new | `flui-rendering::testing` | `cfg(test)`/`testing` |

No new crate. No `flui-testing` umbrella. The **inspection contract** (IR + schema) is producer-crate, always-available types with feature-gated serde; the **test harness** stays per-crate behind `cfg(test)`/`feature = "testing"`.

### Migration from PR #213

- `snapshot.rs` is restructured: `serialize_layer_tree(tree)` → `SceneSnapshot::from_layer_tree(tree)` + `.text` strategy. The free fns `snapshot_tree` / `commands_of` keep working (re-expressed over the IR).
- `serialize_layer_tree` becomes a `#[deprecated(note = "use SceneSnapshot + SnapshotStrategy::text")]` shim for one cycle, then removed.
- The six `.snap` files **do not change**; `cargo insta` is the acceptance gate for the `.text` strategy.

## Public API surface (additions)

- `flui-painting`: `DrawCommandSummary`, `DrawKind`, `PaintSummary` (+ summary scalar types), all `#[non_exhaustive]`, serde/schemars feature-gated. Formatting util (internal).
- `flui-layer`: `SceneSnapshot`, `LayerSummary`, `LayerDescriptor`, `SceneSnapshot::from_layer_tree(&LayerTree)`, `impl Display for SceneSnapshot` (delegates to `.text`).
- `flui-rendering::testing`: `SnapshotStrategy<Scene, Format>`, `SnapshotStrategy::{text, json}`, the `assert_*` helpers re-expressed over the IR; `serialize_layer_tree` deprecated.
- Committed artifact: `schema/scene-snapshot.v1.json` + CI diff-gate.

## Testing strategy

- **Golden preservation:** the six existing `.snap` pass unchanged via `.text` (`cargo insta`). This is the hard gate for every IR/renderer change.
- **Synthetic inspector:** a new `.json` insta snapshot over the dogfood subjects (decorated box, clip, opacity, lazy sliver) — proves the faithful serde form and locks the schema.
- **Round-trip:** if `Deserialize` is derived, a `serde_json` round-trip stability test (`Scene → json → Scene → json` idempotent).
- **Schema gate:** CI regenerates `scene-snapshot.v1.json` and fails on unreviewed diff.
- **Faithful-vs-normalized:** a unit test asserting `.json` carries full-precision values where `.text` shows 2-dec (proves normalization is renderer-only, not in the data).

## Risks & mitigations

- **`.text` drift during the move** → `cargo insta` byte-gate on every step; restructure in small, test-green commits.
- **schemars output churn** → we own the gate by committing + diffing the generated schema; pin `schemars`.
- **IR maintenance cost (one arm per `DrawCommand`/`Layer` variant)** → accepted (ADR-0004); the `#[non_exhaustive]` `Other` arm absorbs additions without a breaking change, and the four roadmap formats amortize the witness layer.
- **Scope creep into AccessKit/pixel** → explicitly deferred; the IR is designed compatible but the wire is roadmap B.

## Roadmap placement

This is the serialization/introspection backbone of render-harness 2.0. It supersedes the "just hoist helpers" minimal option and slots before roadmap B (semantics/AccessKit), which will reuse the IR's tree shape for the live-inspector `TreeUpdate` wire.

## Resolved decisions

- Sibling curated IR, not `DiagnosticsNode` unification, not raw `Layer` serde — refuted at source (ADR-0004).
- Rich faithful IR, not thin `line: String` — normalization in the renderer.
- Committed `schemars` JSON Schema as the contract; `format_version` + `#[non_exhaustive]`.
- Pluggable `SnapshotStrategy` witnesses; `.text` = byte-identical #213, `.json` = synthetic inspector.
- IR in producer crates; no umbrella; AccessKit deferred to roadmap B.
