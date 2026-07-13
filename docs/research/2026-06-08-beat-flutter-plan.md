[← Competitive analysis](2026-06-08-beat-flutter-competitive-analysis.md) · [Roadmap](../ROADMAP.md) · [Foundations Part II](../FOUNDATIONS.md#part-ii--where-flui-is-better-than-flutter) · [Tracker](../ROADMAP-TRACKER.md)

# Beating Flutter — Execution Plan

> **Premise (from the [competitive analysis](2026-06-08-beat-flutter-competitive-analysis.md) + codebase ground-truth):** flui's "better than Flutter" *strategy* is already locked and research-validated (FOUNDATIONS Part II items 1/4/9). The deficit is **execution, measurement, and one missing GPU contract** — not architecture. This plan does not fork the roadmap; it **re-prioritizes the near term** to (a) make the locked advantages real and *measurable*, (b) prove the full pipeline end-to-end, and (c) add the one strategic amendment the research justifies (GPU rendering superiority).

> **Scope discipline:** "beat Flutter" is the destination of the *entire* port, not one PR. This plan covers the **near-term critical path** (the next ~6 PRs / build waves) that converts flui from "structurally strong but unproven" to "demonstrably strong on a runnable demo," plus the strategic decisions that gate the longer GPU track. It is sequenced by dependency, per [ROADMAP](../ROADMAP.md) convention (no calendar dates).

---

## 1. Current state (settled facts)

From the 2026-06-08 ground-truth investigation (supersedes the stale gap-matrix):

| Fact | Value | Evidence |
|---|---|---|
| Render objects implemented | **22** (18 `RenderBox` + 4 `RenderSliver`), all proxy/layout | `grep -c 'impl RenderBox\|impl RenderSliver'` |
| Content-producing leaves | **0** — no `RenderParagraph`/`RenderImage`/`RenderViewport`/`RenderSliverList` | absent from `flui-rendering/src/objects/` |
| Widget catalog | **0 widgets**, no `flui-widgets`/`flui-material`/`flui-cupertino` crate | `grep` for widget structs → EXIT 1 |
| Mutable state path | **none in production** — `set_state` test-only; `GlobalKey::with_current_state` read-only | `dispatch`/`unified.rs:456`; `global_key.rs:137` |
| Rebuild granularity | **coarse** — whole-subtree, no `can_update` short-circuit | `dispatch.rs:103-111`, `generic.rs:386` |
| Inherited data (Theme) | **inert** — production build uses empty dummy context | `behavior.rs:242,365,447-451` |
| Pipeline perf measurement | **none** — 0 pipeline benches, no per-phase spans | `cargo metadata`; `owner.rs` grep |
| wgpu GPU path | **REAL end-to-end** (not a stub) — surface→encoder→draw→present | `flui-engine/.../renderer.rs:576`, 21 WGSL shaders |
| `flui-animation` | **disabled but compiles clean**; Ticker wiring already done | `controller.rs:8,92,194`; commit `73f0e714` |
| `flui-reactivity` | **disabled**, 8k-LOC signals, zero flui-deps, no tree bridge | `Cargo.toml:50-52` |

**Reading:** the render *machine* is real (layout/paint/compositing all wired post-D-block); the *content layer* and *measurement* are near-zero. Nothing renders through `View→Element→RenderObject→Layer→wgpu` today — the only visual example (`desktop_scene`) draws rects straight to a canvas, bypassing the framework.

---

## 2. The winning strategy, per axis

| Axis | The move | Maps to | Status |
|---|---|---|---|
| **Performance** | Make the no-GC advantage **measurable** via a pipeline perf harness (criterion benches + per-phase spans + frame-time histogram) | Part II item 9; ROADMAP:182 assertion | **Wave 1** |
| **DX** | Build the **mutable-state path** (so a button works) + first-class **`Memo`/`can_update`** memoization (beats Compose's heuristic) | C1, Part II item 4 | **Wave 2 + 3** |
| **GPU** | **NEW contract:** pipeline precompile + sparse-strip direction + beat-Impeller-on-scroll | *amendment to Part II* | **Wave 6 (gated on §5 decision)** |
| **Architecture** | Prove the locked C4/C6/C9 reconciliation by rendering a real widget tree end-to-end | C4/C6/C9; ROADMAP Core.1 | **Wave 4 + 5** |

---

## 3. Build waves (dependency-ordered)

Each wave is one PR-sized unit. Effort: S/M/L/XL. Axis tags from §2. All file:line refs are from the ground-truth investigation.

### Wave 1 — Perf harness *(perf · M)* — **the enabling work; build first**
*Why first:* until this exists, every "faster than Flutter" claim is unfalsifiable (analysis §4). It is purely **additive** (new bench files + spans), zero contract risk, and it is what converts the GC-free thesis from inferential to measured.
- **U1a** `crates/flui-rendering/benches/layout.rs` (criterion, `harness=false`) — build representative trees (flat 1K leaves; deep 1K chain; wide Flex/Stack N=10/100/1000), bench `owner.run_layout()` / `layout_dirty_root()`. Reuse fixtures from `tests/run_layout_wiring.rs`.
- **U1b** `crates/flui-rendering/benches/paint.rs` — bench `run_paint()` + `run_compositing()` over the same trees.
- **U1c** per-phase `#[tracing::instrument(name="layout"/"paint"/"compositing")]` spans on `owner.rs:1098/2065/1895`.
- **U1d** extend `flui-scheduler` `FrameTiming` (`frame.rs:541`) with `phase_durations: [Milliseconds; 4]`, populated by the scheduler around each pipeline call → the frame-time-histogram oracle for ROADMAP:182, **without** re-enabling `flui-devtools`.
- **U1e** wire `cargo bench --no-run` into CI as a compile-gate (benches currently aren't built in CI → bitrot risk).
- *Exit:* `cargo bench -p flui-rendering` produces layout/paint throughput numbers; a 5-second run yields a p50/p99/jank-count frame-time histogram. **First defensible perf number vs Flutter.**

### Wave 2 — Mutable-state path *(DX/arch · S)* — **unblocks all interactivity**
*Why:* the framework literally cannot handle a button press today (decision-sheet Q3). Hard gate on every interactive demo. `setState` is the C1 canonical model — this is sanctioned, not a divergence.
- **U2a** wire a production `callback → state-mutation → schedule_build_for` path. `Element::set_state` (`unified.rs:456`) exists but has only `#[cfg(test)]` callers; expose a sanctioned mutable handle for event callbacks (the gesture/`onPressed` path).
- **U2b** decide the external-handle story: `GlobalKey::with_current_state` is read-only (`FnOnce(&T)->R`); add the mutable counterpart or route mutation through the element's `create_mark_dirty_callback` (the proven `AnimatedBehavior` bridge at `behavior.rs:933-943`).
- *Exit:* a `StatefulView` button callback mutates state and schedules exactly one rebuild; covered by a non-test integration test.

### Wave 3 — First-class memoization (`Memo`/`can_update`) *(DX/perf · S–M)* — **beats Compose's heuristic**
*Why:* one `setState` currently rebuilds the whole subtree (`dispatch.rs:103-111`). This is the cheapest perf win in the codebase and is a *locked target* (Part II item 4 / C1 "memoize added").
- **⚠ Contract guardrail (critical):** do **NOT** add a blanket `View: PartialEq` bound — that is the **Druid trap** C1 explicitly forbids ("app state carries no trait bound beyond `'static`"). Instead: `View::can_update(&self, prev: &Self) -> bool` **defaults to `false`** (always rebuild = Flutter parity), with `PartialEq`-based skipping as an **opt-in** via a `Memo<V>` combinator and/or per-view override.
- **U3a** add `can_update` to the `View` trait surface with the safe default; gate `dispatch.rs` `mark_dirty_for_dispatch` on `!prev.can_update(next)` instead of unconditional dirty.
- **U3b** add the `Memo<V>` combinator (the opt-in `PartialEq` fast-path) per Xilem's `memoize`.
- *Exit:* an unchanged subtree under a `Memo` is **not** rebuilt (assert via `ReconcileEvent` trace); the `View` trait gains no new bound on app state. **Type-system memoization — structurally better than Compose's compiler heuristic.**

### Wave 4 — Hello-world vertical slice *(arch · L)* — **proves the pipeline + makes the perf edge demonstrable**
*Why:* this is the single move that proves `View→Element→RenderObject→Layer→wgpu` works end-to-end and turns the AOT/wgpu/no-GC startup edge from aspirational into a *runnable demo* you can benchmark against a Flutter hello-world.
- **U4a** `RenderParagraph` (`flui-rendering/src/objects/paragraph.rs`) — the **only hard new render object**; wrap the existing `flui-painting/text_layout` + `text_painter` infra (do not rebuild). *Effort: XL within the wave.* Aligns with ROADMAP Core.1 / map-doc "top priority."
- **U4b** stand up `flui-widgets` skeleton crate at layer L6 (depends on `flui-view`+`flui-rendering`+`flui-painting`+`flui-interaction`) — ROADMAP:164 already plans this.
- **U4c** thin widget wrappers over existing render objects: `Text`, `Center`, `ColoredBox`/`Container`(color-only), `Padding`, `SizedBox` (the latter four wrap already-existing render objects).
- **U4d** a runnable widget example replacing the canvas-only `desktop_scene`, exercising the full pipeline; capture its startup time + first-frame histogram (feeds Wave 1's numbers).
- *Exit:* `cargo run` shows centered text rendered through the real pipeline on Windows; startup + frame numbers captured.

### Wave 5 — Counter demo *(arch/DX · M)* — **proves interactivity end-to-end**
*Why:* exercises Wave 2's mutable-state path + gesture routing through the live render tree — the highest-risk unproven path (decision-sheet gap #5: recognizers exist but no example proves hit-test reaches the render tree).
- **U5a** verify/finish pointer routing into the live render tree (`RenderPointerListener`/`RenderMouseRegion`); wire `flui-interaction`'s `TapGestureRecognizer` (`recognizers/tap.rs`) to a hit-test on the live tree.
- **U5b** `GestureDetector` (ProxyView) widget; ship the button decoration-free (ColoredBox+Padding+Text) to keep `RenderDecoratedBox` out of the critical path initially.
- **U5c** `Column` widget (wraps existing `RenderFlex`).
- *Exit:* a tappable counter increments via `setState` and rebuilds only the changed subtree (Wave 3 verified live).

### Wave 6 — GPU superiority track *(GPU · L–XL)* — **GATED on §5 decision**
*Why:* the one research-justified addition to Part II (analysis §3.3). Only proceed after the §5 amendment is signed off.
- **U6a** *(precompile)* enumerate + precompile/cache `wgpu::RenderPipeline` state objects at startup; measure shader-jank elimination via Wave 1's histogram.
- **U6b** *(scroll target)* build the sliver scroll stack well (`RenderViewport` + `RenderSliverToBoxAdapter` + `RenderSliverList`) and bench it against Flutter's open scroll-jank cliff (#168788) — *win on scroll where Impeller is weak.*
- **U6c** *(spike, not commitment)* sparse-strip feasibility spike on wgpu/DX12 for typical text-heavy UI (open question §6.1) before any renderer rewrite.

### Parallel side-quest — `flui-animation` re-enable *(DX · S)* — **independent of the waves**
*Why:* cheap, sanctioned (ROADMAP Core.1), wiring already done (`AnimationController` drives `flui_scheduler::Ticker`; `Animation<T>` extends `Listenable`). Enables ROADMAP:182's one falsifiable 60 fps assertion.
- **U7a** uncomment in root `Cargo.toml` `workspace.members` + `default-members`.
- **U7b** `flui-view` `AnimatedView` subscribes to the controller's `Listenable` → `mark_needs_build` (the consumer-side work; reuses the proven bridge).
- **U7c** *(pre-existing bug, fix per work-discipline)* the `flui-geometry` serde-derive bug on `Radius<Pixels>`/`RRect`/`Corner` (`rrect.rs:136`, `corner.rs`) blocks `--all-features`; fix it before enabling animation's `serde` feature. *Independent of animation but in the blast radius.*

---

## 4. Sequencing

```
Wave 1 (perf harness) ──────────────┐  enabling: measures everything after
                                     ▼
Wave 2 (mutable state) ──► Wave 3 (Memo) ──► Wave 5 (counter)
                                     │
Wave 4 (hello-world) ────────────────┴──► (demo benchmarked vs Flutter)
                                     │
                          §5 decision ▼
                                  Wave 6 (GPU track)

Side-quest (animation re-enable) ── parallel, anytime
```

**Rationale:** Wave 1 first (measures all later work). Wave 2 unblocks interactivity; Wave 3 (memoize) depends on the reconcile path Wave 2 touches → sequential. Wave 4 (hello-world) is parallel to 2/3 (different files: render object + new crate vs element-core). Wave 5 needs 2+3. Wave 6 is gated. Animation is fully independent.

---

## 5. The one strategic decision (needs sign-off)

**Amend FOUNDATIONS Part II to add a GPU-rendering-superiority item?** Per [governance](../FOUNDATIONS.md#governance), a Part II amendment needs documented rationale + `STRATEGY.md`/`PORT.md` sync. The research justifies it (analysis §3.3): pipeline precompile, beat-Impeller-on-scroll, sparse-strip direction. Without sign-off, **Wave 6 does not start** and flui's GPU story stays "as good as wgpu allows" rather than "deliberately better than Impeller."

- **Option A — commit the GPU contract now** (adopt precompile + scroll-target as contracts; sparse-strip as a tracked spike). Highest ceiling; commits to an XL track.
- **Option B — defer; finish the content layer first** (Waves 1–5), revisit GPU after a runnable demo exists. Lower risk; keeps GPU "competitive" not "superior" for now.
- *Recommendation:* **Option B** — Waves 1–5 are higher-leverage and contract-safe; do not open the XL GPU track until the pipeline is proven and measured. Revisit Wave 6 with real numbers in hand.

Two smaller open questions (analysis §6) — `flui-reactivity` keep-vs-delete, and the sparse-strip Windows/DX12 spike — are *not* blockers and can ride along.

---

## 6. What ships this session (Build Wave 1, started now)

Per the deliverable ("report + plan + parallel build"), the **contract-safe, highest-leverage** units begin immediately while §5 awaits the user's call:
1. **Wave 1 (perf harness)** — additive, zero contract risk, enables the entire thesis. Built + verified first.
2. **Side-quest (`flui-animation` re-enable + the `flui-geometry` serde fix)** — independent, sanctioned, cheap.

Waves 2–3 (mutable state + `Memo`) touch the C1/C4/C5 element core and carry the Druid-trap guardrail — they are built next, carefully, *not* blind-fanned-out. Waves 4–6 are the larger sequenced follow-ups.
