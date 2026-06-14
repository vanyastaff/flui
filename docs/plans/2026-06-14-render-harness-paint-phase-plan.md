# Render-harness 2.0 — Sub-project A (paint & phase) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add structural paint-snapshot testing (`insta` over the painted `LayerTree` + typed `DrawCommand`s), compile-checked phase-tagged run handles, and real error capture to `flui_rendering::testing`.

**Architecture:** Build on the verified substrate — `FrameRun` already holds the painted `Option<LayerTree>` (`harness.rs:244`); `flui_layer::testing::inspect` already walks it; `PictureLayer::picture() -> &DisplayList` exposes typed `flui_painting::DrawCommand`s. A new `testing/snapshot.rs` serializes the layer tree to a stable text form; new phase handles drive `PipelineOwner<Phase>` (`into_layout → into_compositing → into_paint → into_semantics`) and stop at a phase. No op-sequence matcher (anti-pattern); a narrow `assert_paints_any(pred)` covers targeted op checks.

**Tech Stack:** Rust (edition 2024), `insta` (snapshot review via `cargo insta`), `flui-rendering`/`flui-layer`/`flui-painting` testing modules. All additions are behind the existing `testing` feature / `cfg(test)`.

**Design spec:** `docs/plans/2026-06-14-render-harness-paint-phase-design.md` (status: revised). Read it before starting.

**Branch:** `render-harness-paint-phase` (off `main` = the merged virtualization work, which includes `RenderSliverListLazy`).

---

## File Structure

| Path | Responsibility |
|------|----------------|
| `crates/flui-rendering/Cargo.toml` (modify) | Add `insta` dev-dependency; ensure the self `testing`-feature dev-dependency already enables the harness. |
| `crates/flui-rendering/src/testing/snapshot.rs` (create) | `DrawCommandSummary` + `DrawKind`, `summarize_command`, `serialize_layer_tree`, `serialize_layer_subtree`, `collect_commands`. The stable serializer + the flattener for predicates. The ONE place that matches the `Layer`/`DrawCommand` enums. |
| `crates/flui-rendering/src/testing/harness.rs` (modify) | New phase handles `CompositingRun`/`PaintRun`/`SemanticsRun` + `RenderTester::{run_to_compositing, run_to_paint, run_to_semantics, try_run_layout, try_run_frame, expect_layout_error}`; `snapshot`/`snapshot_of`/`display_commands`/`assert_paints_any` on `PaintRun` + `FrameRun`; `has_overflow`. |
| `crates/flui-rendering/src/testing/mod.rs` (modify) | Re-export the new public items. |
| `crates/flui-rendering/tests/harness_snapshot.rs` (create) | Dogfood: `insta` snapshots of `RenderDecoratedBox` / a clip object / `RenderOpacity` / `RenderSliverListLazy`; error-capture + phase tests. |
| `crates/flui-rendering/tests/snapshots/` (create, generated) | Committed `.snap` files (reviewed like code). |
| `crates/flui-rendering/docs/TESTING.md` (modify) | Document the new sections + examples. |

**Substrate to read (do not re-derive):**
- `crates/flui-painting/src/display_list/command.rs` — the `DrawCommand` enum (31 variants) + `CommandKind`. The serializer matches these.
- `crates/flui-layer/src/layer/` (`mod.rs` for the `Layer` enum, `picture.rs` for `PictureLayer::picture()`) — the layer-tree the serializer walks.
- `crates/flui-layer/src/testing/inspect.rs` — existing walkers (`structure_with_depth`, `layer_kind`); follow their walk pattern.
- `crates/flui-rendering/src/pipeline/phase.rs` — phase typestate + `into_*`/`run_*` transitions (the doc-comment shows the exact legitimate sequence).
- `crates/flui-rendering/src/error.rs` — `RenderError` variants (`Poisoned`/`ContractViolation`/`UnboundedConstraint`/…; no `Overflow`).

---

## Task 1: Add `insta` and prove the tooling

**Files:**
- Modify: `crates/flui-rendering/Cargo.toml`
- Test: `crates/flui-rendering/tests/harness_snapshot.rs` (create)

- [ ] **Step 1: Add the dev-dependency**

In `crates/flui-rendering/Cargo.toml`, under `[dev-dependencies]`, add:

```toml
insta = "1"
```

Run `cargo metadata -q >/dev/null` (or `cargo fetch -p flui-rendering`) to confirm it resolves.

