# 541 — Mouse and touch slop differ; stylus policy is explicit

## The reference rule

`gestures/events.dart` (tag `3.44.0`) has three free functions. Each special-cases
**exactly** `PointerDeviceKind.mouse` as "precise" and resolves every other kind —
stylus, trackpad, invertedStylus, unknown, touch — through the settings profile:

| function | mouse arm | every other kind |
|---|---|---|
| `computeHitSlop(kind, settings)` | `kPrecisePointerHitSlop` (1) | `settings?.touchSlop ?? kTouchSlop` (18) |
| `computePanSlop(kind, settings)` | `kPrecisePointerPanSlop` (2) | `settings?.panSlop ?? kPanSlop` (36→18 here) |
| `computeScaleSlop(kind)` | `kPrecisePointerScaleSlop` (1) | `kScaleSlop` (18) |

The mouse arm never consults `gestureSettings`, so a caller-supplied profile cannot
widen a mouse's tolerance. **Stylus is not precise** under this rule — that is the
"explicit stylus policy" the issue asks for, and it is the reference's answer, not
an omission.

## Triage — the gap is 3 recognizers, not 8

The issue's framing suggested every recognizer reading `touch_slop` was wrong. That
is false. `PrimaryPointerGestureRecognizer._defaultTouchSlop` resolves as
`gestureSettings?.touchSlop ?? kTouchSlop` with **no kind selection at all**, so
every recognizer built on it is already correct reading the touch tier unconditionally.

| recognizer | reference site | verdict |
|---|---|---|
| `tap`, `long_press`, `double_tap` | `PrimaryPointerGestureRecognizer` | **correct as-is** — no kind selection in the reference |
| `drag` | `monodrag.dart` per-axis | already kind-aware |
| `multidrag` | `multidrag.dart` | already kind-aware |
| `force_press` | `force_press.dart:252` `computeHitSlop` | **gap** — read `touch_slop()` for every kind |
| `multi_tap` | `multitap.dart:419` `computeHitSlop` | **gap** — same |
| `tap_and_drag` | `:532` hit + `:1445` pan | **gap** — both tiers, neither kind-aware |
| `scale` | `scale.dart:746` | **different shape** — see Out of scope |

## What this change does

1. **One home for the rule.** `GestureSettings::hit_slop(kind)` and
   `pan_slop_for(kind)` are `computeHitSlop`/`computePanSlop`. The rule was
   duplicated in `drag.rs` and `multidrag.rs` with no shared helper; wiring three
   more recognizers by copy would have made five copies of a one-line policy, which
   is how the three gaps stayed invisible in the first place.
2. **Wire the three gaps** onto those accessors.
3. **Collapse the two existing copies** onto them. `multidrag::slop_for` becomes
   `hit_slop(kind)` exactly. `drag::min_drag_distance` keeps FLUI's per-axis
   narrowing (`pan_slop_vertical`/`_horizontal`, which the reference has no
   equivalent of) but takes its kind split from the accessors.

`tap_and_drag` is the one that needs care: it uses **both** tiers, and they are not
interchangeable. The free-plane drag threshold is `computePanSlop`
(`TapAndPanGestureRecognizer`, `:1445`); the tap-viability threshold is
`computeHitSlop` (`:532`). The axis-locked variants at `:1410` take the *hit* tier
for their drag threshold — noted at `drag_slop`, because a future
vertical/horizontal mode on this recognizer must switch tiers, not reuse this one.

## Test design — why the sample distances are what they are

A slop test only discriminates in the band **strictly between** the two tiers. Below
both, or above both, the gesture resolves the same way whichever tier was read, and
the test passes with the fix reverted. Every test here places its probe in that band
and asserts the placement, so a later constant change cannot silently make the test
vacuous.

`tap_and_drag` carries a second constraint the others do not: its hit-tier probe must
also stay **under the mouse pan slop (2 px)**, or a mouse begins dragging at that
distance and the drag masks the tap-voiding the probe is meant to observe. That upper
bound is tighter than the touch hit slop (18 px), so the midpoint of the two hit tiers
does *not* qualify — the first draft of that test used it and failed.

Red evidence: each production line reverted independently, each with its test failing.

## Out of scope — filed separately

`scale.rs` is not a wrong-tier bug. The reference accepts a scale on a **three-way OR**
(`scale.dart:746`): absolute span delta past `computeScaleSlop`, **or** focal-point
movement past `computePanSlop`, **or** the ratio past 1.05. FLUI implements only the
ratio arm, which is dimensionless and so needs no kind. Two consequences follow, both
behavioural rather than tolerance-level:

- a pinch starting from a wide span never accepts on absolute movement (1000 px span,
  fingers 40 px apart → ratio 1.04, rejected);
- a two-finger **pan** — fingers moving together, ratio unchanged — cannot start the
  recognizer at all.

Related: `GestureSettings::scale_slop` is set by every profile constructor and read by
**nothing** in the workspace, and its value (0.05) is a *ratio* where the reference's
`kScaleSlop` is 18 logical *pixels*. Fixing it is a breaking change to a public
constant's meaning, and adding the two arms changes arena outcomes for two-finger
gestures — its own decision, its own PR.
