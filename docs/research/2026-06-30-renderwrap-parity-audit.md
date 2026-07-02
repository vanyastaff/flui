# RenderWrap Flutter-parity audit (post-`.flutter/` restore)

**Date:** 2026-06-30
**FLUI:** `crates/flui-objects/src/layout/wrap.rs` · **Oracle:** `.flutter/flutter-master/packages/flutter/lib/src/rendering/wrap.dart`

Traced the run-break threshold, container sizing, free-space distribution, cross
placement, intrinsics, and hit-test for the default LTR/down case.

## Fixed

1. **MEDIUM — max-intrinsic added inter-child spacing the oracle omits.**
   `compute_max_intrinsic_width/height` summed child max-intrinsic extents PLUS
   `spacing*(n-1)`; Flutter's `computeMaxIntrinsicWidth/Height` (wrap.dart) sums
   with no spacing term. Horizontal Wrap, spacing 10, children 30/50/40 → oracle
   120, FLUI 140. **Fixed `b5a585f4`** + `harness_wrap_max_intrinsic_width_omits_spacing`
   (red→green). (Min-intrinsic was already max-of-children, no spacing — matched.)

## Remaining — face of the secondary-query gap (architectural, not fixed here)

2. **LOW-MED — wrapping-direction intrinsics use unbounded proxies, not dry
   layout.** For cross-wrapping intrinsics (horizontal min/max height etc.),
   `simulate_wrap_cross` uses each child's `max_intrinsic_width(∞)` + cross probe
   instead of `getDryLayout(maxWidth = limit)`. Horizontal Wrap,
   `min_intrinsic_height(100)` with a flow child that is 1 line at width 200 /
   2 lines at width 100 → oracle 40, FLUI 20. Only manifests with
   constraint-dependent (flow) children; for fixed-size children the two coincide.
   This is the missing-`compute_dry_layout` / harness `getDryLayout` substitute —
   tracked in `2026-06-30-secondary-query-layout-gap.md`, not re-litigated here.

## Faithful (verified, default LTR/down)
Run-break condition + first-child-in-run spacing exemption (incl. precision
tolerance); per-gap spacing folded into run main extent; container size =
`max(run main) × (Σ run cross + runSpacing·(n−1))` then constrained;
`runAlignment`/`alignment` leading+between for all six WrapAlignment values;
spaceBetween `<2`-item fallback; main/cross free-space `max(0,·)` clamping;
`crossAxisAlignment` start/end/center; child constraints (loose cross, bounded
main); hit-test reverse paint order + own-size bounds.

## Missing capabilities (not divergences — FLUI's API can't reach them)
- No `textDirection`/`verticalDirection` → no `flipMainAxis`/`flipCrossAxis`
  (RTL / vertical-up); documented at `wrap.rs:15-19`. Only LTR/down audited.
- No `compute_dry_layout`/`computeDryBaseline` → secondary-query gap (drives #2).
- No `clipBehavior`/`_hasVisualOverflow` → on overflow, positions/size match
  Flutter but content isn't clipped. No position/size impact.

## Takeaway
Wrap's `perform_layout` positioning/sizing/hit-test is faithful for LTR/down; the
one clean bug (max-intrinsic spacing) is fixed; the only other divergence is the
already-tracked secondary-query gap surfacing in intrinsics.