- [ ] **Step 2: Write a smoke snapshot test**

Create `crates/flui-rendering/tests/harness_snapshot.rs`:

```rust
//! Structural paint-snapshot dogfood for the render harness (sub-project A).

#[test]
fn insta_tooling_smoke() {
    insta::assert_snapshot!("smoke", "line one\nline two");
}
```

- [ ] **Step 3: Run, accept the snapshot, confirm it passes**

Run: `cargo test -p flui-rendering --test harness_snapshot insta_tooling_smoke`
Expected: first run creates `tests/snapshots/harness_snapshot__smoke.snap.new`. Accept with `cargo insta accept` (or set `INSTA_UPDATE=always` once), then re-run — Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/flui-rendering/Cargo.toml crates/flui-rendering/tests/harness_snapshot.rs crates/flui-rendering/tests/snapshots/
git commit -m "test(rendering): add insta dev-dep + snapshot tooling smoke test"
```

---

## Task 2: `DrawCommandSummary` + `summarize_command`

A stable, normalized projection of one `DrawCommand` to a single line. This is the leaf of the serializer and the unit a predicate sees.

**Files:**
- Create: `crates/flui-rendering/src/testing/snapshot.rs`
- Modify: `crates/flui-rendering/src/testing/mod.rs`
- Test: in-file `#[cfg(test)] mod tests` in `snapshot.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/flui-rendering/src/testing/snapshot.rs`:

```rust
//! Stable structural serialization of a painted `LayerTree` for snapshot tests.
//!
//! Walks the layer tree (clip/opacity/transform/picture layers) and expands
//! each `PictureLayer`'s `DisplayList` into normalized `DrawCommand` lines. The
//! output is deterministic and platform-independent (no GPU, no pixels) so it
//! is a stable `insta` snapshot oracle. See docs/TESTING.md.

use flui_painting::display_list::{CommandKind, DrawCommand};

/// Coarse category of a painted command (mirrors `CommandKind` plus the few
/// shapes a predicate commonly selects on).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawKind {
    Rect,
    RRect,
    Circle,
    Oval,
    Path,
    Line,
    Arc,
    DRRect,
    Clip,
    Text,
    Image,
    Shadow,
    Gradient,
    Layer,
    Other,
}

/// A normalized, comparison-stable summary of one `DrawCommand`.
#[derive(Debug, Clone, PartialEq)]
pub struct DrawCommandSummary {
    /// Coarse category, for `assert_paints_any` predicates.
    pub kind: DrawKind,
    /// The render-stable, single-line text form used in snapshots.
    pub line: String,
}

/// Formats an `f32` to two decimals, normalizing `-0.0` to `0.0`.
fn f(v: f32) -> String {
    let v = if v == 0.0 { 0.0 } else { v };
    format!("{v:.2}")
}

/// Summarizes one `DrawCommand` into a normalized line + coarse kind.
#[must_use]
pub fn summarize_command(cmd: &DrawCommand) -> DrawCommandSummary {
    match cmd {
        DrawCommand::DrawRect { rect, paint, .. } => DrawCommandSummary {
            kind: DrawKind::Rect,
            line: format!(
                "DrawRect rect=({},{} {}x{}) {}",
                f(rect.left().get()),
                f(rect.top().get()),
                f(rect.width().get()),
                f(rect.height().get()),
                summarize_paint(paint),
            ),
        },
        // ... one arm per DrawCommand variant (see Step 3) ...
        _ => DrawCommandSummary { kind: DrawKind::Other, line: format!("{:?}", cmd.kind()) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_painting::Paint;
    use flui_types::{Rect, geometry::px, styling::Color};

    #[test]
    fn summarize_draw_rect_is_stable() {
        let cmd = DrawCommand::DrawRect {
            rect: Rect::from_ltrb(px(0.0), px(0.0), px(40.0), px(40.0)),
            paint: std::sync::Arc::new(Paint::fill(Color::RED)),
            transform: flui_types::geometry::Matrix4::IDENTITY,
        };
        let s = summarize_command(&cmd);
        assert_eq!(s.kind, DrawKind::Rect);
        assert_eq!(s.line, "DrawRect rect=(0.00,0.00 40.00x40.00) fill #FF0000FF");
    }
}
```

