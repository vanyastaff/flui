<!-- Rust Code Studio concept note (/brainstorm → product-steward capture). One page; no code. -->

# Concept: RotatedBox baseline is parity-only

*Decide, record, and pin what `RenderRotatedBox::compute_dry_baseline` reports for a rotated child — issue #1011, part 2.*

Captured 2026-09-10 by `product-steward`. Status: **concept approved in conversation; not yet a spec, not yet implemented.**

---

## Pre-solution statement (verbatim, from the brainstorm's step-3 header)

> Goal: decide what `RenderRotatedBox::compute_dry_baseline` reports for a rotated child, given that Flutter 3.44.0's `RenderRotatedBox` has no baseline override (null for every turn) and FLUI today forwards the child's baseline for even turns and reports None for odd turns with a comment that falsely claimed parity. Scope: one render object in flui-objects plus its accounting; no widget API change. Constraints: #1012's composited-layer fast path requires layout to be turn-blind up to parity (pinned by `harness_rotated_box_layout_is_turn_blind_up_to_parity`); AGENTS.md Prime Directive rule 1 — a divergence must be an improvement, recorded in a `## Mapping decisions` entry with a replacement test. Non-goals: changing `RenderTransform` (draw-time rotation, already forwards the baseline via `forward_single_child_box_queries!`), touching the childless branch (fixed in #1016).

## Problem / opportunity

Today `compute_dry_baseline` in `crates/flui-objects/src/layout/rotated_box.rs` forwards the child's dry baseline for even turns and returns `None` for odd turns, under a comment that says the design call "has not been made" and points at #1011. The upstream reference at the pinned tag (`.flutter/packages/flutter/lib/src/rendering/rotated_box.dart`, 3.44.0 — verified with `git -C .flutter describe --tags`) has no baseline override at all, so every turn falls through to `RenderBox`'s `null`. FLUI's behavior is therefore an unrecorded divergence: neither a port nor a documented improvement, which is exactly the "MVP reported as parity" failure `AGENTS.md`'s Definition of Done names. A baseline-aligned `Row` containing a `RotatedBox` currently gets an answer nobody has committed to.

## Who it's for

Framework users composing `RotatedBox` inside baseline-aligned containers (`Row` with `CrossAxisAlignment::baseline`, text-adjacent icons rotated by 180°), and the maintainers who need the divergence ledger to be complete before #1012's fast path is trusted.

## Concept note (verbatim, as approved)

> Chosen: keep the parity-only baseline for `RenderRotatedBox` — even turns forward the child's dry baseline unchanged, odd turns report `None` — and record it as a deliberate divergence from Flutter 3.44.0, whose `RenderRotatedBox` has no baseline override and reports `null` for every turn. Rationale: a baseline is a layout line; an even turn keeps the box's size and horizontal axis, so the box takes part in baseline alignment exactly as its unrotated self would (glyphs flip in place) — the same model draw-time rotations use in Compose (`Modifier.rotate`), SwiftUI (`.rotationEffect`), and FLUI's own `RenderTransform` — while an odd turn rotates the baseline axis away, leaving no horizontal baseline. Rejected: match upstream (`None` everywhere — loses the identity case for no gain); identity-only and geometric-mirror (`height − b` at turn 2) — both read the exact turn, breaking 0↔2 turn-blindness and costing `set_quarter_turns` and the premise pin extra cases for a runtime flip that does not occur (rotation animations use `RenderTransform`). Constraints respected: the answer reads only parity, so the turn-blindness pin and `COMPOSITED_LAYER_UPDATE` for 0↔2 / 1↔3 are unchanged. Deliverables: a `## Mapping decisions` entry (flui-objects has no `ARCHITECTURE.md`; create one with that section as the home for render-object contract divergences), a replacement harness test pinning a baseline-aligned `Row` placing `RotatedBox(2)` where `RotatedBox(0)` sits and `RotatedBox(1)` at the top, the `compute_dry_baseline` comment rewritten from 'design call not made' to the decision, and the module doc's 'behavior-faithful port' amended. Risk: creating a crate-level `ARCHITECTURE.md` is a doc-structure call. Assumption: upstream's `null` is an oversight, not a contract. Next: `/dev-task` fast path — test + docs, no logic change.

## Scope

**In:**
- `RenderRotatedBox::compute_dry_baseline` — no logic change; the existing parity-only answer becomes the decision.
- The accounting rule #1 of the Prime Directive owes: a `## Mapping decisions` entry, the replacement test (a baseline-aligned `Row` oracle: `RotatedBox(2)` lands where `RotatedBox(0)` does, `RotatedBox(1)` sits at the top), the rewritten in-code comment, and the module-doc amendment ("behavior-faithful port" is no longer true of this method).

