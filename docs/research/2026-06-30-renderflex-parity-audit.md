# RenderFlex Flutter-parity audit (post-`.flutter/` restore)

**Date:** 2026-06-30
**FLUI:** `crates/flui-objects/src/layout/flex.rs` · **Oracle:** `.flutter/flutter-master/packages/flutter/lib/src/rendering/flex.dart`

Method: compare `perform_layout` / dry layout / intrinsics / baseline to the
oracle with a concrete differing input/output required per finding.

## Fixed

5. **MED — empty flex with `MainAxisSize::Max` collapsed the main axis.** The
   childless short-circuit returned `constraints.smallest()`; Flutter fills the
   bounded main extent (`idealMainSize = maxMainSize`). Empty Row (Max) under
   (0,500,0,300) → oracle (500,0), FLUI (0,0). **Fixed: `34c7c24c`** (+2 harness
   tests).

## Remaining — systemic / substantial (need a focused, careful effort)

1. **CRITICAL — dry layout returns `Size::ZERO`.** `RenderFlex` never overrides
   `compute_dry_layout` (trait default → `Size::ZERO`); oracle returns
   `_computeSizes(...).axisSize` (flex.dart:1079). Row with a 50×30 child,
   `getDryLayout(tight 500×300)` → oracle (500,300), FLUI (0,0). **Systemic:**
   `stack.rs` and `wrap.rs` *also* lack `compute_dry_layout` → the whole
   multi-child layout family measures zero under dry layout. A faithful fix needs
   the sizing extracted into a context-agnostic helper shared by `perform_layout`
   (real, `BoxLayoutContext::layout_child`) and `compute_dry_layout`
   (`BoxDryLayoutCtx::child_dry_layout`) — AND the dry ctx must expose child
   parent-data so flex factors are readable during dry layout (verify; if absent,
   this is an architectural gap, not just a flex fix). Do flex first to establish
   the pattern, then stack/wrap.

2. **HIGH — container reports no baseline.** No override of
   `compute_distance_to_actual_baseline` (or `compute_dry_baseline`) → `None`.
   Oracle: horizontal → highest, vertical → first child actual baseline
   (flex.dart:806). **Structural:** FLUI's `compute_distance_to_actual_baseline(&self, …)`
   has no child-access context and there is no multi-child
   `defaultComputeDistanceTo{Highest,First}ActualBaseline` helper — both need
   adding before flex can implement this.

3. **MED-HIGH — cross-axis intrinsics pass the container extent to every child.**
   `flex.rs:~247-262,565` measures each child at the full incoming cross extent;
   oracle runs a `_computeSizes`-style pass measuring non-flex children at their
   own max-intrinsic main and flex children at their allocated `spacePerFlex*flex`
   (flex.dart:734). Horizontal Row, child maxIntrinsicWidth 100,
   minIntrinsicHeight(100)=10 vs (50)=20; `computeMinIntrinsicHeight(50)` →
   oracle 10, FLUI 20.

4. **MED — baseline cross-extent (ascent+descent) not added to container cross
   size.** `flex.rs:~348-356,429` uses `max_cross` only; oracle accumulates
   `_AscentDescent` and adds `ascent+descent` for baseline-aligned flex
   (flex.dart:1227,1289). Baseline Row, A 20×30 (baseline 25), B 20×30 (baseline
   5) → oracle cross 50, FLUI 30 (B overflows).

6. **LOW / decision — default `FlexFit` is `Loose`, Flutter's null-fit default is
   `Tight`.** `box_variants.rs:~160`, `flex.rs:~304`. Only the raw
   `default().with_flex(...)` path diverges; the real constructors
   (`flexible()`=Tight=Expanded, `Flexible`=Loose) match Flutter, and whether the
   view layer takes the raw path is out of scope (flui-view untouched). Decide:
   align the raw default to Tight, or leave (low reachability).

## Also noted (paint-side, not layout)
`_overflow` is never computed and paint is a no-op, so an overflowing flex does
not clip or draw the overflow stripe (flex.dart:1337,1400). Layout *positions*
match (both clamp free space ≥0). Separate paint-pipeline work.

## Faithful (verified, no finding)
Two-pass proportional flex distribution (`spacePerFlex`/`maxChildExtent`);
`MainAxisAlignment` start/end/center/spaceBetween/spaceAround/spaceEvenly leading
+ between math incl. per-gap `spacing`; free-space clamp ≥0; `MainAxisSize`
ideal-size selection for non-empty flex; `can_flex` demotion under unbounded main;
cross offsets start/end/center/stretch + stretch's tight cross constraints;
main-axis intrinsics fold.

## Structural gap (not a constructible divergence)
No `textDirection`/`verticalDirection` on `RenderFlex` → `_flipMainAxis`/
`_flipCrossAxis` (RTL, `VerticalDirection.up`) absent. LTR/down matches; RTL/up
is a missing capability, not a confirmed divergence.

## Takeaway
Flex's *core distribution and main-axis alignment are faithful*. The gaps cluster
in **dry layout (systemic across flex/stack/wrap)**, **baseline reporting
(structural)**, and **intrinsic measurement** — each a focused effort, not a
quick fix. The restored oracle made all of them precisely characterizable.
