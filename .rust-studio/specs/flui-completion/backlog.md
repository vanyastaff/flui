# FLUI completion backlog (autonomous 12h run, 2026-06-26)

Living, dependency-ordered unit backlog for the 12-hour autonomous push. Each unit
ships via: **design (read-only) → build (worktree-isolated) → adversarial review →
curate-merge → commit, gates green**. Update `Status` as units land. Resume by
picking the top `OPEN` unit. See memory `autonomous-12h-completion-goal`.

Discipline: writers worktree-isolated ONLY; never blind-merge a branch (curate —
remove footguns, fix conflicts); verify `.flutter/` for parity; no stubs/todo!;
leapfrog where Flutter has no strong contract.

Workflow pattern (learned): **design/architect agents → FREEFORM text (no schema)**
— a strict multi-field schema makes the architect overflow the StructuredOutput
retry cap (it writes prose). Reserve JSON schemas for the BUILD result + REVIEW
verdict only. Reviewers diff `HEAD..<branch>` (NOT `main` — main is stale; my work
is on `core1-widgets-slice`). Curate per the review verdicts before merging.

## Wave A — gesture + animation completion (builds on flui-binding Phase 1)

| # | Unit | Status | Notes |
|---|------|--------|-------|
| A1 | **GestureDetector gesture completion** — `GestureArenaScope` InheritedView + `on_long_press` + `on_double_tap` via shared clock-bound arena. | DONE (`53bc652e`) | long-press, double-tap-alone, standalone all verified red→green. DEFERRED to A2: on_tap+on_double_tap combo (tap self-sweeps), multi-detector competition, binding-driven close/sweep (harness does it now). Docs made honest. |
| A2 | **Binding-driven arena lifecycle** — binding drives close/sweep; recognizers don't self-sweep the shared arena; double-tap hold/release. | DONE (`f0b10587` + fix merge) | on_tap+on_double_tap fires double-tap once; multi-detector competition; restart-leak HIGH fixed; sweep-only-on-up parity; 461/461. SweepModel on arena. |
| A2b | **HeadlessBinding full frame driver** — binding owns an optional mounted tree (BuildOwner+ElementTree+PipelineOwner); `pump_frame` adds `build_scope`+`run_frame` after the deadline poll; **restart-aware** controller registry (fix the Phase-1 run-epoch desync — detect a controller's re-run and reset its epoch). Test: a FadeTransition driven by a controller advances frame-to-frame via pump_frame; a setState rebuild propagates. | OPEN | connects existing harness pieces; unblocks A3 implicit animations |
| A3 | **TickerProvider + implicit animations (Phase 3)** — view-layer vsync (`SingleTickerProviderStateMixin` analogue) + restart-aware controller driving; `AnimatedOpacity`, `AnimatedContainer`, `AnimatedAlign`, `AnimatedPadding`. | OPEN | flagship; needs A2; leapfrog: better default curves + deterministic test via binding |

## Wave B — catalog completeness (mostly independent)

| # | Unit | Status | Notes |
|---|------|--------|-------|
| B1 | **Image + ImageProvider** — decode/cache infra + `Image` widget over `RenderImage` (currently MVP). | OPEN | needs image-decode; check flui-assets |
| B2 | **ClipPath + CustomClipper** — Path-based clipper trait + `ClipPath` widget over `RenderClip`. | OPEN | check Path infra in flui-geometry/painting |
| B3 | **Wrap** — `RenderWrap` (run-based flow layout) + `Wrap` widget. | OPEN | new render object |
| B4 | **IntrinsicWidth/IntrinsicHeight, OverflowBox, SizedOverflowBox, RotatedBox** widgets + render objects. | OPEN | several small layout render objects |

## Wave C — scrolling / perf leapfrog

| # | Unit | Status | Notes |
|---|------|--------|-------|
| C1 | **Lazy slivers** — `SliverChildBuilderDelegate` + a `SliverMultiBoxAdaptorElement` (build children on demand during layout). | OPEN | hardest scrolling element; perf leapfrog vs eager |
| C2 | **Scroll physics + Scrollbar + RefreshIndicator**. | OPEN | |

## Wave D — big competitive features

| # | Unit | Status | Notes |
|---|------|--------|-------|
| D1 | **TextField / text input** — focus tree, IME, selection, cursor. | OPEN | large; check flui-interaction focus |
| D2 | **Theme / MediaQuery / responsive** — inherited theming. | OPEN | |

## Done this session (pre-run)
- pan/drag + arena reject fix (`4fea0fbe`); Phase 0 clock + long-press (`4b8f3ff6`);
  dyn-sanction (`82f4b36c`); double-tap clock (`7c141277`); port-check ~4-6x
  (`3357caf4`); flui-binding Phase 1 (`9813e64c`); VERSION-test de-brittle (`23882bc3`).
- (kimi) on_secondary_tap, changelogs, 0.2.0 bump.