> Verify the exact constructors against the substrate: `DrawCommand::DrawRect` fields (`command.rs:121`), `Rect::{left,top,width,height}` and `Pixels::get`, `Matrix4::IDENTITY`, `Paint::fill`, `Color::RED`, and the colour hex format. Adjust the expected string to the real `summarize_paint` output (Step 2).

- [ ] **Step 2: Add `summarize_paint` (referenced above)**

Add to `snapshot.rs`:

```rust
use flui_painting::Paint;

/// Summarizes a `Paint` to `<style> <#RRGGBBAA>[ stroke=<w>]`.
fn summarize_paint(paint: &Paint) -> String {
    // Read the real Paint API: style (Fill/Stroke), color, stroke width.
    // Example shape — match the actual `Paint` accessors:
    let style = match paint.style() { /* PaintingStyle::Fill => "fill", Stroke => "stroke" */ _ => "fill" };
    let mut s = format!("{style} {}", hex_color(paint.color()));
    if paint.stroke_width() != 0.0 {
        s.push_str(&format!(" stroke={}", f(paint.stroke_width())));
    }
    s
}

/// `#RRGGBBAA` from a `Color` (read the real `Color` channel accessors).
fn hex_color(c: flui_types::styling::Color) -> String {
    let (r, g, b, a) = c.to_rgba8(); // adjust to the real accessor
    format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
}
```

> The `Paint`/`Color` accessor names (`style()`, `color()`, `stroke_width()`, `to_rgba8()`) are placeholders for the REAL ones — grep `crates/flui-painting/src/` and `crates/flui-types/src/styling/` and use the actual signatures. Update the Task-2 test's expected string to match. This is the only spot where the colour/paint format is decided; keep it stable thereafter.

- [ ] **Step 3: Run the test (fails), then fill ALL variant arms**

Run: `cargo test -p flui-rendering --lib testing::snapshot::tests::summarize_draw_rect_is_stable`
Expected: FAIL (string mismatch until `summarize_paint` is real).

Then replace the `_ =>` catch-all with **one arm per `DrawCommand` variant** from `command.rs` (31 variants). Each arm produces a `DrawCommandSummary` with the right `DrawKind` and a normalized `line`. Pattern (apply to every variant):
- shapes (`DrawRRect`/`DrawCircle`/`DrawOval`/`DrawPath`/`DrawLine`/`DrawArc`/`DrawDRRect`/`DrawPoints`): `"<Name> <geom> <paint>"`, geom via `f(...)`; `DrawKind` per shape; for `DrawPath` print `bounds + pts=<n>` (NOT raw verbs).
- clips (`ClipRect`/`ClipRRect`/`ClipPath`/`ClipRSuperellipse`): `kind=Clip`, `"<Name> <geom> op=<clip_op>"`.
- text (`DrawText`/`DrawTextSpan`): `kind=Text`, `"<Name> offset=(x,y) <text-or-span-summary>"` (for `DrawTextSpan` summarize the plain text + run count; do NOT require shaped glyphs).
- images (`DrawImage*`/`DrawTexture`/`DrawAtlas`): `kind=Image`, `"<Name> dst=(...)"` (image identity = size only; do not compare pixels).
- effects (`DrawShadow`=`Shadow`, `DrawGradient*`=`Gradient`, `ShaderMask`/`BackdropFilter`=`Layer` — note these carry `Box<DisplayList>` children; recurse into them in Task 3).
- fills (`DrawColor`/`DrawPaint`): `kind=Other`, `"<Name> <paint-or-color>"`.
- layers (`SaveLayer`/`RestoreLayer`): `kind=Layer`.
Keep the per-command `transform` **out** of `line` unless non-identity (the design's "transform is a noise trap" finding); when non-identity, append ` xf=<6 normalized floats>`.

- [ ] **Step 4: Run the test (passes)**

Run: `cargo test -p flui-rendering --lib testing::snapshot::tests::summarize_draw_rect_is_stable`
Expected: PASS. Add a second test asserting a `DrawShadow` summarizes with `kind == DrawKind::Shadow`.

- [ ] **Step 5: Wire the module + commit**

In `crates/flui-rendering/src/testing/mod.rs` add `pub mod snapshot;` and to the re-export block add `pub use snapshot::{DrawCommandSummary, DrawKind};`.

Run: `cargo test -p flui-rendering --lib testing::snapshot` and `cargo clippy -p flui-rendering --all-targets --all-features -- -D warnings`. Expected: PASS, 0 warnings.

```bash
git add crates/flui-rendering/src/testing/snapshot.rs crates/flui-rendering/src/testing/mod.rs
git commit -m "test(rendering): DrawCommandSummary + summarize_command (stable per-command line)"
```

---

## Task 3: `serialize_layer_tree` — walk the painted `LayerTree`

**Files:**
- Modify: `crates/flui-rendering/src/testing/snapshot.rs`
- Test: in-file `#[cfg(test)] mod tests`

