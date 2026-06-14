# Painted-Scene Introspection & Serialization Contract — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the painted scene a curated, faithfully-typed, serde-capable IR (`SceneSnapshot`) rendered through pluggable snapshot-strategy witnesses — `.text` reproducing the six PR #213 `.snap` byte-for-byte, `.json` the faithful inspector form — with a committed, CI-gated JSON Schema as the years-long contract.

**Architecture:** Three layers (ADR-0004). Layer 1 — the IR: `DrawCommandSummary`/`DrawKind`/`PaintSummary` in `flui-painting` and `SceneSnapshot`/`LayerSummary`/`LayerDescriptor` in `flui-layer`, holding *faithful* typed values (full-precision `f32`/`Color`/`Rect`). Layer 2 — a `schemars`-generated, checked-in, CI-diff-gated JSON Schema with `format_version` + `#[non_exhaustive]`. Layer 3 — `SnapshotStrategy<Scene, Format>` witnesses in `flui-rendering::testing`; normalization (2-dec, identity-omit, `#RRGGBBAA`) lives only in the `.text` renderer, so serde JSON stays faithful.

**Tech Stack:** Rust (edition 2024), `serde` (feature-gated), `schemars` (new, vetted), `insta` (existing, `= "1"`), `cargo insta` for golden review.

**Authoritative in-repo sources (READ before implementing — they ARE the spec):**
- `crates/flui-rendering/src/testing/snapshot.rs` — the PR #213 serializer. `summarize_command` (31 arms) defines the curated per-command field-set and format; `write_layer` (19 arms) + `serialize_layer_tree` define the layer-tree walk and indentation. **The rich IR mirrors these arms one-for-one; the byte-exact `.snap` files are the gate.**
- `crates/flui-painting/src/display_list/command.rs:44` — `DrawCommand` (31 variants, fields).
- `crates/flui-layer/src/layer/mod.rs:187` — `Layer` (19 variants); per-variant payload structs in `crates/flui-layer/src/layer/*.rs` (accessors like `clip_rect()`, `clip_behavior()`, `clip_rrect()`, `alpha()`, `dx()/dy()`).
- `crates/flui-rendering/tests/snapshots/harness_snapshot__*.snap` — the six committed goldens (5 scene + 1 smoke string). These MUST NOT change.

**Scope (ADR-0004 layers 1-3).** Out of scope: AccessKit `TreeUpdate` wire (roadmap B), pixel/SVG/trace goldens, a `flui-testing` umbrella.

**Complete variant inventories (so the IR enums are exhaustive):**
- `DrawCommand` (31): `ClipRect, ClipRRect, ClipRSuperellipse, ClipPath, DrawLine, DrawRect, DrawRRect, DrawCircle, DrawOval, DrawPath, DrawText, DrawTextSpan, DrawImage, DrawImageRepeat, DrawImageNineSlice, DrawImageFiltered, DrawTexture, DrawShadow, DrawGradient, DrawGradientRRect, ShaderMask, BackdropFilter, DrawArc, DrawDRRect, DrawPoints, DrawVertices, DrawColor, DrawPaint, DrawAtlas, SaveLayer, RestoreLayer`.
- `DrawKind` (15, existing): `Rect, RRect, Circle, Oval, Path, Line, Arc, DRRect, Clip, Text, Image, Shadow, Gradient, Layer, Other`.
- `Layer` (19): `Canvas, Picture, Texture, PlatformView, PerformanceOverlay, ClipRect, ClipRRect, ClipPath, ClipSuperellipse, Offset, Transform, Opacity, ColorFilter, ImageFilter, ShaderMask, BackdropFilter, Leader, Follower, AnnotatedRegion`.

---

## File Structure

