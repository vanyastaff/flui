[← Roadmap](../ROADMAP.md) · [Foundations Part II](../FOUNDATIONS.md#part-ii--where-flui-is-better-than-flutter) · [Tracker](../ROADMAP-TRACKER.md)

# Beating Flutter — Competitive Analysis (verified)

> **Source:** two parallel agent fan-outs on 2026-06-08 — (1) a deep-research harness (111 agents, 28 sources fetched, 125 claims extracted, 25 adversarially verified by 3-vote refutation, 20 confirmed / 5 killed); (2) a 6-agent codebase ground-truth investigation. This document records only **verified, citable** competitive findings and ties each to flui's current state and locked contracts. Companion: [`2026-06-08-beat-flutter-plan.md`](2026-06-08-beat-flutter-plan.md) (the execution plan).
>
> **Methodology note:** every finding below survived a 2-of-3 adversarial refutation vote. The five **refuted** claims are recorded explicitly (§5) — they are as important as the confirmed ones, because they prevent the plan from resting on false premises (most importantly: "compute rendering beats Skia" is **false** as a blanket claim).

---

## 1. Thesis

flui's competitive position is **structurally strong but unproven and incomplete**.

- **Structurally strong** — Rust's ownership model gives flui, *for free*, the single largest jank-class that Flutter cannot escape: garbage-collection stop-the-world pauses (§3.1). flui's [FOUNDATIONS Part II](../FOUNDATIONS.md#part-ii--where-flui-is-better-than-flutter) already locks the right architectural bets (no-GC, compile-time arity, first-class memoization), and the external research **independently validates** each of them.
- **Unproven** — every performance advantage is currently *inferential*. flui has **zero** benchmarks on its layout/paint/compositing pipeline and **no** per-phase frame timing. "Faster than Flutter" is, today, an unfalsifiable slogan (§4, decision-sheet Q7).
- **Incomplete** — nothing renders through the real `View → Element → RenderObject → Layer → wgpu` pipeline yet. There are 22 render objects, all proxy/layout boxes; every content-producing leaf (text, image, scroll) is missing, and there is no widget catalog (decision-sheet Q8/Q9).

The plan that follows turns *structurally strong* into *demonstrably strong* and closes the *incomplete* gap on the critical path, while adding the one axis the locked contracts omit: **GPU rendering superiority** (§3.3).

---

## 2. How the four axes net out

| Axis | Where Flutter / incumbents hurt | flui's structural lever | Status in flui today |
|---|---|---|---|
| **Performance & memory** | Dart GC STW pauses blow the 16.67 ms frame budget (§3.1); structural, not a bug | No GC, sync hot path, arena allocation (Part II item 9) | Lever **locked in design**, **0% measured** — no pipeline benches |
| **Developer experience** | Flutter's `canUpdate` memoization is framework-internal, not author-visible; Compose's skip model is a compiler heuristic | First-class typed `View::can_update`/`Memo` (Part II item 4); compile-time arity (item 1) | `can_update` **designed, not built**; rebuild is currently **coarse (whole-subtree)**; **no mutable-state path at all** |
| **Rendering & GPU** | Impeller still drops frames on fast `CustomScrollView` scroll (open P2 #168788, §3.3); Skia compiles shaders in-frame | wgpu backend already real end-to-end; can adopt sparse-strip vector rendering + pipeline precompile | **GPU path is REAL** (not a stub) but uses dense tessellation; **no precompile, no sparse-strip; no GPU-superiority contract exists** |
| **Architecture & correctness** | React-style shared-mutable state adapts poorly to Rust (§3.4); Flutter's null-child crashes at paint | Xilem-style associated-type / enum-dispatch static reconciliation (C4/C6/C9); arity as compile error (item 1) | Reconciler is **real & keyed**; type-erasure boundary **locked**; enum-dispatch element storage **partially built** |

**Net:** flui already made the correct *architectural* bets (research confirms them). The deficit is **execution + measurement + one missing GPU contract** — not strategy.

---

## 3. Confirmed findings (verified, by axis)

### 3.1 Performance — the GC-free advantage is real and structural *(confidence: high, 3-0)*

Dart's garbage collector causes Flutter jank through **stop-the-world** pauses that halt main Dart execution. This is structural to a tracing/copying GC, not a one-off bug: the young-generation collector is a parallel STW semispace scavenger that pauses all mutator threads, and concurrent old-gen marking still requires STW safepoints. A pause as short as **1 ms** on top of a near-budget (16 ms) frame exceeds the 16.67 ms 60 fps deadline and drops a frame.

flui's Rust ownership model has **no managed runtime and no STW** — this entire jank class is *architecturally absent*. This is flui's strongest structural advantage and is already locked as [Part II item 9](../FOUNDATIONS.md#part-ii--where-flui-is-better-than-flutter).

> **Honesty bound (from refutation):** the advantage is **removal of a worst-case jank class**, not a claim that every Flutter frame janks. Flutter's mitigations (concurrent marking, idle-time GC scheduling) mean GC is often invisible. flui's win is *predictability* — the absence of the tail-latency spike — which is precisely what a frame-time **histogram** (not a mean) exposes. This is why §4's perf harness must measure p99/jank-count, not just average fps.

Sources: [flutter_smooth GC analysis](https://cjycode.com/flutter_smooth/design/infra/gc/intro/) · [Dart VM GC](https://mrale.ph/dartvm/gc.html) · [dart-lang/sdk#47337](https://github.com/dart-lang/sdk/issues/47337) · [flutter/flutter#72031](https://github.com/flutter/flutter/issues/72031)

### 3.2 DX — first-class memoization beats Compose's heuristic *(confidence: high, 3-0)*

Jetpack Compose's recomposition-skipping is **stability-driven**: it compares each parameter to its previous value (unstable params by instance identity `===`, stable params by `equals()`), and marks a composable skippable only if all args are stable. With *strong skipping* (default in Kotlin 2.0+), all restartable composables become skippable regardless of parameter stability. Crucially, Compose's stability is a **compiler heuristic plus annotations** — an approximation the runtime computes.

flui can do structurally better: encode skip-eligibility in the **type system**. [Part II item 4 / C1](../FOUNDATIONS.md#c1--reactivity-setstate-canonical-signals-out-memoize-added) already specify a typed `View::can_update(&self, prev: &Self) -> bool` plus a `Memo<V>` combinator — Flutter's internal `canUpdate` short-circuit, made first-class and composable. A `PartialEq`-derived `can_update` gives compile-checked, allocation-free skip decisions.

> **Contract guardrail:** C1 warns that a *blanket* `View: PartialEq` bound is the **Druid trap** ("Druid died of the `Data: Clone + PartialEq` constraint-creep"). App state must carry "no trait bound beyond `'static`." Therefore `can_update` must default to "always rebuild" (Flutter parity) with `PartialEq`-based skipping as an **opt-in** via `Memo<V>` / per-view override — *not* a trait-level bound. This is the single most important design subtlety in the near-term plan.

Sources: [Android strong-skipping docs](https://developer.android.com/develop/ui/compose/performance/stability/strongskipping) · [Compose under the hood (slot table)](https://medium.com/androiddevelopers/under-the-hood-of-jetpack-compose-part-2-of-2-37b2c20c6cdd)

### 3.3 GPU — adopt pipeline precompile + sparse-strip; beat Impeller on scroll *(confidence: high, 3-0)*

Three verified, actionable GPU findings — **none currently covered by a flui contract**:

1. **Pipeline precompile kills shader jank.** Impeller's *first* stated objective is "predictable performance": it compiles all shaders and reflection offline at build time and builds pipeline-state objects upfront, eliminating Skia's in-frame shader-compile jank. flui (wgpu) should **precompile and cache its `RenderPipeline` state objects at startup**. *Caveat:* even Impeller still pays residual runtime pipeline-variant compilation on Android Vulkan (~12 ms first variant, [#113719](https://github.com/flutter/flutter/issues/113719)); flui needs a variant-enumeration strategy, not just "compile at startup."

2. **Impeller has an open scroll-jank cliff — a concrete target.** [flutter/flutter#168788](https://github.com/flutter/flutter/issues/168788) (OPEN, P2, found 3.29–3.32) reports visible jank during fast `CustomScrollView` scrolling on Android, reportedly worse than legacy Skia on the same device; corroborated by [#168442](https://github.com/flutter/flutter/issues/168442), [#143920](https://github.com/flutter/flutter/issues/143920). **Sliver/scroll throughput is a live Impeller weakness flui can beat** — *if* it builds the sliver stack well and measures it.

3. **Sparse-strip vector rendering is the forward bet.** Adopt the design direction of [Vello](https://github.com/linebender/vello/blob/main/doc/vision.md) (a wgpu-based GPU-compute 2D renderer), specifically its **sparse-strip** evolution ([vello#670](https://github.com/linebender/vello/issues/670), Raph Levien; corroborated by the [ETH Zürich 2025 thesis](https://github.com/LaurenzV/master-thesis)): memory usage is decoupled from path bounding boxes (rotation/transform-insensitive), producing a run-length-compressed result that represents antialiased boundary pixels densely and solid interiors sparsely — so rasterization becomes *more* efficient as paths grow larger/sparser (the **opposite** scaling from dense rasterization; Li et al. 2016 measured 2.5×–30× as paths sparsen).

> **Honesty bound (from refutation — see §5):** the claim that *compute-shader rendering structurally beats Skia/Impeller for dynamic masking/blending* was **REFUTED (0-3)**. flui must **not** assume compute rendering is unconditionally faster. The defensible Vello win is the **sparse-strip memory/scaling property**, not a blanket compositing victory. Vello is also itself mid-migration toward hybrid CPU/GPU (Vello Hybrid / Vello CPU) — track Linebender's *current* roadmap, don't treat `vision.md` as final.

### 3.4 Architecture — don't transliterate Dart; flui already chose right *(confidence: high, 3-0)*

Raph Levien (Druid/Xilem/Vello): UI architectures from GC languages "don't adapt well to Rust, mostly because they rely on shared mutable state." React-style state "requires shared mutable access… clunky at best in Rust." The counterexamples confirm the rule: Leptos invented a bespoke reactive-ownership tree + `Copy`/`'static` signals; Dioxus uses `GenerationalBox`/`CopyValue` wrappers — both *invented Rust-specific adaptations* rather than porting naively.

flui's response is already locked and correct:
- **C9 / C4** preserve concrete types from `build()` to the `Slab` node; `dyn` erasure happens at exactly two sanctioned points — the Xilem associated-type / enum-dispatch lesson (zero boxing/downcast in the common path).
- **C1** keeps `setState` canonical and **signals out of the catalog** — matching Xilem's own convergence *away* from signals. The research's signal-based suggestions are therefore **deliberately not adopted**; the sanctioned equivalent is `Memo`/`can_update` (§3.2).

Reference architectures worth continued study (validated as real, distinct designs): **Xilem** (three-tree, associated-type static views), **GPUI/Zed** (hybrid immediate+retained, framework-owned `Entity` via `Rc`), **Makepad** (live-DSL hot reload), **Compose** (slot-table gap-buffer, migrating to LinkBuffer).

Sources: [Levien — Rust UI architecture](https://raphlinus.github.io/rust/gui/2022/05/07/ui-architecture.html) · [GPUI README](https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md) · [Makepad](https://github.com/makepad/makepad)

### 3.5 DX — Rust hot-reload is now competitive (with a Windows asterisk) *(confidence: high, 3-0)*

Rust's historically weak hot-reload story is now good enough to neutralize a major Flutter DX edge: **Dioxus Subsecond** delivers sub-second (often sub-200 ms) incremental Rust rebuilds via binary patching — measured ~900 ms default / 500–600 ms subsecond-dev profile on M1, ~300 ms with Cranelift on M3 Pro ([Dioxus PR#3797](https://github.com/DioxusLabs/dioxus/pull/3797)). **Makepad** offers no-recompile iteration for DSL/styling/layout/shader changes via its live-design loop.

> **Honesty bound:** Subsecond is experimental in 0.7, patches only the tip crate, and has **documented Windows-linker caveats** — directly relevant since flui's primary dev OS is Windows. flui already ships `flui-hot-reload` (dlopen scene plugins). Whether sub-second Rust hot-reload is achievable on Windows is an **open question** (§6), not a settled win.

---

## 4. The measurement gap (the highest-leverage finding)

flui can currently measure **nothing** about production frame/layout/paint/startup/memory performance:

- **0 benches** in `flui-rendering`, `flui-painting`, `flui-layer`, `flui-engine`, `flui-app`, `flui-scheduler`. The only 8 benches cover geometry/color micro-ops (`flui-types`) and reconcile *complexity over mocks* (`flui-view`).
- `run_layout` / `run_paint` / `run_compositing` (`flui-rendering/src/pipeline/owner.rs:1098/2065/1895`) have **no `#[instrument]` spans, no per-phase timing**.
- `FrameTiming` (`flui-scheduler/src/frame.rs:541`) stores one wall-clock elapsed — **no per-phase split**.
- A complete per-phase profiler exists but is in **disabled** `flui-devtools`.

The good news: `criterion 0.7`, `tracing-tracy 0.11`, `puffin_http 0.16` are already pinned, and `cargo bench -p flui-view --no-run` compiles green (59.6 s). **The harness is one focused unit away.** Until it exists, §3.1's GC-free advantage is a claim, not a number — so the perf harness is the *enabling* work for the entire competitive thesis.

---

## 5. Refuted claims (the guardrails)

These five claims were **killed** by adversarial verification. The plan must not rest on them:

| Claim | Vote | Why it matters |
|---|---|---|
| "Compute-shader 2D rendering structurally beats Skia for dynamic masking/blending" | **0-3** | The plan must **not** justify a Vello/compute rewrite on "compute is faster." Only the sparse-strip memory/scaling property is defensible (§3.3). |
| "Dart GC produced 11 ms pauses mid-layout-phase" (specific framing) | **0-3** | The *specific* anecdote doesn't hold; the *structural* STW argument (§3.1) does. Don't cite the 11 ms number. |
| "Idle/between-frame GC scheduling can't ever be acceptable" | **0-3** | Overstated — Flutter's GC is often invisible. flui wins on *worst-case predictability*, not "Flutter is always janky." |
| "Vello's backdrop / tile_alloc stage is a bottleneck sparse-strip eliminates entirely" | **1-2** | Don't claim a specific Vello-internal win; flui isn't Vello. |
| "Compose recomposition = positional memoization + `remember` equality" | **1-2** | Use the verified strong-skipping model (§3.2), not this framing. |

---

## 6. Open questions (carried into the plan)

1. **Sparse-strip on wgpu/Windows-DX12:** does the memory/scaling advantage hold on flui's target GPUs, and what is the fragment-shader cost model for *typical UI* (text-heavy, many small paths) vs the large/sparse paths where sparse-strip wins? (A small spike, not a commitment.)
2. **Compile-time stability in Rust's type system:** can `can_update` skip-eligibility be encoded as a trait-bounded marker *without* the Druid `PartialEq`-constraint-creep trap (C1)? How does it interact with the C9 static-reconciliation path?
3. **Rust hot-reload ceiling on Windows:** given Subsecond's Windows-linker caveats, can flui hit sub-second iteration on its primary dev OS, or does Flutter retain a real DX edge there?
4. **flui-reactivity's fate:** C1 locks signals out of the *catalog* but permits a post-parity application-author opt-in. Keep the 8k-LOC signal runtime parked as a divergence candidate (it compiles; deletion discards a real asset), or commit to deleting it to remove decision debt?

---

## 7. What this changes in the locked plan

The research **validates** FOUNDATIONS Part II items 1, 4, 9 — no architectural fork is warranted. It surfaces exactly **one strategic addition**: Part II has **no GPU-rendering-superiority item**. The research justifies adding one (pipeline precompile + sparse-strip direction + beat-Impeller-on-scroll), which is an amendment to the locked Part II and requires sign-off per [governance](../FOUNDATIONS.md#governance). Everything else is execution + measurement of already-locked bets. See the [plan](2026-06-08-beat-flutter-plan.md).
