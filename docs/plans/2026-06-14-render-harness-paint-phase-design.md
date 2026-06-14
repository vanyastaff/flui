---
date: 2026-06-14
title: "Render-harness 2.0 — Sub-project A: paint & phase verification (revised)"
type: design
status: revised
revision: "Rewritten after an adversarial review (harsh-critic) + market research. The
  original flat-`DrawCommand` `paints` matcher was verified WRONG against flui's paint
  model; A.2 is re-premised on a structural layer-tree + display-list snapshot."
extends: crates/flui-rendering/src/testing (mod testing) + docs/TESTING.md
---

# Render-harness 2.0 — Sub-project A: paint & phase verification (revised)

## Summary

Bring `flui_rendering::testing` (the render-object test harness) to a **future-correct,
market-best** render-paint verification model. The first draft of this spec ported
Flutter's op-level `paints` matcher; an adversarial review verified that approach is
**wrong for flui's paint model** and market research showed op-matchers are an
**anti-pattern as a primary mechanism**. This revision adopts the cross-framework
2024-2026 consensus: a **typed structural snapshot** as the primary oracle.

Three parts:
- **A.1 Phase-granular pumping** — phase-tagged run handles over flui's real
  `PipelineOwner<Phase>` typestate (`Layout → Compositing → Paint → Semantics`),
  exposing intermediate phases. *Compile-checked, no runtime phase-panic.*
- **A.2 Structural paint snapshot** — serialize the painted `LayerTree` (clip / opacity
  / transform layers + each `PictureLayer`'s typed `DrawCommand` sequence with
  normalized args) to a stable, purpose-built text form, asserted with **`insta`**
  (`cargo insta review`). Plus a narrow `assert_paints_any(pred)` for "this op exists"
  targeted checks. *This is the right use of flui's typed display list — Flutter's
  `paints` is a weaker version of it.*
- **A.3 Error/assertion capture (scoped)** — fallible runs surfacing the real
  `RenderError` (`Poisoned` panic-capture, `ContractViolation`, `UnboundedConstraint`)
  + a `has_overflow(node)` flag read. No invented `Overflow` error.

Sub-project **A** of a research-informed **layered roadmap** (below): structural
snapshot (A) + semantics (B) are the two *primary* stability layers; finders (C),
gestures (D), and a narrow software-renderer pixel golden (E) follow.

## Context

### Adversarial review findings (verified against code)

The draft's A.2 assumed flui paint is a flat `Vec<DrawCommand>` stream. It is not — every
load-bearing assumption was checked and falsified:

- **Paint is a sans-IO fragment model.** `context/paint_cx.rs`: a node's `paint_raw`
  records a `PaintFragment` of `FragmentOp::{Run, Child{index}, Push(FragmentClip), Pop}`
  — children are **index markers, not commands**; subtree clips are `Push(FragmentClip)`
  ops, **never** `DrawCommand::ClipRect`. Only the composer resolves fragments into a
  multi-layer `LayerTree`. So `record_paint(node) -> Vec<DrawCommand>` cannot include a
  node's children, and `clip_*` matcher methods are dead against a flat slice.
- **No flat per-node command slice exists.** A frame is a `LayerTree`; the only per-picture
  access is `PictureLayer::picture() -> &DisplayList` (`flui-layer`). Per-node brackets
  (`OffsetLayer`) exist **only for repaint boundaries** (`is_repaint_boundary` defaults
  `false`); ordinary test render objects bake into a shared picture with no delimiter, so
  `paints_for(node)` slicing had no bracket to cut on.