| File | Responsibility |
|---|---|
| `Cargo.toml` (root) | add `schemars` to `[workspace.dependencies]` |
| `crates/flui-painting/Cargo.toml` | add `schemars` optional dep + `schemars` feature wiring |
| `crates/flui-painting/src/display_list/summary.rs` (new) | the painted-command IR: `DrawCommandSummary`, `DrawKind`, `PaintSummary`, `summarize_command`, `Display`; the hoisted `fmt` helpers |
| `crates/flui-painting/src/display_list/mod.rs` | `pub mod summary;` + re-exports |
| `crates/flui-layer/Cargo.toml` | add `serde` + `schemars` optional deps + features (currently has neither) |
| `crates/flui-layer/src/snapshot.rs` (new) | `SceneSnapshot`, `LayerSummary`, `LayerDescriptor`, `SceneSnapshot::from_layer_tree`, `Display`, `to_json`/`to_json_with` |
| `crates/flui-layer/src/lib.rs` | `pub mod snapshot;` + re-exports |
| `crates/flui-foundation/src/debug.rs` | `#[non_exhaustive]` on `DiagnosticsNode`, `DiagnosticsProperty` |
| `crates/flui-rendering/src/testing/snapshot.rs` | shrink to: `SnapshotStrategy`, `.text`/`.json`, re-exports; `serialize_layer_tree` → `#[deprecated]` shim |
| `crates/flui-rendering/tests/harness_snapshot.rs` | unchanged behavior; add a `.json` synthetic-inspector test |
| `schema/scene-snapshot.v1.json` (new) | committed JSON Schema |
| `xtask` or a gen test | regenerate + diff-gate the schema |
| `.github/workflows/*.yml` | schema diff-gate step |

---

## Task 0: Dependencies & features

**Files:**
- Modify: `Cargo.toml` (root `[workspace.dependencies]`, after line 165 `insta = "1"`)
- Modify: `crates/flui-painting/Cargo.toml` (`[features]` line 32 `serde = [...]`, `[dependencies]`)
- Modify: `crates/flui-layer/Cargo.toml` (`[features]` line 11, `[dependencies]` line 20)

- [ ] **Step 1: Vet `schemars`**

Confirm the latest `schemars` 1.x version, license (MIT), and MSRV against the workspace. Read `crates/flui-painting/Cargo.toml` to confirm `serde` is already optional there.

Run: `cargo info schemars`
Expected: a 1.x version, MIT license.

- [ ] **Step 2: Add `schemars` to workspace deps**

In root `Cargo.toml` under `[workspace.dependencies]`, add (pin to the vetted 1.x):

```toml
schemars = { version = "1", default-features = false, features = ["derive"] }
```

- [ ] **Step 3: Wire `schemars` into `flui-painting`**

In `crates/flui-painting/Cargo.toml` `[dependencies]`:

```toml
schemars = { workspace = true, optional = true }
```

In `[features]`, extend the existing `serde` feature line OR add a sibling so the IR can derive `JsonSchema` only when wanted:

```toml
schemars = ["dep:schemars", "serde", "flui-types/serde"]
```

- [ ] **Step 4: Wire `serde` + `schemars` into `flui-layer` (currently has neither)**

In `crates/flui-layer/Cargo.toml` `[dependencies]`:

```toml
serde = { workspace = true, optional = true }
schemars = { workspace = true, optional = true }
```

In `[features]`:

```toml
serde = ["dep:serde", "flui-painting/serde", "flui-types/serde"]
schemars = ["dep:schemars", "serde", "flui-painting/schemars"]
```

- [ ] **Step 5: Verify the workspace builds with the new features**

Run: `cargo check -p flui-painting --features schemars && cargo check -p flui-layer --features schemars`
Expected: clean (no types use the features yet; this only proves the dependency graph resolves).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/flui-painting/Cargo.toml crates/flui-layer/Cargo.toml Cargo.lock
git commit -m "build(painting,layer): add schemars + flui-layer serde for the scene-snapshot IR"
```

---

## Task 1: Close `#[non_exhaustive]` hygiene gaps

**Files:**
- Modify: `crates/flui-foundation/src/debug.rs` (`DiagnosticsNode` ≈582, `DiagnosticsProperty` ≈318)

`DrawCommand` and `Layer` are already correct (`DrawCommand` is `#[non_exhaustive]`; `Layer` is intentionally exhaustive — a new variant is meant to be a compile error per the snapshot.rs contract comment, and the IR projects it). `DrawKind` gains `#[non_exhaustive]` in Task 3 when it moves. This task closes the two foundation gaps.

- [ ] **Step 1: Add the attribute**

Add `#[non_exhaustive]` immediately above `pub struct DiagnosticsNode {` and `pub struct DiagnosticsProperty {`.

