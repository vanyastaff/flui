---
date: 2026-06-14
title: "Render-harness 2.0 — Sub-project A: paint & phase verification"
type: design
status: draft
supersedes_notes: extends crates/flui-rendering/src/testing (mod testing) + docs/TESTING.md
---

# Render-harness 2.0 — Sub-project A: paint & phase verification

## Summary

Bring flui's render-object test harness (`flui_rendering::testing`) to market-best
parity with **how Flutter actually validates render objects** — not the widget-level
`WidgetTester`, but the render-level `rendering_tester.dart` + `mock_canvas.dart`
(`paints`) facility. Three additions, built entirely on existing flui substrate:

- **A.1 Phase-granular pumping** — drive the pipeline to a chosen phase
  (`Layout | CompositingBits | Paint | Composite | Semantics`), mirroring Flutter's
  `pumpFrame(phase: EnginePhase.…)`.
- **A.2 `paints` PaintPattern matcher** — assert the *sequence of paint operations*
  a render object emits, with argument matching, over flui-painting's typed
  `DrawCommand` display list. Flutter's `paints..rect()..clipRect()..circle()`,
  but type-safe over a closed enum instead of a dynamic mock canvas.
- **A.3 Error/assertion capture** — fallible runs that surface `RenderError` so a
  test can assert a render object *fails the right way* (overflow, contract
  violation, paint assertion).

This is sub-project **A** of a 4-part **render-harness 2.0** decomposition
(A paint&phase → B finders&matchers → C gesture/interaction → D semantics). A is
foundational: it is the correctness layer the other three lean on (B/C/D all want
phase-aware checks; C/D produce paint/semantics the `paints`/phase machinery
verifies).

## Context

### Current harness (baseline)

`crates/flui-rendering/src/testing/` is already a strong **render-level** harness
(it is render-level, not widget-level, because `flui-view` does not yet build render
trees — the honest-foundation theme): `box_node`/`sliver_node` tree DSL,
`RenderTester::mount().with_size()/with_constraints().run_layout()/run_frame()`,
`Probe` (id-by-label, offset, box/sliver geometry, hit-test, diagnostics, property
lookup, descendant-by-type, dump), `BoxQueryRun` (intrinsics, dry_layout,
dry_baseline), multi-frame (`pump`, `pump_frames`, `pump_idle_frames`, `simulate`,
`advance_layout`/`advance_paint`, `update`/`update_paint`), `FrameReport`, layer
**structure** assertions (`structure()`, `picture_bounds()`, `opacity_alpha()`), a
catalog gate (`catalog_covers_every_render_object_name`).

The gap vs Flutter render-object testing: flui asserts layer **structure** + bounds,
but not the **paint operations** themselves (which colour the `DrawRect`, which rect
the `ClipRect`, which `DrawPath`); it has `run_layout`/`run_frame` but no
intermediate phases; and it has no error-path testing (runs `.expect()` success).

### Market study (verified against Flutter source)

Flutter `packages/flutter/test/rendering/rendering_tester.dart`:
- `layout(RenderBox, {constraints, alignment, phase, onErrors})` — wraps the SUT in
  `RenderPositionedBox`/`RenderConstrainedBox`, attaches to `renderView`, pumps.
- `pumpFrame({phase: EnginePhase, onErrors})` — runs **up to** a phase:
  `layout → compositingBits → paint → composite → flushSemantics → sendSemanticsUpdate`.
- `TestRenderingFlutterBinding` captures errors (`takeAllFlutterExceptions`,
  `expectNoFlutterErrors`, `absorbOverflowedErrors`, `onErrors`).
- Helper SUTs: `RenderSizedBox`, `TestCallbackPainter`, `TestClipPaintingContext`.

Flutter `packages/flutter_test/lib/src/mock_canvas.dart`:
- `expect(renderObject, paints..rect(color:…, rect:…)..clipRect()..circle(x,y,radius)..path(includes:[…]))`.
- Vocabulary: `transform/translate/scale/rotate/save/restore/saveRestore`,
  `clipRect/clipRRect/clipPath/clipRSuperellipse`, shapes
  `rect/rrect/drrect/circle/path/line/arc`, raster/text `image/drawImageRect/paragraph/shadow`.
