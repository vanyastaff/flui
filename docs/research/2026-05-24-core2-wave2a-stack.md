# Core.2 Wave 2a — `RenderStack` + `PositionedSpec` typed view

**Phase:** Core.2 (Render-Object Catalog) — Wave 2a
**Date:** 2026-05-24
**Scope:** one new file (`crates/flui-rendering/src/objects/stack.rs`)
hosting `RenderStack` and its `PositionedSpec` helper
**Result:** ✅ all gates green

---

## Goal

Land the first multi-child render object beyond `RenderFlex` —
`RenderStack`. Stack is the foundation for every UI primitive that
layers content: overlays, dialogs, drawer scrims, snackbars, hero
animations, `IndexedStack`, badges, and any composite-layout
component in Material / Cupertino.

The implementation also stress-tests the `Variable` arity contract
on a layout shape that `RenderFlex` did not exercise — namely
"two coexisting layout flows on the same set of children":
non-positioned (auto-sized + aligned) **and** positioned (explicit
top/right/bottom/left/width/height anchors), where one flow's output
feeds the other's input.

## What was built

Single file `crates/flui-rendering/src/objects/stack.rs` containing:

| Item | Role |
|---|---|
| `pub use flui_types::layout::StackFit` | re-export of the existing fit enum (`Loose`, `Expand`, `Passthrough`) |
| `struct PositionedSpec` (public) | typed view over `StackParentData`'s six positioning fields |
| `PositionedSpec::from_parent_data(&StackParentData) -> Option<Self>` | constructor that returns `None` for non-positioned children — the discipline that lets layout branch on `Option<PositionedSpec>` instead of re-reading optional fields |
| `PositionedSpec::child_constraints(stack_size: Size) -> BoxConstraints` | ports Flutter's `layoutPositionedChild` constraint derivation (paired-edge tighten or explicit width/height) |
| `PositionedSpec::child_offset(stack_size, child_size, alignment) -> Offset` | computes the child's top-left position per Flutter's edge-anchor + fallback-alignment rules, per axis independently |
| `fn alignment_along_axis(component: f32, free: Pixels) -> Pixels` | private helper that maps `Alignment` scalars `[-1, 1]` to `[0, free]`, used by both positioned-fallback and non-positioned alignment |
| `struct RenderStack` | the render object, Variable arity, `StackParentData` |
| `RenderStack::new()` (const), `with_fit`, `with_alignment`, `with_clip_behavior` (const builders) | ergonomic constructors |
| `RenderStack::set_*(...) -> bool` | mutators that return change-flag for pipeline short-circuit |
| `RenderStack::has_visual_overflow() -> bool` | post-layout query — observable for tests/diagnostics |
| Two-pass `perform_layout` | Pass 1 sizes the stack from non-positioned children; Pass 2 positions both kinds |
| `paint` | applies `clip_behavior` only when overflow happened AND the user opted into clipping |
| `hit_test` | reverse-order child test (top-most first), Flutter parity |

Plus wiring: `objects/mod.rs` registers `stack` and re-exports
`PositionedSpec`, `RenderStack`, `StackFit`.

LOC: **~700 (file)** / **21 new tests**.

## Rust-native improvement (the headline)

Flutter's `RenderStack` decides each child's layout by **reading raw
optional fields off the child's `StackParentData`** at every site that
needs the decision. The fields are all `double?`; "did the caller
actually opt into positioning?" lives in
`StackParentData.isPositioned`, which re-reads the same optional
fields:

```dart
// Flutter — every layout step re-asks parent_data:
final StackParentData childParentData = child.parentData! as StackParentData;
if (!childParentData.isPositioned) {
  // non-positioned flow ...
} else {
  _hasVisualOverflow = layoutPositionedChild(
    child, childParentData, size, alignment,  // ← passes whole parent_data
  ) || _hasVisualOverflow;
}

static bool layoutPositionedChild(
    RenderBox child, StackParentData childParentData,
    Size size, AlignmentGeometry alignment) {
  // re-reads childParentData.left, .right, .top, .bottom,
  // .width, .height ad-hoc throughout the function body...
}
```

The Rust port lifts that bimodal decision into a **typed view** —
`PositionedSpec` — that:

1. **Cannot exist for a non-positioned child.**
   `PositionedSpec::from_parent_data` returns `None` when
   `!pd.is_positioned()`. The caller (RenderStack's `perform_layout`)
   stores it as `Vec<Option<PositionedSpec>>` and `match`es on each
   slot.
2. **Carries typed `Pixels` for every field**, not raw `f32`.
3. **Owns the constraint-derivation and offset-derivation math** as
   methods. The render object's `perform_layout` body never re-reads
   the underlying `StackParentData` after the initial snapshot — the
   `PositionedSpec` is sufficient.
4. **Is constructed once per child per layout** (Pass 1) and reused
   in Pass 2. Flutter's algorithm re-reads `childParentData` in both
   passes implicitly via the `child.parentData!` cast.

The optional-field tangle (`Option<f32>`-everywhere) is preserved in
`StackParentData` for back-compat with the existing parent-data
machinery, but the layout/paint/hit-test code never sees it.

This pattern — **"lift bimodal child layouts into a sum type"** — is
directly reusable for the other Wave 2 render objects:

* `RenderWrap` has wrap-line vs leftover-on-line semantics —
  a `WrapItemSpec { first_in_line: bool, x_offset: Pixels, y_offset: Pixels }`
  helper would do the same thing.
* `RenderTable` has column-span / row-span / explicit-width
  decisions per cell — a `CellSpec` typed view collapses those.
* `RenderFlex` already has flex / non-flex bimodal layout (Pass 1
  non-flex, Pass 2 flex). It could profitably be refactored to a
  `FlexChildSpec { flex: Option<NonZeroU32>, fit: FlexFit }` for
  consistency.

Other Rust niceties carried forward from Waves 1 / 3a:

* `const fn` builders — `RenderStack::new()`, `.with_fit(...)`,
  `.with_alignment(...)`, `.with_clip_behavior(...)` compose at
  compile time.
* Setters return `bool` — pipeline can skip `mark_needs_layout` on
  no-op writes.
* `has_visual_overflow()` is a **method**, not a private field —
  observable for tests / debug overlays / diagnostic dumps without
  touching painting. Flutter exposes the same flag only indirectly
  through the clipBehavior-gated paint path.
* `Diagnosticable` dump includes `fit`, `alignment`, `clip_behavior`,
  `size`, `has_visual_overflow`, `child_count`.

## Behavior loyalty

`RenderStack::perform_layout` is a step-for-step port of Flutter's
`RenderStack.performLayout`:

| Flutter step | FLUI step |
|---|---|
| No children → `size = biggest.isFinite ? biggest : smallest` | identical |
| Build `nonPositionedConstraints` from `StackFit` | `non_positioned_constraints` method, identical match table |
| Pass 1: layout each non-positioned child, accumulate `(width, height) = max(...)` | identical, accumulating into `(content_w, content_h)` |
| Resolve `size` = `(width, height)` if any non-positioned, else `biggest`/`smallest` fallback | identical |
| Pass 2: non-positioned → `alignment.alongOffset(size − child.size)`; positioned → `layoutPositionedChild(...)` | identical, but `layoutPositionedChild` body is replaced by `PositionedSpec.child_constraints` + `.child_offset` |
| Set `_hasVisualOverflow = layoutPositionedChild(...) || _hasVisualOverflow` | identical, computed via `child_overflows(...)` helper |

Paint behavior matches Flutter:
* No overflow OR `clip_behavior == Clip::None` → paint children
  directly (no canvas save).
* Otherwise → `canvas.save()`, `clip_rect_ext(bounds,
  ClipOp::Intersect, clip_behavior)`, paint children, `restore()`
  (via `BoxPaintContext::with_save`).

Hit testing matches Flutter:
* Out of bounds → miss.
* Otherwise test children in **reverse order** (top-most first) at
  the cached offsets from layout.

## Gates

| Gate | Command | Result |
|---|---|---|
| Build | `cargo check -p flui-rendering` | ✅ |
| Workspace build | `cargo check --workspace` | ✅ |
| Tests | `cargo test -p flui-rendering --lib` | ✅ **362 passed (+21 new)** |
| Lints | `cargo clippy -p flui-rendering --all-targets -- -D warnings` | ✅ clean |
| Format | `cargo fmt -p flui-rendering --check` | ✅ |
| Port-check refusal triggers | `bash scripts/port-check.sh` | ✅ **13/13 + FR-033 clean** |

## Coverage delta

* **Render objects implemented:** 15 → **16**
  (Wave 2a adds one render object — `RenderStack` — plus the
  reusable `PositionedSpec` helper that future `Positioned` widget
  wiring will build on).
* **Multi-child layout family:** 2/8 done (`RenderFlex` + `RenderStack`).
* **Widget catalog unblocked:** `Stack`, `IndexedStack`, `Positioned`
  (once a `ParentDataWidget` wrapper lands in `flui-widgets`), plus
  every Material/Cupertino primitive that overlays content
  (overlays, dialogs, drawer scrim, snackbar, hero, badge, …).

## Caveat / follow-up

* `PositionedSpec::child_constraints` uses `(stack_size.width - l - r).max(0)`
  via the existing `Pixels`-typed `max(Pixels::ZERO)` API — Flutter
  asserts `>= 0` and lets layout proceed with the clamped value;
  we silently clamp (Flutter parity for release-mode behavior).
  Adding a `tracing::warn!` on negative gap is a one-line follow-up
  if diagnostic observability is needed.
* `RenderPositioned` (the `ParentDataWidget` that sets `StackParentData`
  fields from a `Positioned`-typed widget) is not part of Wave 2a —
  it belongs in `flui-widgets` (Business.1) and uses
  `PositionedSpec` as the typed view back. The render-object side
  is now complete.

## Next steps

Per the wave plan in
[`widget-renderobject-map.md`](widget-renderobject-map.md):

* **Wave 2b** — `RenderWrap` / `RenderTable` / `RenderListBody`.
  All Variable arity; `PositionedSpec`-style typed views recommended
  per family.
* **Wave 3b** — `RenderDecoratedBox` + `RenderRepaintBoundary`
  (needs painting/layer infra).
* **Wave 4** — pointer + simple proxy.
* **Wave 7** — `RenderParagraph` (Core.1 critical path).

Wave 2a, 2b, 3b, 4 share no files; remain independently
parallelizable.