- [ ] **Step 2: Fix any in-crate struct-literal construction**

`#[non_exhaustive]` forbids struct-literal construction *outside* the crate, but in-crate is fine. Search for external construction:

Run: `rg "DiagnosticsNode \{|DiagnosticsProperty \{" crates --glob '!crates/flui-foundation/**'`
Expected: no struct-literal hits outside `flui-foundation` (constructors like `DiagnosticsNode::new`/`DiagnosticsBuilder` are used instead). If any exist, switch them to the constructor.

- [ ] **Step 3: Verify**

Run: `cargo build -p flui-foundation && cargo build -p flui-rendering`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add crates/flui-foundation/src/debug.rs
git commit -m "refactor(foundation): #[non_exhaustive] on DiagnosticsNode/DiagnosticsProperty for additive evolution"
```

---

## Task 2: Hoist the formatting helpers into `flui-painting`

The private helpers in `snapshot.rs` (`f`, `hex_color`, `fmt_rect`, `fmt_rrect`, `fmt_point`, `fmt_clip_op`, `fmt_clip`, `summarize_paint`, `maybe_transform`) format `flui-painting` types and must be shared by the IR's `Display` impls. Move them to `flui-painting`, keeping behavior byte-identical.

**Files:**
- Create: `crates/flui-painting/src/display_list/summary.rs` (start it here; grows in Task 3)
- Modify: `crates/flui-painting/src/display_list/mod.rs` (`pub mod summary;`)
- Modify: `crates/flui-rendering/src/testing/snapshot.rs` (delete the local helpers, import from `flui_painting`)

- [ ] **Step 1: Write a failing test for the hoisted helpers**

In `crates/flui-painting/src/display_list/summary.rs`:

```rust
//! Painted-command introspection IR + stable text formatting.
//! The `fmt::*` helpers are the single source of the normalized text format
//! (2-decimal floats, `#RRGGBBAA`, identity-transform omitted) shared by the
//! `Display` impls of the scene-snapshot IR.

#[cfg(test)]
mod tests {
    use super::fmt::{f, hex_color};
    use flui_types::styling::Color;

    #[test]
    fn float_is_two_decimals_and_normalizes_negative_zero() {
        assert_eq!(f(-0.0), "0.00");
        assert_eq!(f(1.5), "1.50");
    }

