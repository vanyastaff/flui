# Architectural synthesis: the "secondary-query-during-layout" gap

**Date:** 2026-06-30
**Source:** emerged from the post-`.flutter/`-restore render-object parity audits
(box, flex, transform/fit, intrinsics). The *contained* parity bugs were fixed
(13 commits); the **remaining** parity gaps cluster around one systemic
architectural area, documented here so they're tackled as one focused design
workstream rather than re-discovered piecemeal.

## The pattern

Flutter render objects routinely perform **secondary queries on their children
during (or instead of) layout** — `getDryLayout`, `getMaxIntrinsicWidth/Height`,
`getDistanceToBaseline`/`getDryBaseline` — and these must work the same whether
called in a real frame or a dry/measurement pass, and must see the children's
**parent data** (flex factors, positioning). FLUI's equivalent infrastructure is
**under-wired**, and several render objects either return wrong results or were
deliberately gated to avoid the gap. The individual symptoms:

1. **Multi-child dry layout returns `Size::ZERO`** (flex/stack/wrap; task #15 / 
   `renderflex-parity-audit.md` #1). Root: `BoxDryLayoutCtx`
   (`context/intrinsics.rs:174`) exposes `child_count` + `child_dry_layout` but
   **no child parent-data accessor** — so a flex can't read flex factors during
   dry layout. CONFIRMED architectural.

2. **Multi-child baseline reports `None`** (flex; task #15 #2). Root:
   `compute_distance_to_actual_baseline(&self, …)` has **no child-access
   context**, and there is no multi-child
   `defaultComputeDistanceTo{Highest,First}ActualBaseline` helper. Structural.

3. **`RenderIntrinsicWidth/Height` don't force the child to its intrinsic size by
   default** (task #16). Root: the intrinsic-query callback
   (`child_max_intrinsic_width`) "returns 0.0 in Direct-storage (test) contexts"
   (intrinsic_width.rs:224) — the live callback is only wired in the real
   pipeline — so the type was gated to only force width when `step_width` is set,
   leaving the default (no-step) case a no-op. Un-gating needs the intrinsic
   query wired (and testable) during layout across contexts.

## Why these are one problem

All three are the same shape: **a child query (dry size / intrinsic / baseline)
that needs to run during a layout-or-measure pass, with full access to the
child's parent data, in *every* context including the test harness.** FLUI's
layout/dry/intrinsic/baseline contexts (`crates/flui-rendering/src/context/`)
each expose a *subset* of these, and the test harness (`testing/`) wires only
some. Fixing them one render object at a time is wasteful and risks divergent
half-solutions; the leverage is in the context layer.

## Proposed workstream (chief-architect-level)

1. **Unify the "child query" capability.** Give the dry / intrinsic / baseline
   layout contexts a consistent way to (a) read a child's parent data and (b)
   recursively query the child's dry size / intrinsic / baseline — mirroring
   Flutter's `ChildLayoutHelper.{layoutChild,dryLayoutChild}` + the child's
   public `getXxx` methods backed by one `_computeSizes`-style routine.
2. **Wire it in the test harness** (`RenderTester`) so these queries return real
   values during a layout/measure pass — without which the fixes can't be
   red→green-tested (the current 0.0/`None`/`ZERO` returns).
3. **Then** land the per-render-object fixes that depend on it: flex/stack/wrap
   `compute_dry_layout`, flex baseline, IntrinsicWidth/Height default forcing —
   each verified against `.flutter/` with the now-working queries.

## Scope note

This is the dividing line between the *contained* parity work (done — 13 fixes,
verifiable with today's contexts) and the *architectural* parity work (this
note). The contained audits also surfaced genuinely **intentional** divergences
(SizedBox leaf-expand, FractionallySizedBox infinity-avoidance,
ConstrainedOverflowBox dry consistency — tasks #14/#15) which are maintainer
decisions, not part of this workstream.