**Out (explicitly, for now):**
- `RenderTransform` (draw-time rotation; forwards the baseline via `forward_single_child_box_queries!` and stays that way).
- The childless branch (`ctx.child_count() == 0 → None`, fixed in #1016).
- Any widget-layer API change; any change to `set_quarter_turns` impact classes or the turn-blindness pin.

## Constraints & unknowns

- **Turn-blindness up to parity is load-bearing.** `harness_rotated_box_layout_is_turn_blind_up_to_parity` (`crates/flui-objects/tests/render_object_harness.rs`) pins that layout, dry layout, intrinsics, and dry baseline read `quarter_turns` only via `is_vertical()`; #1012's `COMPOSITED_LAYER_UPDATE` route for 0↔2 / 1↔3 depends on it. The chosen answer reads parity only, so the pin is untouched.
- **Reference is verified, not assumed.** `.flutter` sits at `3.44.0`; `rotated_box.dart` has no `computeDryBaseline`/`computeDistanceToActualBaseline` override (zero `baseline` hits in the file).
- **Open doc-structure question (steward observation, not part of the approved note):** the note proposes creating `crates/flui-objects/ARCHITECTURE.md`. In the tree today, `crates/flui-rendering/ARCHITECTURE.md` `## Mapping decisions` already hosts flui-objects render-object divergences — including the `RenderRotatedBox` `set_quarter_turns` entry and the `RenderTransform` entry under "A composited-layer update patches the enclosing capture". Two homes are possible: (a) a new `flui-objects/ARCHITECTURE.md` (the note's proposal; establishes the crate's own ledger, but flui-objects also has no `AGENTS.md`, so it would be the crate's first architecture doc), or (b) a sibling entry beside the existing `RenderRotatedBox` entry in flui-rendering's file (zero structure change, keeps the rotated-box story in one place). Steward recommendation: (b) for this slice — it is the smaller ripple and matches where the reader already finds `RenderRotatedBox`; (a) is a separate decision for `chief-architect` if the flui-objects ledger is wanted on its own. Decide at `/dev-task` start, not mid-implementation.
- **Assumption to state in the mapping entry:** upstream's `null` is an oversight, not a contract — no Flutter test in `.flutter/packages/flutter/test/` asserts a `RotatedBox` baseline (so there is no oracle to replace, only one to add).

## Options weighed

| Option | Trade-off | Verdict |
|--------|-----------|---------|
| Match upstream: `None` for every turn | Zero divergence to record; loses the identity case (turn 0 really is unchanged) for no gain | rejected |
| Identity-only: forward at turn 0, `None` otherwise | Reads the exact turn → breaks 0↔2 turn-blindness; extra `set_quarter_turns`/pin cases for a flip that never happens at runtime | rejected |
| Geometric mirror: `height − b` at turn 2 | Same exact-turn read as above; "baseline distance from the top" of an upside-down box is a definition nobody consumes | rejected |
| **Parity-only: forward for even, `None` for odd** (today's code, now decided) | Reads parity only; matches Compose/SwiftUI/`RenderTransform` draw-time model; needs the accounting (entry + test + comment + module doc) | **chosen** |

## Chosen direction & next step

Parity-only, recorded as a deliberate improvement. Next: `/dev-task` on the fast path — replacement harness test, mapping entry (home per the open question above), comment and module-doc rewrite; no logic change to `compute_dry_baseline`. Acceptance: the new test fails if either branch of `compute_dry_baseline` is flipped; `harness_rotated_box_layout_is_turn_blind_up_to_parity` still green; `just ci` green.

## Outcome (what the implementation found)

The "no logic change" premise did not survive first contact: the even-turn passthrough existed
only in `compute_dry_baseline`, and a baseline-aligned `Row` asks the LIVE query, which the
layout driver answers by walking to the child only when `forwards_baseline_to_only_child()`
says so — `RenderRotatedBox` never overrode it, so a real row top-aligned even `RotatedBox(0)`.
The replacement oracle went red at turn 0 on its first run. The fix adds
`forwards_baseline_to_only_child = !is_vertical()`, so both halves of the query answer from the
same parity predicate.

Mapping-entry home: option (b) — beside the existing `RenderRotatedBox` entry in
`crates/flui-rendering/ARCHITECTURE.md`; no flui-objects ledger was created.

Corrected acceptance: `harness_rotated_box_baseline_follows_the_child_for_even_turns_and_is_absent_for_odd`
pins the live half by `Row` offsets (turns 0/2 at the reference dy, turns 1/3 at the cross
start) and the dry half by value at all four quadrants; verified red under three mutants — live
flag refused, live flag granted for odd turns, dry answer `None` for every turn (upstream's).