- Arg matching: colour (1/255 tolerance), rect, strokeWidth, style, path-contains-points.
- `paintsNothing`, `paintsAssertion`, `paintsExactlyCountTimes(#drawRect, 2)`,
  `something(pred)`, `everything(pred)`. Subset-sequence match: order matters,
  intervening unmatched calls are skipped.

### Substrate (verified — A builds only on what exists)

- **`flui_painting::display_list::DrawCommand`** — a closed, `#[non_exhaustive]`
  29-variant enum (`DrawRect/DrawRRect/DrawCircle/DrawOval/DrawPath/DrawLine/DrawArc/
  DrawDRRect/DrawPoints`; `ClipRect/ClipRRect/ClipPath/ClipRSuperellipse`;
  `DrawText/DrawTextSpan`; `DrawImage*`; `DrawShadow`; `DrawGradient*`; `ShaderMask`;
  `BackdropFilter`; `DrawColor/DrawPaint`; `SaveLayer/RestoreLayer`; …). Each draw
  variant carries `paint: Arc<Paint>` (colour/style/stroke) and a baked-in
  `transform: Matrix4`. `DrawCommand::kind() -> CommandKind {Draw|Clip|Effect|Layer}`.
- **`flui_painting::testing::record(|canvas| …) -> DisplayList`** — already records a
  closure's commands; `command_count`, `bounds`, `diagnostics`, `dump`.
- **`DisplayList`** holds the recorded `DrawCommand` sequence; a `Picture` layer in the
  produced `LayerTree` carries one.
- **`PipelineOwner<Phase>`** typestate already models pipeline phases; the harness
  drives `run_layout` (Layout) and `run_frame` (full).
- **`RenderError`** / `RenderResult` — the pipeline returns these; the harness
  currently unwraps them.

flui's typed closed-enum display list is **cleaner than Flutter's dynamic mock
canvas**: the matcher pattern-matches real enum variants with full typed args and the
per-command transform is directly inspectable (no `save`/`translate` bookkeeping).

## Goals / Non-goals

**Goals (sub-project A):**
1. Drive the harness to any pipeline phase and expose phase-gated probes.
2. Assert paint-operation sequences (with arg matching) for a render object, both
   **in isolation** (`record_paint(node)`) and **end-to-end** (`frame.display_list()`).
3. Assert error/failure paths (`try_run_to`, `expect_layout_error`, paint assertion).
4. Dogfood: re-express a representative slice of existing structure-only paint tests
   in terms of `paints`, and add op-level paint coverage for the new lazy `SliverList`.

**Non-goals (deferred to B/C/D):**
- Rich finders / quantity matchers / geometry-by-finder (B).
- Gesture/pointer simulation + `pump_and_settle` (C).
- Semantics-tree finders/matchers (D) — A exposes only the `Semantics` *phase*.
- Pixel goldens (no GPU in the harness); a textual display-list snapshot may reuse
  `paints`/`dump` later — out of scope here.

## Design

### A.1 — Phase-granular pumping

```rust
#[non_exhaustive]
pub enum RenderPhase { Layout, CompositingBits, Paint, Composite, Semantics }

impl RenderTester {
    pub fn run_to(self, phase: RenderPhase) -> Run;       // drive pipeline up to `phase`
    pub fn run_layout(self) -> LayoutRun;                 // == run_to(Layout), kept
    pub fn run_frame(self) -> FrameRun;                   // == run_to(Composite), kept
}
```

- `run_to` drives the production `PipelineOwner` through each phase boundary up to the
  requested one (flush layout → compositing bits → paint into a `LayerTree` →
  composite → flush semantics). Each boundary reuses the existing pipeline entry
  points; no new pipeline logic.
- **Return shape (decision):** keep the existing `LayoutRun`/`FrameRun` types as the
  public faces (back-compat) and have `run_to` return a `Run` enum/struct that records
  the reached phase. Probes are **phase-gated**: geometry/offset always; `paints` /
  `display_list` / layer probes require `>= Paint`; semantics probes require
  `Semantics`. Calling a probe before its phase panics with a message naming the
  phase needed and the phase reached (test-time misuse, fail loud).
- `LayoutRun`/`FrameRun` remain so existing tests and the catalog gate are untouched;
  `run_to(Paint)` is the new capability (paint without composite — the cheapest phase
  that produces a display list).