    #[test]
    fn color_is_uppercase_rrggbbaa() {
        assert_eq!(hex_color(Color::from_rgba_u8(255, 0, 0, 255)), "#FF0000FF");
    }
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p flui-painting summary::tests`
Expected: FAIL — `module `fmt` not found`.

- [ ] **Step 3: Move the helpers verbatim into a `fmt` submodule**

Copy the helper bodies from `snapshot.rs` (lines ≈70-180: `f`, `hex_color`, `fmt_rect`, `fmt_rrect`, `fmt_point`, `fmt_clip_op`, `fmt_clip`, `summarize_paint`, `maybe_transform`) into a `pub(crate) mod fmt` in `summary.rs`, keeping the `debug_assert!(v.is_finite(), …)` in `f`. Adjust imports to `flui-painting`'s own paths (`crate::display_list::{ClipOp, Paint}`, `flui_types::…`, `flui_types::painting::Clip`). Make each helper `pub(crate)`.

Add `pub mod summary;` to `crates/flui-painting/src/display_list/mod.rs`.

- [ ] **Step 4: Verify the helper test passes**

Run: `cargo test -p flui-painting summary::tests`
Expected: PASS (2 tests).

- [ ] **Step 5: Point `snapshot.rs` at the hoisted helpers**

In `crates/flui-rendering/src/testing/snapshot.rs`, delete the local helper fns and `use flui_painting::display_list::summary::fmt::{…}` (re-export them `pub(crate)` from `flui-painting` if the paths need widening). Keep `summarize_command`/`write_layer` calling them.

- [ ] **Step 6: Verify the goldens are byte-identical**

Run: `cargo test -p flui-rendering --test harness_snapshot`
Expected: PASS — all 6 `.snap` unchanged (no `.snap.new` produced).

- [ ] **Step 7: Commit**

```bash
git add crates/flui-painting/src/display_list/summary.rs crates/flui-painting/src/display_list/mod.rs crates/flui-rendering/src/testing/snapshot.rs
git commit -m "refactor(painting): hoist snapshot fmt helpers into flui-painting (shared by the IR)"
```

---

## Task 3: Rich painted-command IR in `flui-painting`

Replace `DrawCommandSummary { kind, line: String }` with a **faithful, typed** enum (one arm per the 31 `summarize_command` arms), move `DrawKind` here, and split formatting into `Display`. `summarize_command` becomes IR-building; the per-command line moves into `Display`.

**Files:**
- Modify: `crates/flui-painting/src/display_list/summary.rs`
- Modify: `crates/flui-rendering/src/testing/snapshot.rs` (re-export from `flui-painting`; delete the moved types)

- [ ] **Step 1: Write the failing byte-exact test (the gate)**

In `summary.rs` `#[cfg(test)] mod tests`, add tests that pin the exact lines the `.snap` files show. Use the real `DrawCommand`:

```rust
#[test]
fn draw_rect_line_matches_golden() {
    use crate::display_list::{DrawCommand, Paint};
    use flui_types::{geometry::{Matrix4, Rect, Pixels}, styling::Color};
    let cmd = DrawCommand::DrawRect {
        rect: Rect::from_ltwh(Pixels::new(0.0), Pixels::new(0.0), Pixels::new(40.0), Pixels::new(40.0)),
        paint: std::sync::Arc::new(Paint::fill(Color::from_rgba_u8(255, 0, 0, 255))),
        transform: Matrix4::IDENTITY,
    };
    let summary = summarize_command(&cmd);
    assert_eq!(summary.kind(), DrawKind::Rect);
    assert_eq!(summary.to_string(), "DrawRect rect=(0.00,0.00 40.00x40.00) fill #FF0000FF");
}

#[test]
fn draw_drrect_line_matches_decorated_box_golden() {
    // Mirror the exact bytes from harness_snapshot__decorated_box.snap line 3:
    // "DrawDRRect outer=(0.00,0.00 80.00x60.00 r=0.00/0.00/0.00/0.00) inner=(2.00,2.00 76.00x56.00 r=0.00/0.00/0.00/0.00) fill #000000FF"
    // Construct the DrawDRRect command per command.rs:357 fields and assert to_string().
    // (Build outer/inner RRect from rects with zero radii; fill black.)
}
```

(The second test body is filled in once `DrawDRRect`'s exact fields are read from `command.rs:357`. The golden line to reproduce is quoted above verbatim.)

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p flui-painting summary::tests::draw_rect_line_matches_golden`
Expected: FAIL — `summarize_command`/`DrawCommandSummary`/`DrawKind` not yet in this crate.

- [ ] **Step 3: Define `DrawKind` (moved, `#[non_exhaustive]`) and the rich `DrawCommandSummary`**

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum DrawKind { Rect, RRect, Circle, Oval, Path, Line, Arc, DRRect, Clip, Text, Image, Shadow, Gradient, Layer, Other }

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum DrawCommandSummary {
    Rect { rect: RectXywh, paint: PaintSummary, transform: Option<[f32; 16]> },
    DRRect { outer: RRectSummary, inner: RRectSummary, paint: PaintSummary, transform: Option<[f32; 16]> },
    Shadow { path_bounds: RectXywh, color: [u8; 4], elevation: f32, transform: Option<[f32; 16]> },
    ClipRect { rect: RectXywh, op: ClipOpKind, behavior: ClipBehaviorKind, transform: Option<[f32; 16]> },
    // … one arm for every DrawCommand variant currently handled in the snapshot.rs `summarize_command`
    //   (31 total: see the inventory in the plan header). Each arm carries the SAME curated fields the
    //   existing arm formats, but as faithful typed values (full-precision f32 in RectXywh/RRectSummary,
    //   raw [u8;4] color, raw [f32;16] transform) — NEVER a pre-formatted String.
    Other { kind_hint: &'static str },
}
```

Define the small faithful field types used above (all serde+schemars gated): `RectXywh { x: f32, y: f32, w: f32, h: f32 }`, `RRectSummary { rect: RectXywh, radii: [f32; 4] }`, `PaintSummary { style: PaintStyleKind, color: [u8; 4], stroke_width: Option<f32> }`, plus `ClipOpKind`/`ClipBehaviorKind`/`PaintStyleKind` mirror enums (so the IR does not leak non-serde `flui-painting` painting enums; map from them in `summarize_command`).

Add `pub fn kind(&self) -> DrawKind` returning the existing classification (port from the current `DrawCommandSummary.kind` assignments).

- [ ] **Step 4: Port `summarize_command` to build the IR**

Move `summarize_command(cmd: &DrawCommand) -> DrawCommandSummary` into `summary.rs`. For each of the 31 arms in the current `snapshot.rs::summarize_command`, replace the `format!("…")` `line` with the typed variant, extracting the SAME fields into faithful values (e.g. `RectXywh { x: rect.left().get(), … }`, `color: [c.r, c.g, c.b, c.a]`, `transform: (!t.is_identity()).then(|| t.to_array())`). Keep the `_ =>` catch-all → `DrawCommandSummary::Other { kind_hint }`.

- [ ] **Step 5: Implement `Display` reproducing the exact current lines**

```rust
impl core::fmt::Display for DrawCommandSummary {
    fn fmt(&self, out: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use fmt::*;
        match self {
            Self::Rect { rect, paint, transform } =>
                write!(out, "DrawRect rect={} {}{}", fmt_rect_xywh(rect), summarize_paint_line(paint), maybe_transform_arr(transform)),
            // … one arm per variant, porting the EXACT format string from the current snapshot.rs arm,
            //   now reading the typed fields and applying fmt::* normalization (2-dec, #RRGGBBAA, identity-omit).
            _ => Ok(()),
        }
    }
}
```

The `fmt::*` helpers from Task 2 are adapted to take the faithful field types (e.g. `fmt_rect_xywh(&RectXywh)`, `maybe_transform_arr(&Option<[f32;16]>)`). Normalization lives HERE only.

- [ ] **Step 6: Verify the byte-exact tests pass**

Run: `cargo test -p flui-painting summary::tests`
Expected: PASS — `draw_rect_line_matches_golden` and the DRRect golden line match exactly.

- [ ] **Step 7: Re-export from `snapshot.rs`, delete the moved definitions**

In `snapshot.rs`, delete the old `DrawCommandSummary`/`DrawKind`/`summarize_command` and `pub use flui_painting::display_list::summary::{DrawCommandSummary, DrawKind, summarize_command};`. `write_layer`/`serialize_layer_tree` now call the re-exported `summarize_command` and `DrawCommandSummary::to_string()` for the per-command line.

- [ ] **Step 8: Verify goldens unchanged**

Run: `cargo test -p flui-rendering --test harness_snapshot && cargo test -p flui-rendering --lib snapshot`
Expected: PASS — 6 `.snap` byte-identical; lib snapshot unit tests green.

- [ ] **Step 9: Commit**

```bash
git add crates/flui-painting/src/display_list/summary.rs crates/flui-rendering/src/testing/snapshot.rs
git commit -m "feat(painting): faithful typed DrawCommandSummary IR (normalization in Display, not data)"
```

---

## Task 4: `SceneSnapshot` / `LayerSummary` / `LayerDescriptor` in `flui-layer`

The painted-tree IR. Projects the `LayerTree` into faithful typed nodes; `Display` reproduces the current tree text.

**Files:**
- Create: `crates/flui-layer/src/snapshot.rs`
- Modify: `crates/flui-layer/src/lib.rs` (`pub mod snapshot;`)
- Modify: `crates/flui-rendering/src/testing/snapshot.rs` (`serialize_layer_tree` delegates here — Task 5)

- [ ] **Step 1: Write the failing byte-exact tree test**

In `crates/flui-layer/src/snapshot.rs` tests, build a small `LayerTree` (an `OffsetLayer` over a `PictureLayer` with one `DrawRect`) and assert `SceneSnapshot::from_layer_tree(&tree).to_string()` equals the exact `colored_box.snap` body:

```
Offset dx=0.00 dy=0.00
  Picture bounds=(0.00,0.00 40.00x40.00)
    DrawRect rect=(0.00,0.00 40.00x40.00) fill #FF0000FF
```

(Use the existing `flui-layer::testing` builders to construct the tree; read `crates/flui-layer/src/testing/` for the helpers.)

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p flui-layer snapshot::tests`
Expected: FAIL — `SceneSnapshot` not defined.

- [ ] **Step 3: Define the IR types**

```rust
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SceneSnapshot { pub format_version: u32, pub root: Option<LayerSummary> }

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct LayerSummary {
    pub kind: &'static str,
    pub descriptor: LayerDescriptor,
    pub commands: Vec<flui_painting::display_list::summary::DrawCommandSummary>,
    pub children: Vec<LayerSummary>,
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum LayerDescriptor {
    Canvas,
    Picture { bounds: [f32; 4] },
    Texture { id: i64, bounds: [f32; 4] },
    PlatformView { id: i64, bounds: [f32; 4] },
    PerformanceOverlay,
    ClipRect { rect: [f32; 4], behavior: &'static str },
    ClipRRect { rect: [f32; 4], radii: [f32; 4], behavior: &'static str },
    ClipPath { bounds: [f32; 4], point_count: usize, behavior: &'static str },
    ClipSuperellipse { rect: [f32; 4], behavior: &'static str },
    Offset { dx: f32, dy: f32 },
    Transform { /* TransformLayer exposes no public matrix getter today; see Display blind-spot note */ },
    Opacity { alpha: f32 },
    ColorFilter, ImageFilter,
    ShaderMask { bounds: [f32; 4] },
    BackdropFilter { bounds: [f32; 4] },
    Leader { link_id: u64 }, Follower { link_id: u64 },
    AnnotatedRegion,
    Other { kind_hint: &'static str },
}
```

The 19 descriptor arms mirror `write_layer` in `snapshot.rs` one-for-one (read it for the exact per-variant accessor calls and header text). Opaque handles project to ids only (`TextureId`/`PlatformViewId` → `i64`, `LayerLink` → `u64`). `format_version` constant `SCENE_SNAPSHOT_FORMAT_VERSION: u32 = 1`.

- [ ] **Step 4: Implement `from_layer_tree` (port `serialize_layer_tree`'s walk)**

Port the deterministic `LayerTree` walk from `snapshot.rs::serialize_layer_tree`/`write_layer`: from the root `LayerId`, build a `LayerSummary` per node (kind string + descriptor via the per-variant accessors + `commands` from `PictureLayer::picture()` mapped through `summarize_command` + recursive `children`). Return `SceneSnapshot { format_version: SCENE_SNAPSHOT_FORMAT_VERSION, root }`.

- [ ] **Step 5: Implement `Display` (port the indentation + headers)**

`impl Display for SceneSnapshot` and a recursive `write_layer_summary(out, &LayerSummary, depth)` reproducing the exact current format: 2-space indent per depth, the layer header line (`Offset dx=… dy=…`, `Picture bounds=(…)`, `ClipRect rect=(…) clip=…`, `Opacity alpha=…`, etc. — byte-identical to `write_layer`), then `commands` one level deeper via `DrawCommandSummary::to_string()`, then children. Keep the `Layer::Transform` blind-spot comment (no public matrix getter — prints bare `Transform`).

- [ ] **Step 6: Verify the byte-exact tree test passes**

Run: `cargo test -p flui-layer snapshot::tests`
Expected: PASS — output equals the `colored_box.snap` body exactly.

- [ ] **Step 7: Commit**

```bash
git add crates/flui-layer/src/snapshot.rs crates/flui-layer/src/lib.rs
git commit -m "feat(layer): SceneSnapshot/LayerSummary/LayerDescriptor IR + faithful from_layer_tree + Display"
```

---

## Task 5: `SnapshotStrategy` witnesses + migrate `snapshot.rs` + deprecate

**Files:**
- Modify: `crates/flui-rendering/src/testing/snapshot.rs` (shrink to strategies + re-exports)
- Modify: `crates/flui-rendering/src/testing/harness.rs` (`snapshot()` uses the IR path)
- Modify: `crates/flui-rendering/tests/harness_snapshot.rs` (add the `.json` synthetic-inspector test)

- [ ] **Step 1: Write the failing strategy tests**

```rust
#[test]
fn text_strategy_reproduces_serialize_layer_tree() {
    // Build a dogfood tree; assert SnapshotStrategy::text().render(&scene)
    // == the legacy serialize_layer_tree(&tree) output, byte-for-byte.
}
#[test]
fn json_strategy_is_faithful_full_precision() {
    // A DrawRect at x=0.333333; assert the .json output contains "0.333333"
    // (faithful) while .text shows "0.33" (normalized) — proves normalization
    // is renderer-only, not in the IR data.
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p flui-rendering --lib snapshot::strategy`
Expected: FAIL — `SnapshotStrategy` not defined.

- [ ] **Step 3: Define `SnapshotStrategy` + built-ins**

```rust
pub struct SnapshotStrategy<Value, Format> { render: fn(&Value) -> Format }
impl<V, F> SnapshotStrategy<V, F> { pub fn render(&self, v: &V) -> F { (self.render)(v) } }

impl SnapshotStrategy<SceneSnapshot, String> {
    pub fn text() -> Self { Self { render: |s| s.to_string() } }
    pub fn json() -> Self { Self { render: |s| s.to_json_pretty() } } // serde_json over the faithful IR
}
```

Add `SceneSnapshot::to_json_pretty(&self) -> String` in `flui-layer` (feature `serde`) using `serde_json` (add `serde_json` dev-or-feature dep as vetted).

- [ ] **Step 4: Rewire the harness + deprecate the shim**

`harness.rs::snapshot()` → `SceneSnapshot::from_layer_tree(&tree).to_string()` (or `SnapshotStrategy::text().render(&scene)`). In `snapshot.rs`, make `serialize_layer_tree` a `#[deprecated(note = "use SceneSnapshot::from_layer_tree + SnapshotStrategy::text")]` shim delegating to the new path. Keep `snapshot_tree`/`commands_of` working over the IR.

- [ ] **Step 5: Add the `.json` synthetic-inspector insta test**

In `tests/harness_snapshot.rs`, add `assert_json_snapshot!`-style tests (or `assert_snapshot!` over `.json()` output) for the 4 dogfood subjects (decorated box, clip, opacity, lazy sliver). Accept them with `cargo insta accept --all` after eyeballing.

- [ ] **Step 6: Verify the six text goldens are STILL byte-identical**

Run: `cargo test -p flui-rendering --test harness_snapshot`
Expected: PASS — the 6 original `.snap` unchanged; the 4 new `__json_*.snap` added.

- [ ] **Step 7: Commit**

```bash
git add crates/flui-rendering/ crates/flui-layer/
git commit -m "feat(rendering): SnapshotStrategy witnesses (.text byte-identical, .json faithful inspector) + deprecate serialize_layer_tree"
```

---

## Task 6: Committed JSON Schema + CI diff-gate

**Files:**
- Create: `schema/scene-snapshot.v1.json`
- Create: a generator (an `xtask` subcommand or a `#[test]` that writes-then-diffs)
- Modify: `.github/workflows/<checks>.yml` (add the gate)

- [ ] **Step 1: Write the failing schema-stability test**

```rust
// crates/flui-layer/tests/schema_stability.rs (feature schemars)
#[test]
fn committed_schema_matches_generated() {
    let generated = serde_json::to_string_pretty(&schemars::schema_for!(flui_layer::snapshot::SceneSnapshot)).unwrap();
    let committed = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schema/scene-snapshot.v1.json")).unwrap();
    assert_eq!(generated.trim(), committed.trim(),
        "scene-snapshot schema drifted — review the diff and, if intended, regenerate the committed schema and bump format_version per ADR-0004");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p flui-layer --features schemars --test schema_stability`
Expected: FAIL — committed schema file missing.

- [ ] **Step 3: Generate + commit the schema**

Add a small generator (xtask `gen-schema` or a one-shot ignored test) that writes `schema/scene-snapshot.v1.json` from `schema_for!(SceneSnapshot)`. Run it; commit the output.

- [ ] **Step 4: Verify the stability test passes**

Run: `cargo test -p flui-layer --features schemars --test schema_stability`
Expected: PASS.

- [ ] **Step 5: Wire the CI gate**

Add a CI step (in the existing `checks` job) that runs the stability test with `--features schemars`, so an unreviewed schema diff fails the build. Document in the test message that schemars output is not semver-stable, so the committed file is the owned gate (ADR-0004 §References, Leventhal).

- [ ] **Step 6: Commit**

```bash
git add schema/scene-snapshot.v1.json crates/flui-layer/tests/schema_stability.rs .github/ xtask/ 2>/dev/null
git commit -m "feat(layer): committed schemars JSON Schema + CI diff-gate (the years-long contract)"
```

---

## Task 7: Serialization-context seam (`format`-mode, single faithful mode v1)

A minimal, real seam so a future curated-inspector projection is a non-breaking addition (ADR-0004 §Decision 2). Not a hollow stub: `#[non_exhaustive]` enum + a `to_json_with` entry whose default IS `to_json`.

**Files:**
- Modify: `crates/flui-layer/src/snapshot.rs`

- [ ] **Step 1: Write the failing seam test**

```rust
#[test]
fn faithful_is_the_default_mode() {
    let scene = /* small SceneSnapshot */;
    assert_eq!(scene.to_json_pretty(), scene.to_json_with(SceneJsonMode::Faithful));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p flui-layer --features serde snapshot::tests::faithful_is_the_default_mode`
Expected: FAIL — `SceneJsonMode` not defined.

- [ ] **Step 3: Add the mode seam**

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SceneJsonMode { #[default] Faithful }

impl SceneSnapshot {
    pub fn to_json_with(&self, mode: SceneJsonMode) -> String {
        match mode { SceneJsonMode::Faithful => self.to_json_pretty() }
    }
}
```

`to_json_pretty` stays the v1 default. A future `Curated` mode is a new `#[non_exhaustive]` variant + a filtering pass — additive.

- [ ] **Step 4: Verify**

Run: `cargo test -p flui-layer --features serde snapshot::tests::faithful_is_the_default_mode`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/flui-layer/src/snapshot.rs
git commit -m "feat(layer): SceneJsonMode seam (faithful v1) for additive output-mode evolution"
```

---

## Task 8: Final gates

- [ ] **Step 1: Full workspace verification**

Run: `cargo fmt --all -- --check && cargo clippy -p flui-painting -p flui-layer -p flui-rendering --all-targets --all-features -- -D warnings && cargo test -p flui-painting -p flui-layer -p flui-rendering && bash scripts/port-check.sh`
Expected: all clean; the 6 original `.snap` byte-identical; new `.json` + schema tests green; port-check exit 0.

- [ ] **Step 2: Confirm the deprecation surface**

Run: `rg "serialize_layer_tree" crates --glob '*.rs'`
Expected: only the `#[deprecated]` shim definition + its delegation; no live internal callers.

- [ ] **Step 3: Commit any fmt/clippy fixes**

```bash
git add -A && git commit -m "chore(scene-snapshot): fmt/clippy/port-check green"
```

---

## Self-review notes (author)

- **Spec coverage:** Task 0 = deps/features; 1 = non_exhaustive gaps; 2 = hoist helpers; 3 = layer-1 command IR; 4 = layer-1 scene IR; 5 = layer-3 strategies + migration + deprecate; 6 = layer-2 schema + CI gate; 7 = serialization-context seam; 8 = gates. All seven design-spec scope points covered.
- **Byte-exact discipline:** every IR/renderer task gates on the unchanged six `.snap` via `cargo insta`; the `.text` format never changes — only its producer moves.
- **No-placeholder caveat:** the 31 `DrawCommand` arms and 19 `Layer` arms are not transcribed individually; the authoritative source is the existing `summarize_command`/`write_layer` (the implementer reads them) and the byte-exact `.snap` gate proves completeness. Representative arms (`DrawRect`, `DrawDRRect`, `ClipRect`, `Offset`, `Picture`, `Opacity`) are shown fully; the inventory tables guarantee enumeration.
- **Faithful-vs-normalized invariant** is itself tested (Task 5 Step 1 `json_strategy_is_faithful_full_precision`) so the load-bearing decision can't silently regress.
- **Verify-against-substrate** is built into Tasks 0/3/4 (read `command.rs`, `layer/mod.rs`, the per-variant layer structs, and the `.snap` bytes before writing).