- [ ] **Step 1: Write the failing test (whole-tree snapshot via the harness)**

Add to `snapshot.rs` tests:

```rust
#[test]
fn serialize_simple_box_is_stable() {
    use crate::objects::RenderColoredBox;
    use crate::testing::{RenderTester, box_node};
    use flui_types::{Size, geometry::px};

    let run = RenderTester::mount(box_node(RenderColoredBox::red(40.0, 40.0)))
        .with_size(Size::new(px(40.0), px(40.0)))
        .run_frame();
    let tree = run.layer_tree().expect("a frame paints a layer tree");
    let text = serialize_layer_tree(tree);
    // Exact lines depend on the real Layer enum; assert structural shape:
    assert!(text.contains("Picture"), "snapshot must list the picture layer:\n{text}");
    assert!(text.contains("DrawRect rect=(0.00,0.00 40.00x40.00)"), "snapshot must expand commands:\n{text}");
}
```

- [ ] **Step 2: Run it (fails — `serialize_layer_tree` undefined)**

Run: `cargo test -p flui-rendering --lib testing::snapshot::tests::serialize_simple_box_is_stable`
Expected: FAIL (`serialize_layer_tree` not found).

- [ ] **Step 3: Implement the walk**

Add to `snapshot.rs`:

```rust
use flui_layer::LayerTree;

/// Serializes a whole painted layer tree to a stable, indented text form.
#[must_use]
pub fn serialize_layer_tree(tree: &LayerTree) -> String {
    let mut out = String::new();
    // Walk from the tree root. Follow flui_layer::testing::inspect's walk
    // pattern (structure_with_depth) for how to reach the root layer + children.
    write_layer(&mut out, tree_root_layer(tree), 0);
    out
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth { out.push_str("  "); }
}

/// Emits one layer line + recurses children; PictureLayers expand their commands.
fn write_layer(out: &mut String, layer: &flui_layer::Layer, depth: usize) {
    indent(out, depth);
    match layer {
        // Match EVERY Layer variant (read crates/flui-layer/src/layer/mod.rs).
        // Picture: print "Picture bounds=(...)" then each command on the next
        // indent level, via summarize_command(...).picture() -> &DisplayList.
        // Clip*/Opacity/Transform/Offset: print the kind + its defining param
        // (clip rect, alpha, matrix, offset), then recurse children.
        // ShaderMask/BackdropFilter DrawCommands carry Box<DisplayList> children
        // — recurse those too so masked content is in the snapshot.
        _ => { out.push_str("Layer\n"); }
    }
}
```

> Fill `tree_root_layer`, the `Layer` match arms, and the picture-command expansion against the REAL `flui_layer::Layer` enum + `PictureLayer::picture()` (`crates/flui-layer/src/layer/`). Use `flui_layer::testing::inspect::layer_kind` for the kind name where helpful. Indent children by depth; expand a picture's commands at `depth+1` using `summarize_command(cmd).line`.

- [ ] **Step 4: Run it (passes)**

Run: `cargo test -p flui-rendering --lib testing::snapshot::tests::serialize_simple_box_is_stable`
Expected: PASS.

- [ ] **Step 5: Add `serialize_layer_subtree`, `collect_commands` + commit**

Add:

```rust
/// Serializes only the layer subtree rooted at the layer that paints `node`'s
/// content. Falls back to the whole tree if no per-node boundary exists
/// (documented approximate — see the design's snapshot_of risk).
#[must_use]
pub fn serialize_layer_subtree(tree: &LayerTree, node: flui_foundation::RenderId) -> String {
    // Locate the OffsetLayer/boundary keyed to `node` if present; else whole tree.
    // Reuse the walk from serialize_layer_tree on the located subtree.
    let _ = node;
    serialize_layer_tree(tree) // refine to subtree when a boundary exists
}

/// Flattens every DrawCommand across all pictures (for predicates).
#[must_use]
pub fn collect_commands(tree: &LayerTree) -> Vec<DrawCommandSummary> {
    let mut v = Vec::new();
    // Walk the tree; for each PictureLayer push summarize_command for each cmd;
    // recurse ShaderMask/BackdropFilter child display lists.
    collect_from_layer(tree_root_layer(tree), &mut v);
    v
}
```

