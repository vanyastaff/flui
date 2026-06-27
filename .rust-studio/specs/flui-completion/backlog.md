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
| A2b | **HeadlessBinding full frame driver** — binding owns an optional mounted tree (BuildOwner+ElementTree+PipelineOwner); `pump_frame` adds `build_scope`+`run_frame` after the deadline poll; **restart-aware** controller registry (fix the Phase-1 run-epoch desync — detect a controller's re-run and reset its epoch). Test: a FadeTransition driven by a controller advances frame-to-frame via pump_frame; a setState rebuild propagates. | DONE | `HeadlessBinding::with_tree` + `register_controller`; `AnimationController::run_generation()` chokepoint in `restart_ticker`; re-anchor on generation bump. Discriminating tests: `second_run_ticks_from_its_own_start` (forward→reverse no stale-anchor snap), `registered_controller_advances_fade_opacity` (FadeTransition E2E through pump_frame), `run_generation_bumps_per_run_not_per_tick`. 264 widgets + binding green; port-check + clippy clean. |
| A3 | **TickerProvider + implicit animations (Phase 3)** — view-layer vsync (`SingleTickerProviderStateMixin` analogue) + restart-aware controller driving; `AnimatedOpacity`, `AnimatedContainer`, `AnimatedAlign`, `AnimatedPadding`. | DONE | `Vsync` registry (flui-animation) driven by binding; `VsyncScope` InheritedView (mirrors GestureArenaScope); `AnimatedBuilder` general primitive; implicit widgets = StatefulView holding persistent controller, build→AnimatedBuilder, `did_update_view(old,new)` retargets tween. NO flui-view surgery (controller's stable notifier reused via AnimatedBuilder). `did_update_view` gained `new_view` param (Flutter `this.widget`). 6 discriminating tests (interpolate/retarget-midflight/first-frame-no-motion ×4 widgets) + 3 Vsync unit tests. 714 + 3 green; fmt/clippy/port-check clean. AnimatedContainer animates alignment/padding/color/width/height/margin (decoration/constraints/transform pass-through, follow-up). Curve typed `Cubic` (all `Curves::Ease*`); elastic/bounce = type-erasure follow-up. |

## Wave B — catalog completeness (mostly independent)

| # | Unit | Status | Notes |
|---|------|--------|-------|
| B1 | **Image + ImageProvider** — decode/cache infra + `Image` widget over `RenderImage` (currently MVP). | DONE (merge `45f884fc` + tests) | `ImageProvider` trait (sync `resolve()`, documented async extension point) + DirectImageProvider/MemoryImage/FileImage/NetworkImage(typed `AsyncNotWired` error); `Image` widget over RenderImage (fit/alignment/width/height); decode behind optional `images` feature. Curated: agent's `decode_bytes` uses `image` crate DIRECTLY (bypasses flui-assets decode — boundary dup, follow-up) AND was untested → I added 2 decode unit tests (PNG dimension round-trip + garbage→DecodeFailed). 7 widget + 2 decode tests; 105 default green, clippy/fmt/port-check clean. DEFERRED: async/network/cache (no view-layer async yet). |
| B2 | **ClipPath + CustomClipper** — Path-based clipper trait + `ClipPath` widget over `RenderClip`. | DONE | Infra already existed (`RenderClipPath = RenderClip<Path>` + `with_clipper(Fn(Size)->Path)` closure clipper); only the `ClipPath` widget was missing. Added widget (clip_behavior defaults `Clip::AntiAlias` per Flutter) + pass-through layout test. Did it directly (small wiring, curate-overhead > build). clippy(pedantic)+fmt+port-check clean. Note: closure clipper can't diff identity (no `shouldReclip`) — reinstalled each update. |
| B3 | **Wrap** — `RenderWrap` (run-based flow layout) + `Wrap` widget. | DONE (merge of `1f5f6e11`) | RenderWrap (flui-objects) + Wrap widget; perform_layout verified 1:1 vs `.flutter/.../wrap.dart` (run-building, runAlignment/alignment/crossAxisAlignment, spaceBetween/Around/Evenly, vertical); reused existing WrapParentData/Axis/WrapAlignment. Catalog + 6 harness + 10 widget tests. 483 obj+widgets green; clippy/fmt/port-check clean. RTL deferred (TextDirection not plumbed, parity w/ flex). Built by worktree-isolated agent, curated+gate-verified by me. |
| B4 | **IntrinsicWidth/IntrinsicHeight, OverflowBox, SizedOverflowBox, RotatedBox** widgets + render objects. | DONE | 5 RenderBoxes + widgets, verified vs `.flutter` proxy_box/shifted_box. Framework: box→box intrinsic-during-layout REUSES pre-existing `box_intrinsic_query_borrowed` unsafe (no new unsafe); `BoxConstraints::flipped`. Curated: agent SKIPPED widget integration tests (claimed out-of-scope) — I added 6 (incl. discriminating IntrinsicHeight collapses-maxed-column + step_width rounds 30→40). 17 harness + 6 integration. 1563 rendering+objects+widgets+view green; clippy/fmt/port-check clean. |

## Wave C — scrolling / perf leapfrog

| # | Unit | Status | Notes |
|---|------|--------|-------|
| C1 | **Lazy slivers** — `SliverChildBuilderDelegate` + a `SliverMultiBoxAdaptorElement` (build children on demand during layout). | OPEN — render layer EXISTS, element backend is the gap | SCOPED 2026-06-27: `RenderSliverListLazy` (flui-objects/src/sliver/sliver_list_lazy.rs, 864L) already does virtualization — `Virtualizer` O(log n) range, re-entrant `build_and_layout_box_child` contract, `DeferredMutations` (flui-rendering/src/pipeline/deferred.rs). THE GAP: the build-backend is a v1 stub that "parks the request" (sliver_list_lazy doc step ~20) — needs the ELEMENT-layer to actually build a child view on demand DURING the layout pass (re-entrant build via the deferred queue) from a `SliverChildBuilderDelegate`, then `SliverList.builder` widget. This is the design-heavy element×layout re-entrancy crux — needs a dedicated design pass (NOT a mechanical port). |
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