- **The `Composite` phase does not exist.** `pipeline/phase.rs` typestate is
  `Idle → Layout → Compositing → Paint → Semantics → finish`. Paint *produces* the
  `LayerTree`; there is no post-paint composite phase. The phases are **compile-time
  enforced** (the crate's explicit design); a runtime-panic "phase-gated Run" would
  regress that.
- **`RenderError` has no `Overflow` variant** (`UnboundedConstraint`/`InvalidGeometry`/
  `InvalidConstraints`/`Poisoned`/`ContractViolation`/…); overflow is a `has_visual_overflow`
  flag, not an error.

(The review also claimed `RenderSliverListLazy` "does not exist" — that was a stale-index
error; it is present, 22 references across 4 files, and is a valid dogfood target.)

### Market research consensus (2024-2026, cited)

- **Op-level paint matchers are an anti-pattern as a primary mechanism.** Flutter's
  `PaintPattern` has a documented correctness bug ([flutter#95981] — `paints..something(pred=>false)`
  *passes silently* instead of failing); its subset-match hides regressions; it is
  notoriously brittle on benign paint refactors; the Flutter team does not recommend it
  for app tests. Flutter never shipped a `matchesLayerTree` API ([flutter#46992], open
  since 2019) — `debugDumpLayerTree` is a debug tool, **not** a stable oracle.
- **Typed structural snapshots are the right primary layer.** Analogy from the rustc test
  suite: a typed display list is the **"MIR of rendering"** — test it as structured text
  (op names + args + structure) like `mir-opt` tests; pixel goldens are the "codegen
  tests" (narrow, expensive, separate). `insta` (used by Masonry/Xilem for widget-tree
  snapshots) gives `cargo insta review` interactive diff. egui's `egui_kittest` *"prefer
  regular tests over image comparison"*; Jest's docs separate textual-structural snapshots
  from pixel visual-regression.
- **Semantics-tree assertions are the co-primary stability layer.** Compose (`onNodeWith*`),
  Flutter (`matchesSemantics`), egui (AccessKit `get_by_role`), React Testing Library
  (query by role/label) all converge on testing the semantic tree because it survives
  paint/layout refactors.
- **Pixel goldens are a narrow secondary fence**, done with a *deterministic software
  renderer* (Slint `SoftwareRenderer`, not GPU), run on fixed CI, updated with mandatory
  human review (Masonry `MASONRY_TEST_BLESS`, insta review) — never auto-`--update`.

[flutter#95981]: https://github.com/flutter/flutter/issues/95981
[flutter#46992]: https://github.com/flutter/flutter/issues/46992

### Verified substrate A builds on

- `flui_painting::display_list::{DrawCommand (31 variants, closed `#[non_exhaustive]`),
  DisplayList}`; `flui_painting::testing::record`.
- `flui_layer` `LayerTree` (Offset/Clip/Opacity/Transform/Picture layers);
  `PictureLayer::picture() -> &DisplayList`; `flui_layer::testing::inspect` (existing
  layer walk: `structure_with_depth`, picture bounds, opacity alpha).
- `flui_rendering` harness already produces a `LayerTree` from `run_frame`
  (`FrameRun::layer_tree()`), and `PipelineOwner<Phase>` typestate with
  `into_layout/into_compositing/into_paint/into_semantics` + `run_*`.
- `RenderResult`/`RenderError` from every `run_*`; `Poisoned` is the `catch_unwind`
  capture of a panicking render-object body.
- `cargo-insta` is available in the studio toolchain.

## Goals / Non-goals

**Goals (A):**
1. Phase-tagged runs that reach any pipeline phase, compile-checked.
2. A stable structural snapshot of the painted `LayerTree` + per-picture `DrawCommand`
   sequence, asserted via `insta`; focused (per-node-subtree) snapshots to avoid rot.
3. A narrow targeted `assert_paints_any(pred)` for "this op exists" checks.
4. Fallible runs surfacing real `RenderError` + an overflow flag read.
5. Dogfood: snapshot the genuinely paint-logic-heavy objects (decoration: border DRRect
   + shadow + fill; clip; opacity; the virtualized `RenderSliverListLazy` child
   structure) — not the tautological `RenderColoredBox` single-rect.

**Non-goals (A):**
- The flat `paints..rect()..clip()` op-sequence matcher (anti-pattern; superseded by the
  snapshot + targeted predicate).
- Semantics-tree finders/matchers (B), finders/quantity matchers (C), gestures (D),
  pixel goldens (E) — roadmapped, not in A. A exposes only the `Semantics` *phase*.

## Design

### A.1 — Phase-granular pumping (compile-checked, fixed enum)

Phase-specific run handles, each exposing only phase-appropriate probes (compile-checked,
preserving the `PipelineOwner<Phase>` philosophy — no runtime phase-panic):

```rust
impl RenderTester {
    pub fn run_layout(self) -> LayoutRun;          // after Layout (exists; geometry/intrinsics)
    pub fn run_to_compositing(self) -> CompositingRun; // after Compositing (compositing bits)
    pub fn run_to_paint(self) -> PaintRun;         // after Paint (LayerTree available)
    pub fn run_frame(self) -> FrameRun;            // full frame (exists; == PaintRun today)
    pub fn run_to_semantics(self) -> SemanticsRun; // after Semantics (semantics phase exposed for B)
}
```

- Each handle drives the existing `into_*`/`run_*` transitions up to its phase and stops.
  `LayoutRun`/`FrameRun` are unchanged (back-compat + the catalog gate). `PaintRun` is the
  cheapest handle that exposes the painted `LayerTree` (and thus the snapshot/A.2).
- `Probe` (geometry/offset/hit/diagnostics) is implemented where the data exists
  (`LayoutRun` onward). Paint probes (`snapshot`, `layer_tree`, `assert_paints_any`) live
  on `PaintRun`/`FrameRun` only — calling them before paint is a **compile error**, not a
  panic.
- `SemanticsRun` is introduced here (phase reachable) but its finders/matchers are
  sub-project B; A only proves the phase runs and exposes the raw tree.

### A.2 — Structural paint snapshot (primary oracle)

Walk the painted `LayerTree` and serialize to a **stable, purpose-built** text form
(explicitly *not* `Debug`/`dump` output — anti-pattern #3): layer nesting is the snapshot's
tree shape (so clip/opacity/transform scoping is structurally preserved — no
cross-clip false positives a flat matcher would produce), and each `PictureLayer`'s
`DisplayList` expands to one normalized line per `DrawCommand`.

```rust
impl PaintRun /* and FrameRun */ {
    pub fn snapshot(&self) -> String;                 // whole painted LayerTree
    pub fn snapshot_of(&self, node: RenderId) -> String; // a node's layer subtree (focused)
    pub fn display_commands(&self) -> Vec<DrawCommandSummary>; // flattened, for predicates
    pub fn assert_paints_any(&self, pred: impl Fn(&DrawCommandSummary) -> bool); // targeted
}
```

Usage:
```rust
let run = RenderTester::mount(/* decorated box */).with_size(...).run_to_paint();
insta::assert_snapshot!(run.snapshot());          // cargo insta review on change
run.assert_paints_any(|c| matches!(c.kind, DrawKind::Shadow)); // targeted: a shadow is painted
```

**Serialization rules (stability is the contract):**
- Deterministic field order; floats normalized (fixed decimals); colours as `#RRGGBBAA`;
  `Paint` summarized (`color`, `style`, `stroke_width`); `Path` summarized (bounds +
  point-count, not raw verbs); per-command `transform` **omitted unless non-identity**
  (the review's "transform is a noise trap" finding) and then printed normalized.
- Layer lines carry the layer kind + its defining param (clip rect, opacity, transform
  offset) and indent by depth; picture commands indent under their picture.
- A `redact`-style hook for any non-deterministic field (none expected today; future-proof).
- `snapshot_of(node)` scopes to the node's layer subtree when it is a boundary; otherwise
  to the picture run the node contributes (sliced by the fragment `Child` marker the
  composer already tracks — structural, not a guessed byte range).

`assert_paints_any` is the *only* op-level assertion (the narrow case Flutter's
`paints..something()` covers) and is implemented as a real predicate over the flattened
commands (no silent-pass bug like [flutter#95981]: a non-matching predicate **fails**).

### A.3 — Error / assertion capture (scoped to real errors)

```rust
impl RenderTester {
    pub fn try_run_layout(self) -> Result<LayoutRun, RenderError>;
    pub fn try_run_frame(self)  -> Result<FrameRun,  RenderError>;
    pub fn expect_layout_error(self) -> RenderError;   // asserts Err, returns it
}
pub fn has_overflow(run: &impl Probe, node: RenderId) -> bool; // reads has_visual_overflow flag
```

- The realistic failing paths are `Poisoned` (a render object's layout/paint body panics →
  `catch_unwind` → `RenderError::Poisoned`), `ContractViolation`, and
  `UnboundedConstraint`. `expect_layout_error` is for "this misconfiguration must fail"
  tests; the dogfood includes one render object driven into each real variant.
- Overflow is a flag, not an error: `has_overflow` reads `has_visual_overflow` from the
  committed geometry/diagnostics, no fake error path.

## Public API surface (additions, behind `testing` feature / `cfg(test)`)

`RenderPhase`-tagged handles `CompositingRun`/`PaintRun`/`SemanticsRun` +
`RenderTester::{run_to_compositing, run_to_paint, run_to_semantics, try_run_layout,
try_run_frame, expect_layout_error}`; `snapshot`/`snapshot_of`/`display_commands`/
`assert_paints_any` + `DrawCommandSummary`/`DrawKind`; `has_overflow`. The snapshot
serializer lives in a new `testing/snapshot.rs`; phase handles extend `testing/harness.rs`.
`insta` is added as a dev-dependency.

## Testing strategy

1. **Snapshot serializer unit tests** — build known `LayerTree`s (via the harness on small
   trees) and assert the serialized form is stable, ordered, normalized, and structurally
   reflects clip/opacity nesting; `snapshot_of` scopes correctly.
2. **Phase tests** — `run_to_paint` exposes a `LayerTree` while `run_layout` (by type)
   cannot reach `snapshot`; phase handles reach the right `PipelineOwner<Phase>`.
3. **`assert_paints_any`** — passes on a present op, **fails** on an absent one (the
   anti-#95981 guarantee), fails on a false predicate.
4. **A.3** — a render object whose paint panics yields `Poisoned` via `try_run_frame`;
   `has_overflow` true for an over-constrained flex child.
5. **Dogfood (paint-logic-heavy, not tautological):** `insta` snapshots of
   `RenderDecoratedBox` (border `DrawDRRect` + `DrawShadow` + fill — the real multi-command
   paint logic), a clip object (clip-layer structure), `RenderOpacity` (opacity layer), and
   `RenderSliverListLazy` (only visible+cache children's pictures appear — the
   virtualization win asserted at the paint-structure layer, beyond built-count).
6. `docs/TESTING.md` updated; `cargo doc -D warnings`, the catalog gate, and `cargo insta
   test` stay green; committed `.snap` files reviewed like code.

## Risks & mitigations

- **Snapshot rot** (developers rubber-stamp updates) — *mitigation:* `snapshot_of(node)`
  focused snapshots, one render object per snapshot; CI surfaces the diff; never auto-update.
- **Serializer instability across platforms** (float formatting, map order) — *mitigation:*
  fixed-decimal float formatting, deterministic ordered walk, no `HashMap` iteration in the
  serializer; this is a *no-GPU structural* snapshot, so it is platform-independent by
  construction (the pixel-golden platform problem is E's, not A's).
- **`snapshot_of` scoping for non-boundary nodes** — *mitigation:* scope by the composer's
  fragment `Child` marker (a real structural delimiter), or document `snapshot_of` as
  "the node's picture contribution + its layer subtree"; do not guess byte ranges.

## Layered roadmap (render-harness 2.0, research-informed)

Two *primary* stability layers, then interaction/query, then a narrow visual fence:

- **A (this) — Structural paint snapshot + phase pump + error capture.** Primary
  render-correctness oracle (typed display-list + layer-tree via `insta`).
- **B — Semantics-tree assertions (AccessKit).** *Co-primary* stability layer (elevated
  from "last" by the research): query/assert the semantics tree (label/role/action/flags),
  AccessKit-wired (which flui needs for platform a11y anyway). Survives paint refactors.
- **C — Finders & quantity matchers.** by-type/key/predicate/descendant/ancestor +
  `finds_one/nothing/n` + geometry-by-finder + auto-fail tree/snapshot dump.
- **D — Gesture/pointer simulation.** `TestGesture` through the gesture arena
  (tap/drag/fling/long-press) + `pump_and_settle`; validates lazy-`SliverList` scroll.
- **E — Software-renderer pixel golden (narrow).** Deterministic *software* rasterizer
  (not wgpu) for specific visual cases (gradients/borders/text); separate track, mandatory
  review on update. A GPU smoke-test (pipeline doesn't panic) may sit beside it.

The op-level paint matcher is intentionally **not** a layer: demoted to A's targeted
`assert_paints_any(pred)` only (anti-pattern as primary, per [flutter#95981] + brittleness).

## Resolved decisions

- A.2 mechanism: **structural layer-tree + display-list snapshot (`insta`)**, not a flat
  `DrawCommand` op-matcher (the latter contradicts flui's fragment/layer-tree paint model
  and is a market-documented anti-pattern). Targeted `assert_paints_any(pred)` retained.
- A.1 phases: `Layout/Compositing/Paint/Semantics` (no `Composite`); **compile-checked
  phase-tagged run handles**, not a runtime-panic unified `Run`.
- A.3: surface **real** `RenderError` (`Poisoned`/`ContractViolation`/`UnboundedConstraint`)
  + `has_overflow` flag; no invented overflow error.
- Roadmap: structural-snapshot (A) + semantics (B) co-primary; pixel golden (E) narrow,
  software-renderer, separate.