Run: `cargo test -p flui-rendering --lib testing::snapshot` + clippy. Expected: PASS, 0 warnings.

```bash
git add crates/flui-rendering/src/testing/snapshot.rs
git commit -m "test(rendering): serialize_layer_tree/_subtree + collect_commands"
```

---

## Task 4: `snapshot` / `display_commands` / `assert_paints_any` on `FrameRun`

**Files:**
- Modify: `crates/flui-rendering/src/testing/harness.rs`
- Test: in `tests/harness_snapshot.rs`

- [ ] **Step 1: Write the failing test**

Add to `tests/harness_snapshot.rs`:

```rust
use flui_rendering::objects::RenderColoredBox;
use flui_rendering::testing::{box_node, DrawKind, RenderTester};
use flui_types::{Size, geometry::px};

#[test]
fn frame_snapshot_and_predicate() {
    let run = RenderTester::mount(box_node(RenderColoredBox::red(40.0, 40.0)))
        .with_size(Size::new(px(40.0), px(40.0)))
        .run_frame();

    insta::assert_snapshot!("colored_box", run.snapshot());
    run.assert_paints_any(|c| c.kind == DrawKind::Rect);
}

#[test]
#[should_panic(expected = "no painted command matched")]
fn assert_paints_any_fails_on_absent_op() {
    let run = RenderTester::mount(box_node(RenderColoredBox::red(40.0, 40.0)))
        .with_size(Size::new(px(40.0), px(40.0)))
        .run_frame();
    run.assert_paints_any(|c| c.kind == DrawKind::Shadow); // no shadow → must panic
}
```

- [ ] **Step 2: Run (fails — methods undefined)**

Run: `cargo test -p flui-rendering --test harness_snapshot frame_snapshot_and_predicate`
Expected: FAIL (`snapshot`/`assert_paints_any` not found).

- [ ] **Step 3: Implement on `FrameRun`**

In `harness.rs`, add to `impl FrameRun`:

```rust
/// Stable structural snapshot of the painted layer tree (for `insta`).
#[must_use]
pub fn snapshot(&self) -> String {
    self.layer_tree
        .as_ref()
        .map(super::snapshot::serialize_layer_tree)
        .unwrap_or_else(|| "<no layer tree>".to_string())
}

/// Snapshot of the layer subtree painting `node`.
#[must_use]
pub fn snapshot_of(&self, node: RenderId) -> String {
    self.layer_tree
        .as_ref()
        .map(|t| super::snapshot::serialize_layer_subtree(t, node))
        .unwrap_or_else(|| "<no layer tree>".to_string())
}

/// Every painted command, flattened (for predicate assertions).
#[must_use]
pub fn display_commands(&self) -> Vec<super::snapshot::DrawCommandSummary> {
    self.layer_tree
        .as_ref()
        .map(super::snapshot::collect_commands)
        .unwrap_or_default()
}

/// Asserts at least one painted command matches `pred`. Panics with the full
/// snapshot if none does (a real assertion — unlike Flutter `paints..something`).
pub fn assert_paints_any(&self, pred: impl Fn(&super::snapshot::DrawCommandSummary) -> bool) {
    if !self.display_commands().iter().any(pred) {
        panic!("no painted command matched the predicate:\n{}", self.snapshot());
    }
}
```

- [ ] **Step 4: Run (passes)**

Run: `cargo test -p flui-rendering --test harness_snapshot` then `cargo insta accept`, re-run.
Expected: PASS (both the snapshot and the `should_panic` test).

- [ ] **Step 5: Export + commit**

In `testing/mod.rs` re-export `DrawCommandSummary`/`DrawKind` (already from Task 2). Run clippy.

```bash
git add crates/flui-rendering/src/testing/harness.rs crates/flui-rendering/tests/harness_snapshot.rs crates/flui-rendering/tests/snapshots/
git commit -m "test(rendering): FrameRun snapshot/display_commands/assert_paints_any"
```

---

## Task 5: Phase-tagged run handles (A.1)

