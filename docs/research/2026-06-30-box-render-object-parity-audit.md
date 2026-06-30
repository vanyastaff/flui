# Box render-object Flutter-parity audit (post-`.flutter/` restore)

**Date:** 2026-06-30
**Trigger:** `.flutter/` oracle restored → first systematic parity audit of the
box-layout render objects (`crates/flui-objects/src/layout/`) against
`.flutter/flutter-master/packages/flutter/lib/src/rendering/{box,proxy_box,shifted_box}.dart`.

The grid geometry delegate audit earlier the same day found 5 real divergences;
this audit covers the box render objects. Method: compare each `perform_layout`
/ `compute_dry_layout` / intrinsic / baseline against the oracle, with a concrete
differing input/output required for each finding (no speculation).

## Fixed (clear divergences, no intentional-divergence flag)

1. **HIGH — `BoxConstraints::enforce` clamped the wrong direction.** `a.enforce(b)`
   must clamp `a`'s own values into `b`'s range (oracle `box.dart:222`); FLUI
   clamped `b` into `a`. At `additional.enforce(parent)` the additional
   constraints overrode the parent's hard limits → `RenderConstrainedBox` children
   overflowed and dry size was wrong on non-overlapping ranges. **Fixed: `0ea900ee`**
   (workspace nextest 3160/0).

2. **MED-HIGH — `RenderSizedOverflowBox` intrinsics delegated to child.** Oracle
   overrides all four intrinsics to `requested_size` (`shifted_box.dart`); FLUI
   returned the child's intrinsics (0 when childless). **Fixed: `9bbb38ac`.**

3. **MED — `RenderBaseline` laid child out un-loosened.** Oracle uses
   `constraints.loosen()` (`shifted_box.dart`); FLUI passed the incoming
   constraints, stretching a small child under a tight axis. **Fixed: `efc886ad`.**

## Remaining — decision required (plausibly intentional Rust-native divergences)

These are **not** force-fixed: matching Flutter could revert a deliberate
safety/leaf design. They need a maintainer call (match the oracle vs. keep the
divergence and document it). All currently claim or imply parity, so at minimum
the docs should be made honest.

4. **`RenderSizedBox` — unset axis (`None`) fills to `max`, not min/pass-through.**
   `sized_box.rs:74-84,101-109`. FLUI's `RenderSizedBox` is a `Leaf` modelling
   `None` as "expand" (`expand()` = `(None,None)`), unlike Flutter's `SizedBox`
   (= `RenderConstrainedBox(tightFor(...))`, `null → 0..∞ loose`, childless → min).
   Concrete: `SizedBox(width:100)` no child under `(0,200,0,200)` → oracle `(100,0)`,
   FLUI `(100,200)`. *Plausibly intentional leaf-expand design.*

5. **`RenderFractionallySizedBox` — factor axis falls back to `min` when incoming
   `max` is infinite.** `fractionally_sized_box.rs:221-243`. Oracle does
   `max * factor` unconditionally (relies on ∞ absorption). Concrete: incoming
   `(minW 30, maxW ∞)`, `width_factor 2.0` → oracle child width ∞, FLUI 60.
   *Plausibly intentional infinity-avoidance, but the module doc claims
   "behavior-faithful" — that claim is currently false for this case.*

6. **`RenderConstrainedOverflowBox` (`DeferToChild`) dry layout uses inner
   constraints + re-constrain.** `overflow_box.rs:267-281`. Oracle's dry path
   passes the **outer** constraints and returns the child's dry size directly
   (`shifted_box.dart:737`), which is itself inconsistent with Flutter's own
   `performLayout` (inner constraints). FLUI is internally self-consistent (both
   inner) but diverges from the oracle's dry path. Concrete: override `maxW 50`,
   incoming `(0,200,…)`, child intrinsic 100 → oracle dry 100, FLUI dry 50.
   *Matching the oracle would reintroduce Flutter's own dry/layout inconsistency.*

## Remaining — low severity, deferred

7. **`RenderBaseline::compute_dry_baseline` ignores cross-kind queries.**
   `baseline.rs:107-118`. Oracle returns `baseline + result1 − result2` where
   `result1`/`result2` are the child's dry baseline for the requested kind and the
   box's `baselineType` (both loosened) (`shifted_box.dart:1601`). FLUI returns
   `None` for a different kind and `baseline_offset` directly otherwise (un-loosened).
   Concrete: box `Alphabetic`, query `Ideographic`, child alpha 18 / ideo 20,
   offset 50 → oracle 52, FLUI `None`. Real but low-traffic; needs child dry
   baseline for two kinds.

## Already-correct (verified faithful, no finding)

`aspect_ratio.rs` (4-step clamp), `padding.rs` (deflate ≡ oracle since maxW≥minW),
`limited_box.rs` (unbounded-only capping), `align.rs`/`center.rs`
(`alongOffset`, `loosen()`, factor intrinsics), `constrained_box.rs` intrinsics,
`RenderConstrainedOverflowBox` `perform_layout` + `Max`-fit paths,
`RenderSizedOverflowBox` `perform_layout`/dry size, `fractionally_sized_box`
intrinsics + `align_child`, and `BoxConstraints::{constrain,loosen,deflate,
smallest,biggest,has_tight_*}`.

## Takeaway

The carefully-ported render objects hold up; divergences cluster in (a) a shared
primitive used before its semantics were oracle-checked (`enforce`), (b) a type
that overrides defaults (`RenderSizedOverflowBox`), and (c) deliberate
Rust-native safety choices (#4/#5/#6) that should be either matched to the oracle
or documented as intentional. The restored `.flutter/` oracle is what made all of
this detectable.
