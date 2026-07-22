# Transform/fit render-object Flutter-parity audit (post-`.flutter/` restore)

**Date:** 2026-06-30
**FLUI:** `crates/flui-objects/src/layout/{fitted_box,transform,fractional_translation}.rs`
**Oracle:** `.flutter/flutter-master/packages/flutter/lib/src/rendering/proxy_box.dart` (+ `painting/box_fit.dart`)

Method: verify the actual arithmetic (matrix multiply order, `applyBoxFit`
source/destination math, alignment×free-space, hit-test inverse) against the
oracle. Notably the first hypotheses about `fitted_box`'s scale/alignment were
**refuted** by working the algebra — FLUI uses a different-but-equivalent
representation (source = full child + negative-free-space alignment folds the
crop offset into `align_offset`); identical output for all fits/alignments.

## Fixed (all three were clear divergences)

1. **HIGH — `RenderFittedBox` sized the box with a plain `constrain`, not
   aspect-preserving.** `perform_layout` used `incoming.constrain(child_size)`;
   Flutter uses `constrainSizeAndAttemptToPreserveAspectRatio` (ScaleDown loosens
   first). FLUI's own `compute_dry_layout` already did this, so wet ≠ dry. child
   100×50 under (0,60,0,∞) Contain → (60,30); wet returned (60,50). **Fixed
   `8947d6b1`** — extracted a shared `fitted_size` helper used by both
   perform_layout and compute_dry_layout so they can't drift again.

2. **HIGH — `RenderFractionalTranslation` hit-test gated on its own untransformed
   bounds.** A leading `is_within_own_size` check rejected pointers over the
   *shifted* child. Flutter's `hitTest` skips the self-bounds check (so does
   FLUI's own `RenderTransform`). translation (1,0), 80×40, pointer (120,20) →
   hit child at (40,20); FLUI returned false. **Fixed `ec9c9f4a`** (gate removed).

3. **MEDIUM — `RenderTransform` treated `origin` and `alignment` as mutually
   exclusive.** `compute_origin` returned `origin` when set, else the alignment
   offset; Flutter applies BOTH (`pivot = alignment.alongSize + origin`). With
   the default CENTER alignment, setting an origin silently dropped the center
   pivot. scale(2,2), 100×100, CENTER, origin (10,0) → pivot (60,50); FLUI used
   (10,0). **Fixed `61298797`**.

## Noted gaps (missing capability, not constructible divergences)
- `RenderTransform` has no `transformHitTests` flag (always transforms hit
  tests). Flutter's default is `true`, so the default matches; the `false` path
  is simply unreachable in FLUI — a missing feature, not a divergence.
- `RenderTransform::needs_compositing` defaults `true` vs Flutter's
  `child != null && filterQuality != null` — a compositing/layer-count strategy
  difference, not transform geometry. Out of scope.

## Faithful (verified, no finding)
`RenderFittedBox` scale + alignment transform (algebraically + numerically
equal to Flutter's `T(destRect)·S·T(-sourceRect)` for all fits/alignments);
`BoxFit::apply` scale factors; `has_visual_overflow`; FittedBox `compute_dry_*`;
FittedBox hit-test (own-size gate IS correct here — Flutter keeps default
`hitTest`) + inverse; `RenderTransform` size == child size, alignment-only and
origin-only effective matrices, hit-test inverse; `RenderFractionalTranslation`
paint offset (fraction×size), size == child size, half-open bounds.

## Takeaway
Three clear, contained bugs — one (FittedBox) caught by FLUI's own dry≠wet
inconsistency, two (FractionalTranslation, Transform) by Flutter's own
self-consistency (other types in the same family did it right). All fixed with
red→green tests. Heavy use of *refutation* (working the algebra) kept false
positives out — the fitted-box scale/alignment representation looked divergent
but is provably equivalent.