**Files:**
- Modify: `crates/flui-rendering/src/testing/harness.rs`, `mod.rs`
- Test: `tests/harness_snapshot.rs`

- [ ] **Step 1: Write the failing test**

Add to `tests/harness_snapshot.rs`:

```rust
#[test]
fn run_to_paint_exposes_layer_tree() {
    let run = RenderTester::mount(box_node(RenderColoredBox::red(40.0, 40.0)))
        .with_size(Size::new(px(40.0), px(40.0)))
        .run_to_paint();
    assert!(run.layer_tree().is_some(), "PaintRun must hold the painted layer tree");
    run.assert_paints_any(|c| c.kind == DrawKind::Rect);
}
```

- [ ] **Step 2: Run (fails — `run_to_paint`/`PaintRun` undefined)**

Run: `cargo test -p flui-rendering --test harness_snapshot run_to_paint_exposes_layer_tree`
Expected: FAIL.

- [ ] **Step 3: Implement `PaintRun` + `run_to_paint`**

In `harness.rs`. Drive the transitions to `PaintPhase` and capture the layer tree. Read `pipeline/phase.rs` for the exact `into_*`/`run_*` sequence and how the paint phase exposes its layer tree (`run_paint` return / `take_layer_tree`):

```rust
use crate::pipeline::PaintPhase; // confirm the exact phase marker name in phase.rs

pub struct PaintRun {
    owner: PipelineOwner<PaintPhase>,
    root_id: RenderId,
    registry: RenderLabelRegistry,
    layer_tree: Option<LayerTree>,
}

impl RenderTester {
    /// Drives layout → compositing → paint and stops, exposing the layer tree.
    #[must_use]
    pub fn run_to_paint(self) -> PaintRun {
        let (owner, root_id, registry) = self.build();
        let mut owner = owner.into_layout();
        owner.run_layout().expect("layout");
        let mut owner = owner.into_compositing();
        owner.run_compositing().expect("compositing");
        let mut owner = owner.into_paint();
        let layer_tree = owner.run_paint().expect("paint"); // adjust to real return/take
        PaintRun { owner, root_id, registry, layer_tree }
    }
}

impl PaintRun {
    #[must_use] pub fn root(&self) -> RenderId { self.root_id }
    #[must_use] pub fn layer_tree(&self) -> Option<&LayerTree> { self.layer_tree.as_ref() }
    // snapshot/snapshot_of/display_commands/assert_paints_any: identical bodies to
    // FrameRun's (Task 4). Factor the four into a private helper fn over
    // `Option<&LayerTree>` to avoid duplication (DRY), e.g.
    // `fn snapshot_of_tree(tree: Option<&LayerTree>) -> String`.
}

impl Probe for PaintRun {
    type Phase = PaintPhase;
    fn pipeline(&self) -> &PipelineOwner<PaintPhase> { &self.owner }
    fn registry(&self) -> &RenderLabelRegistry { &self.registry }
}
```

> DRY: extract the snapshot/predicate bodies (Task 4) into free fns in `snapshot.rs` taking `Option<&LayerTree>`, and have BOTH `FrameRun` and `PaintRun` call them. Update Task 4's `FrameRun` methods to delegate.

- [ ] **Step 4: Run (passes)**

Run: `cargo test -p flui-rendering --test harness_snapshot run_to_paint_exposes_layer_tree`
Expected: PASS.

- [ ] **Step 5: Add `run_to_compositing`/`run_to_semantics` + a compile-fail proof + commit**

Add `CompositingRun`/`SemanticsRun` (same shape; no `layer_tree` on `CompositingRun`; `SemanticsRun` exposes the semantics tree raw — just `owner()` for now, finders are sub-project B). Add a doc-test proving `LayoutRun` has no `snapshot` (compile-fail):