### A.2 — `paints` PaintPattern matcher

A fluent builder that subset-sequence-matches a `&[DrawCommand]`.

```rust
pub fn paints() -> PaintPattern;
pub fn paints_nothing() -> PaintPattern;     // asserts the command slice is empty

pub struct PaintPattern { /* ordered Vec<Step> */ }

impl PaintPattern {
    // shapes (all arg fields optional; None = "don't care")
    pub fn rect(self, m: RectMatch) -> Self;            // DrawRect
    pub fn rrect(self, m: RRectMatch) -> Self;          // DrawRRect
    pub fn circle(self, m: CircleMatch) -> Self;        // DrawCircle
    pub fn oval(self, m: RectMatch) -> Self;            // DrawOval
    pub fn path(self, m: PathMatch) -> Self;            // DrawPath (includes/excludes pts)
    pub fn line(self, m: LineMatch) -> Self;            // DrawLine
    pub fn arc(self, m: ArcMatch) -> Self;              // DrawArc
    pub fn drrect(self, m: DRRectMatch) -> Self;        // DrawDRRect
    // clips
    pub fn clip_rect(self, m: RectMatch) -> Self;       // ClipRect
    pub fn clip_rrect(self, m: RRectMatch) -> Self;     // ClipRRect
    pub fn clip_path(self, m: PathMatch) -> Self;       // ClipPath
    // text / image / effects / layers
    pub fn text_span(self, m: TextSpanMatch) -> Self;   // DrawTextSpan
    pub fn image(self, m: ImageMatch) -> Self;          // DrawImage*
    pub fn shadow(self, m: ShadowMatch) -> Self;        // DrawShadow
    pub fn save_layer(self, m: SaveLayerMatch) -> Self; // SaveLayer
    // escape hatches
    pub fn something(self, p: impl Fn(&DrawCommand) -> bool + 'static) -> Self;
    pub fn everything(self, p: impl Fn(&DrawCommand) -> bool + 'static) -> Self;
    pub fn exactly_n_times(self, kind: DrawSelector, n: usize) -> Self;

    // run the match; on failure produce a diff-style report (expected step, the
    // command it failed on, count of skipped commands, a dump of the full slice).
    pub fn matches(&self, commands: &[DrawCommand]) -> Result<(), PaintMismatch>;
}
```

- **Arg matchers** are small structs with optional fields and a builder (e.g.
  `RectMatch::new().rect(r).color(c).style(Fill).stroke_width(2.0)`), each field
  `Option<…>`; `None` skips. Colour compares with **1/255** per-channel tolerance;
  geometry with an epsilon. The per-command `transform` is matchable via an optional
  `transform: Option<Matrix4Match>` on every step (flui advantage over Flutter).
- **Subset-sequence semantics** (same as Flutter): steps must appear **in order**;
  non-matching commands between matched steps are skipped; trailing unmatched commands
  are allowed unless the pattern asserts otherwise. `paints_nothing()` requires empty.
- **Source — both (decision):**
  - *Isolation:* `RenderTester::record_paint(&Run, node) -> Vec<DrawCommand>` — paints
    **one** render object's `paint` into a `flui-painting` recording context at its
    committed offset, returning only its commands. The Flutter `paints` on a
    RenderObject. The primary, unit-style entry.
  - *Integration:* `FrameRun::display_list() -> &[DrawCommand]` (whole painted frame)
    and `FrameRun::paints_for(node) -> Vec<DrawCommand>` (the node's subtree slice,
    sliced by the save/restore + offset bracket the paint driver emits per node).
    Proves the object reaches the frame with the right parent transform/clip.
  - Ergonomic assert: `assert_paints(commands, paints()…)` free fn + `Run::assert_paints(node, …)`
    (isolation) and `FrameRun::assert_paints_frame(…)` (integration).

### A.3 — Error / assertion capture

```rust
impl RenderTester {
    pub fn try_run_to(self, phase: RenderPhase) -> Result<Run, RenderError>;
}
impl RenderTester { pub fn expect_layout_error(self) -> RenderError; } // asserts Err, returns it

// helpers
pub fn assert_overflow(run: &Run, node: RenderId);        // asserts an overflow RenderError/flag
pub fn assert_paints_panics(node_paint: impl FnOnce());   // paint asserts/panics (paintsAssertion)
```