```rust
/// ```compile_fail
/// # use flui_rendering::objects::RenderColoredBox;
/// # use flui_rendering::testing::{box_node, RenderTester};
/// let run = RenderTester::mount(box_node(RenderColoredBox::red(1.0, 1.0))).run_layout();
/// let _ = run.snapshot(); // error: no method `snapshot` on LayoutRun
/// ```
```

Export `PaintRun`/`CompositingRun`/`SemanticsRun` from `mod.rs`. Run clippy + tests.

```bash
git add -A
git commit -m "test(rendering): compile-checked phase-tagged run handles (run_to_paint/compositing/semantics)"
```

---

## Task 6: Error / assertion capture (A.3)

**Files:**
- Modify: `crates/flui-rendering/src/testing/harness.rs`, `mod.rs`
- Test: `tests/harness_snapshot.rs`

- [ ] **Step 1: Write the failing test**

Add a tiny render object whose `paint` panics, mount it, and assert `try_run_frame` returns `Err(Poisoned)`. (Define the panicking object inline in the test file, mirroring the `FixedBox` pattern in `tests/u3c_lazy_sliver_contract.rs`.)

```rust
#[test]
fn try_run_frame_captures_poisoned_paint() {
    // A leaf RenderBox whose paint panics -> pipeline catch_unwind -> RenderError::Poisoned.
    // (impl RenderBox for PanicPaintBox with perform_layout returning a fixed Size and
    //  paint() { panic!("boom") }; see u3c_lazy_sliver_contract.rs for the impl scaffold.)
    let err = RenderTester::mount(box_node(PanicPaintBox))
        .with_size(Size::new(px(10.0), px(10.0)))
        .try_run_frame()
        .expect_err("a panicking paint must surface as Err");
    assert!(matches!(err, flui_rendering::error::RenderError::Poisoned { .. }));
}
```

- [ ] **Step 2: Run (fails — `try_run_frame` undefined)**

Run: `cargo test -p flui-rendering --test harness_snapshot try_run_frame_captures_poisoned_paint`
Expected: FAIL.

- [ ] **Step 3: Implement the fallible runs + `has_overflow`**

In `harness.rs`:

```rust
use crate::error::RenderError;

impl RenderTester {
    /// Like `run_frame` but surfaces the pipeline error instead of panicking.
    pub fn try_run_frame(self) -> Result<FrameRun, RenderError> {
        let (owner, root_id, registry) = self.build();
        let (owner, result) = owner.run_frame();
        let layer_tree = result?;
        Ok(FrameRun { owner, root_id, registry, layer_tree })
    }

    /// Like `run_layout` but surfaces the error.
    pub fn try_run_layout(self) -> Result<LayoutRun, RenderError> {
        let (owner, root_id, registry) = self.build();
        let mut owner = owner.into_layout();
        owner.run_layout()?;
        Ok(LayoutRun { owner, root_id, registry })
    }

    /// Asserts the layout phase fails and returns the error.
    #[must_use]
    pub fn expect_layout_error(self) -> RenderError {
        self.try_run_layout().expect_err("expected layout to fail")
    }
}

/// Whether `node` reports visual overflow (a flag, not an error).
#[must_use]
pub fn has_overflow(probe: &impl Probe, node: RenderId) -> bool {
    probe
        .property(node, "has_visual_overflow")
        .map(|v| v == "true")
        .unwrap_or(false) // confirm the real diagnostics property name/value
}
```

> Confirm the `RenderError::Poisoned` variant shape (`error.rs`) and the `has_visual_overflow` diagnostics property name; adjust the test + `has_overflow` to match. If no render object exposes `has_visual_overflow` via diagnostics, read the flag from the committed geometry instead and document it.

- [ ] **Step 4: Run (passes)**

Run: `cargo test -p flui-rendering --test harness_snapshot try_run_frame_captures_poisoned_paint`
Expected: PASS.

- [ ] **Step 5: Export + commit**

```bash
git add -A
git commit -m "test(rendering): fallible runs (try_run_frame/expect_layout_error) + has_overflow"
```

---

## Task 7: Dogfood snapshots (paint-logic-heavy objects)

**Files:**
- Modify: `crates/flui-rendering/tests/harness_snapshot.rs`
- Create (generated): `crates/flui-rendering/tests/snapshots/*.snap`

- [ ] **Step 1: Write the dogfood snapshots**

Add `insta::assert_snapshot!` tests for the genuinely multi-command-paint objects (NOT the tautological `RenderColoredBox`). For each: mount, `run_frame`, snapshot.

```rust
#[test]
fn snapshot_decorated_box() {
    use flui_rendering::objects::RenderDecoratedBox; // confirm exact ctor + BoxDecoration API
    // A decoration with a border (DrawDRRect), shadow (DrawShadow), and fill —
    // the real multi-command paint logic geometry/structure can't catch.
    let run = RenderTester::mount(box_node(/* RenderDecoratedBox with border+shadow+fill */))
        .with_size(Size::new(px(60.0), px(40.0)))
        .run_frame();
    insta::assert_snapshot!("decorated_box", run.snapshot());
}

#[test]
fn snapshot_opacity_layer() {
    use flui_rendering::objects::{RenderColoredBox, RenderOpacity};
    let run = RenderTester::mount(
        box_node(RenderOpacity::new(0.5))
            .child(box_node(RenderColoredBox::red(20.0, 20.0))),
    )
    .with_size(Size::new(px(20.0), px(20.0)))
    .run_frame();
    insta::assert_snapshot!("opacity", run.snapshot()); // must show an Opacity layer wrapping the picture
}

#[test]
fn snapshot_lazy_sliver_list_paints_only_visible_band() {
    // Mount RenderSliverListLazy under the SliverHost scaffold from
    // tests/u3c_lazy_sliver_contract.rs; pump to settle; snapshot.
    // The snapshot must show pictures for ONLY the visible+cache children —
    // the virtualization win asserted at the paint-structure layer.
}
```

> Use the exact `RenderDecoratedBox`/`BoxDecoration`/`RenderOpacity` constructors (grep `crates/flui-rendering/src/objects/`). For the lazy-sliver snapshot, reuse the `SliverHost` + `FixedBox` + `build_and_pump` scaffold already in `tests/u3c_lazy_sliver_contract.rs` (copy the minimal harness or factor it into `testing::sliver`).

- [ ] **Step 2: Generate + review the snapshots**

Run: `cargo test -p flui-rendering --test harness_snapshot`, then `cargo insta review` — **read each `.snap`**: confirm the decorated box shows DrawShadow + DrawDRRect + fill in order; opacity shows the Opacity layer; the lazy list shows only the visible+cache pictures. Accept only if correct.

- [ ] **Step 3: Run to confirm stable + commit**

Run: `cargo test -p flui-rendering --test harness_snapshot` (second run, no `.snap.new`). Expected: PASS.

```bash
git add crates/flui-rendering/tests/harness_snapshot.rs crates/flui-rendering/tests/snapshots/
git commit -m "test(rendering): dogfood paint snapshots (decoration/opacity/lazy-sliver)"
```

---

## Task 8: Document + final gates

**Files:**
- Modify: `crates/flui-rendering/docs/TESTING.md`

- [ ] **Step 1: Document the new API**

Add to `docs/TESTING.md` a "Paint snapshots & phases" section: `run_to_paint`/`PaintRun`, `snapshot`/`snapshot_of`/`display_commands`/`assert_paints_any`, `try_run_frame`/`expect_layout_error`/`has_overflow`, the `cargo insta review` workflow, and a note that op-sequence matching is intentionally NOT provided (anti-pattern; use the snapshot + `assert_paints_any`). Cross-link the design doc.

- [ ] **Step 2: Full gate**

Run, expect all green:
- `cargo fmt --all -- --check`
- `cargo clippy -p flui-rendering --all-targets --all-features -- -D warnings`
- `cargo test -p flui-rendering`
- `RUSTDOCFLAGS="-D warnings" cargo doc -p flui-rendering --no-deps --document-private-items`
- `bash scripts/port-check.sh -v` (the catalog gate + triggers stay clean)

- [ ] **Step 3: Commit**

```bash
git add crates/flui-rendering/docs/TESTING.md
git commit -m "docs(rendering): document paint-snapshot + phase harness additions"
```

---

## Notes for the implementer

- **`insta` workflow:** first run writes `.snap.new`; `cargo insta review` (or `accept`) promotes it; commit `.snap` files and review them like code. NEVER blind-accept — a wrong snapshot is a false-green.
- **Stability is the contract:** all floats via `f(...)` (2 decimals), colours `#RRGGBBAA`, deterministic ordered walks, no `HashMap` iteration in the serializer, transform omitted unless non-identity.
- **DRY:** the four snapshot/predicate methods are shared by `FrameRun` and `PaintRun` via free fns in `snapshot.rs`.
- **Do not** add a `paints..rect()..clip()` op-sequence matcher — the design rejects it (anti-pattern). `assert_paints_any(pred)` is the only op-level assertion.
- **Substrate signatures are authoritative:** where this plan shows a placeholder accessor (`Paint::style`, `Color::to_rgba8`, `run_paint` return, `RenderError::Poisoned` shape, `has_visual_overflow` property), grep the real one and use it; update the affected test's expected string. These are the only "fill from the real API" spots.