- The harness already gets a `RenderResult` from the pipeline; `try_run_to` returns it
  instead of unwrapping. `expect_layout_error` is the inverse of the happy path for the
  "this misconfiguration must fail" tests.
- Overflow: surfaced from the render object's diagnostics/flag (e.g. flex/`has_visual_overflow`)
  — `assert_overflow` reads the committed geometry/flag, not a string.

## Public API surface (additions, all behind the `testing` feature / `cfg(test)`)

`flui_rendering::testing` gains: `RenderPhase`, `Run`, `RenderTester::{run_to, try_run_to,
record_paint, expect_layout_error}`, `paints`, `paints_nothing`, `PaintPattern` + the
`*Match` arg structs + `DrawSelector`, `assert_paints`, `FrameRun::{display_list,
paints_for, assert_paints_frame}`, `assert_overflow`, `assert_paints_panics`. The
matcher + arg structs live in a new `testing/paint.rs`; phases extend `testing/harness.rs`.

## Testing strategy

The harness is test-only, so it is validated by **its own tests** plus dogfooding:
1. `paints` unit tests: build known `DisplayList`s via `flui_painting::testing::record`
   and assert the matcher accepts/rejects (in-order, skip, arg tolerance, `nothing`,
   `exactly_n_times`, predicates, failure-report content).
2. Phase tests: `run_to(Paint)` produces a display list while `run_to(Layout)` does not;
   phase-gated probe misuse panics.
3. Isolation vs integration parity: `record_paint(node)` and `paints_for(node)` agree on
   a simple `RenderColoredBox` (modulo parent transform).
4. Dogfood: re-express ≥3 existing structure-only paint tests
   (`RenderColoredBox` colour, `RenderOpacity`, a clip object) as `paints` assertions,
   and add **op-level paint coverage for `RenderSliverListLazy`** (only visible+cache
   children emit draw commands — the virtualization win, now asserted at the paint
   layer, not just built-count).
5. `docs/TESTING.md` updated with the new sections + examples; the workspace doc gate
   (`cargo doc -D warnings`) and the catalog gate stay green.

## Risks & mitigations

- **Slicing a node's commands out of the integration display list** (`paints_for`) is
  the trickiest piece — the paint driver must emit a recognizable per-node bracket
  (save/restore + offset) to slice on. *Mitigation:* if the bracket is not already
  unambiguous, isolation (`record_paint`) is the primary path and `paints_for` is
  best-effort/documented-approximate; do not block A on perfect slicing.
- **Phase boundaries** must reuse production pipeline entry points exactly, or the
  harness tests a different code path than production. *Mitigation:* `run_to` calls the
  same `PipelineOwner` phase methods `run_frame` already composes; no bespoke phase
  logic.
- **Matcher scope creep** — the 29-variant enum tempts a method per variant.
  *Mitigation:* ship the high-frequency ~12 (rect/rrect/circle/oval/path/line/arc/drrect,
  clip_rect/rrect/path, text_span/image/shadow/save_layer) + the `something/everything`
  predicate escape hatch for the long tail; add variants on demand.

## Decomposition context (render-harness 2.0)

- **A (this doc)** — paint & phase verification. *Foundational.*
- **B** — finders (by-type/key/predicate/descendant/ancestor) + quantity matchers
  (`finds_one/nothing/n`) + geometry-by-finder + auto-fail tree/paint dump.
- **C** — gesture/pointer simulation through the gesture arena (`TestGesture`, tap/drag/
  fling/long-press) + `pump_and_settle`. Validates lazy-`SliverList` scroll.
- **D** — semantics-tree finders/matchers (labels/actions/flags), atop A's `Semantics` phase.

Each ships its own spec → plan → PR.

## Resolved decisions

- `paints` source: **both** isolation + integration (isolation primary).
- Phase API: `run_to(RenderPhase)` + retained `run_layout`/`run_frame` aliases; unified
  phase-gated `Run`.
- Matcher: fluent builder over the typed `DrawCommand` enum (not a dynamic mock canvas);
  per-command transform directly matchable.
- Scope: A excludes B/C/D and pixel goldens (YAGNI).
