# ADR-0002: Engine-wide threading architecture — thread-affine control plane, Send-ready data plane

*Draw the engine's Send boundary below the bindings: make the control plane `!Send`/thread-affine, keep the data plane `Send`-ready, and gate parallel layout behind hard entry criteria instead of betting the rework on it.*

---

- **Status:** Superseded by [ADR-0027](ADR-0027-owner-affine-ui-realms.md) (2026-07-11; it absorbs the Send-boundary table, G1–G4 gates, non-goals, and migration order)
- **Date:** 2026-06-09
- **Deciders:** @vanyastaff *(pending sign-off)*
- **Scope:** workspace-wide — `flui-foundation`, `flui-scheduler`, `flui-rendering`, `flui-painting`/`flui-layer`, `flui-view`, `flui-interaction`, `flui-app`/`flui-platform`
- **Supersedes:** `crates/flui-interaction/docs/ADR-001-gesture-binding-threading-model.md` (crate-local; this ADR absorbs its decision — gestures are now a *consequence* of the engine-wide boundary, not a separate decision)

---

## Verdict

**Target architecture (one paragraph).** flui runs a **single thread-affine control plane** — binding singletons, widget build/reconcile, gesture arbitration, scheduler frame-orchestration, event routing — owned by the UI thread, `!Send` via `PhantomData<*const ()>`, storing plain `Rc`/`RefCell`/owned fields with **no locks**. It runs a **`Send`-ready data plane** — `RenderObject`, `Scene`, `LayerTree`, `NodePtr`, `ImageCache`, the platform `BackgroundExecutor` — whose types stay `Send` exactly as they are today. The two planes are connected by **ownership transfer over bounded, wake-integrated owner lanes**, never `Arc<Mutex<tree>>`: `Scene` moves by value paint→raster, platform owner commands move as private data-only messages, and `PipelineOwner` accepts dirty requests through its channel. The binding decision of this ADR is **the Send-boundary placement**, shippable on its own. **Parallel build/layout/paint is explicitly NOT the lever this ADR commits to** — it is demoted to a documented future track behind four hard gates.

**Is parallel layout the lever or a trap? — Honest answer: it is a narrow, conditional, secondary win that is currently a trap if shipped speculatively.** The Dart-impossible *capability* is real (Rust can transfer subtree ownership to workers; Dart's no-shared-heap cannot). But for *this* engine, today, it wins only on large, wide, text-light, provably-independent trees (long lists / grids / dashboards), regresses typical app frames (Servo measured 221ms→243ms→264ms going 1→2→4 threads on small/medium trees, arXiv 2002.03850; flui's own 1000-node layout is ~205µs — below Rayon break-even), is capped at **<15% of total frame even at perfect scaling** (Meyerovich, WWW2010), and is blocked by two unbuilt prerequisites (a global `FONT_SYSTEM` lock and an absent relayout-boundary oracle). **The unconditional beat-Flutter wins are GC-free and control-plane lock deletion — not parallel layout.** Parallel layout ships only if a committed large-list benchmark proves ≥1.5×.

---

## Context

### Today's reality: single-threaded, wearing a `Send + Sync` costume

The whole frame runs on one thread. `AppBinding::draw_frame` takes `shared_pipeline_owner.write()`, `std::mem::take`s the owner, runs the entire `run_frame()` typestate chain (Layout→Compositing→Paint→Semantics) inline, and writes it back; `run_layout` (`flui-rendering/src/pipeline/owner.rs:1098`) is a serial `for dirty_node in …` loop. Platform input arrives single-threaded from one event-loop pump (bespoke Win32 `GetMessageW` on the dev/CI platform; legacy winit elsewhere). There is **no production `rayon`/`par_iter`/worker-pool** anywhere on the hot path — every `thread::spawn`/`tokio::spawn` is in tests, devtools, dead code, or the file-dialog escape hatch. The de-facto thread model is *identical to Flutter's UI isolate*.

Yet the codebase pays a pervasive `Send + Sync` tax: ~300 `Arc` in gestures, ~25 lock fields in the scheduler, `DashMap`s used single-producer, hand-written `unsafe impl Send/Sync` on `RenderingFlutterBinding` (`crates/flui-app/src/bindings/renderer_binding.rs:126-127`) and `WindowsPlatform` (`crates/flui-platform/src/platforms/windows/platform.rs:105-106`).

### The boundary is in the wrong place — and it traces to one line

The entire control-plane `Send + Sync` edifice is forced by **one supertrait**:

```rust
// crates/flui-foundation/src/binding.rs:106
pub trait BindingBase: Sized + Send + Sync + 'static { … }
```

It is forced not by any exercised cross-thread access but by the singleton storage mechanics: every binding is stored in a process-wide `OnceLock<$binding>` and handed out as `&'static Self` (`binding.rs:140,187-188`). `OnceLock<T>: Sync` requires `T: Send + Sync`, and returning `&'static Self` to arbitrary callers requires `Self: Sync`. **That is the whole reason the supertrait exists** — confirmed by ADR-001's own 2026-06-09 correction (the only cited cross-thread justification, a tokio gesture timer at `flui-interaction/src/timer.rs:350`, is dead code with zero production callers).

So the control plane (six of seven bindings) is forced `Send + Sync` to satisfy a storage decision, while the **one genuinely data-plane binding** — `PaintingBinding`/`ImageCache` — inherits the bound it actually needs for free. This is the exact inversion of a correct control/data split.

### The data plane is already architecturally correct — and unused

`RenderObject<P>: … + Send + Sync` (`crates/flui-rendering/src/traits/render_object.rs:142`) is correctly placed: it is the precondition for moving render objects across workers. `Scene` moves by value (`Send`, `!Sync`) — the actor pattern in miniature. The render tree is a `Slab<RenderNode>` indexed by `RenderId` (no per-node `Arc`/`Rc`/`RefCell`), and the **disjoint-subtree borrow primitive already exists and is Miri-clean**: `get_subtree_mut` materializes N disjoint `&mut RenderNode` from one `*mut Slab` reborrow (`storage/tree.rs:351`), and `SubtreeBorrows`/`NodePtr` drive the recursive walk (`owner.rs:1459-1583`). This machinery was built for single-threaded soundness (the U20.1 Stacked/Tree-Borrows fix, PR #145), but it is exactly the partition substrate a Rayon fan-out would need.

### Why this is decided now

The user wants flui to be **more multithreaded, faster, and safer than Flutter — done architecturally right, engine-wide.** The honest cash-out of that instinct is *not* "parallelize layout." It is: **delete the costume on the control plane** (fewer locks = lower latency + fewer deadlock classes than Flutter's GC-shared model on the latency-critical path), **keep the data plane `Send`-ready** so the one genuine off-thread win available today (image decode) can ship, and **refuse to bet the project** on a sub-15%-ceiling, prerequisite-blocked, ecosystem-unprecedented parallel-layout rewrite.

### Constraints / prior art

- **Edition 2024, Rust 1.96.** `PhantomData<*const ()>` is the canonical zero-cost `!Send`/`!Sync` marker (rust-lang/rust#95985); `rayon::scope` (not `join`) is required for non-`'static` arena borrows; `crossbeam-channel` `bounded(1)` is the SPSC backbone with natural backpressure.
- **Ecosystem convergence:** every production retained-mode Rust GUI is `!Send`-by-default for its app/event context — GPUI (`App: !Send`, `Rc`/`RefCell`, run-to-completion effect queue), Druid/Xilem, Floem, iced, Dioxus. **egui is the lone `Send + Sync` outlier and its issue #1379 is structurally identical to flui's C3 arena re-entrancy bug.** GPUI/Servo split foreground (control, `!Send`) from background (data, `Send + 'static`) at the executor API. **No production Rust GUI parallelizes layout** — only Servo (a browser engine) does, and Servo's 2023 verdict abandoned mandatory fine-grained parallelism as "difficult to observe."
- **Flutter cannot follow:** Dart isolates have independent heaps; the render tree is root-isolate-owned mutable state that cannot cross isolate boundaries without an O(N) deep copy. Flutter is *moving the other way* — merging Platform+UI runners (3.29+) for native-interop simplicity, reducing parallelism.

---

## Decision

We adopt the following, in two **orthogonal, separately-shippable phases**. They are opposite moves on disjoint type sets (control flips to `!Send`; data stays `Send`) and **must not be coupled**.

### The Send boundary across all 7 crates

| Crate / area | Plane | Target | Seam primitive (cite) |
|---|---|---|---|
| `flui-foundation` — `BindingBase`, singleton storage | **Control** | **`!Send`** — drop `Send + Sync` supertrait; replace `OnceLock<&'static Self>` with `thread_local!`/UI-owned storage; mark `PhantomData<*const ()>` | root cause `binding.rs:106`; storage `binding.rs:187-188` |
| `flui-foundation` — callback aliases | Control | **`!Send`** — drop `+ Send + Sync` from `VoidCallback`/`ValueChanged`/etc. | `callbacks.rs:70,92,108,134,151,165,187` |
| `flui-view` — `WidgetsBinding`, `BuildOwner`, `ElementTree`, build/reconcile | **Control** | **`!Send`** — `RwLock<WidgetsBindingInner>` → plain owned fields; element tree stays parent-owned `Box<dyn ElementBase>` (single-owner, UI thread) | `flui-view/src/binding.rs:511-534`; element storage `element/child_storage.rs:32` |
| `flui-interaction` — `GestureBinding`, arena, recognisers | **Control** | **`!Send`** — `DashMap` → `RefCell<FxHashMap>`; `Arc<Mutex<State>>` → `Rc<RefCell>`/plain; per-entry `parking_lot::Mutex` → `RefCell` | binding `binding.rs:149-182`; arena `arena/mod.rs:576-578` |
| `flui-scheduler` — frame orchestrator + tickers | **Control** | **`!Send`** — `Mutex`/`DashMap` state → owned; **but** `create_ticker` vends `Arc<Scheduler>` for cancellation → needs an explicit owned-handle/channel design; **migrate LAST** | scheduler `scheduler.rs:306-380`; ticker vend `scheduler.rs:1537-1541` |
| `flui-rendering` — `PipelineOwner`, layout/compositing/paint walk | **Data (orchestrated from control)** | **Keep `Send`** on `RenderObject`/`RenderTree`/`NodePtr`; the *orchestration* (`run_frame`, dirty-queue) is driven by the `!Send` control thread; `Arc<RwLock<PipelineOwner>>` → owned handle/channel | `RenderObject` `traits/render_object.rs:142`; disjoint primitive `storage/tree.rs:351`, `owner.rs:1459-1583` |
| `flui-painting` / `flui-layer` — `ImageCache`, `Scene`, `LayerTree`, display list | **Data** | **Keep `Send` exactly as-is** — this is the one genuine data-plane binding; `Scene` by-value handoff is the model seam | `ImageCache` `flui-painting/src/binding.rs:356`; `Scene: Send` `flui-layer/src/scene.rs:104` |
| `flui-app`/`flui-platform` — `AppBinding`, `RenderingFlutterBinding`, platforms | **Control (host) + Data (executor)** | **`!Send`** host: delete `unsafe impl Send/Sync` (renderer becomes a plain UI-thread field, drops `Arc<Mutex<Renderer>>`); **Keep `Send`** on `BackgroundExecutor` (`R: Send + 'static` is the correct gate), data-only control senders, raw window handles; owner receivers remain `!Send + !Sync` | delete renderer wrapper unsafety; keep platform owner capabilities private and owner-affine |

**The seam between planes is owned-value transfer over a channel.** Control→data extract uses a typed wrapper (a Bevy `Extract<>` / GPUI `cx.update()` analogue): immutable borrow of the control-plane tree in, owned `Send` job out, over `crossbeam-channel bounded(1)` (natural backpressure). Data→control writeback uses a fallible `Weak`-style callback so results for deleted subtrees silently no-op (the GPUI pattern). `Arc<dyn Fn + Send + Sync>` never crosses this boundary — only owned `Send` data (computed geometry, decoded images, recorded `Scene`).

### Phase 1 — Control-plane `!Send` flip (the binding decision; ship now)

1. Relax `BindingBase` off `Send + Sync` and move singleton storage from `OnceLock<&'static Self>` to `thread_local!`/UI-thread-owned. **Migrate the ~104 `instance()`/`ensure_initialized()` call sites across ~11 files per-binding behind a transitional shim** (keep `instance()` backed by a thread-local during migration); **flip the supertrait LAST** once all impls are thread-local-backed. Order: **Gesture → Widgets → App → Renderer-orchestration → Scheduler (last)**.
2. Mark control-plane roots `PhantomData<*const ()>`.
3. Drop `+ Send + Sync` from the `callbacks.rs` aliases; delete the hand-written `unsafe impl Send/Sync` on `RenderingFlutterBinding` and `WindowsPlatform`.
4. **Keep the data plane (`RenderObject`/`Scene`/`NodePtr`/`ImageCache`/`BackgroundExecutor`) exactly `Send` as-is.** Leave `get_subtree_mut`/`NodePtr` exactly where they are (their value is making the single-threaded walk sound — independent of parallelism).

### Phase 1.5 — Ship the one real parallel win available today (additive, low-risk)

Parallelize **image decode** on the already-spun-up-but-idle `BackgroundExecutor` (`flui-platform/src/executor.rs:67` — a `num_cpus` tokio pool that today serves only file dialogs). `PaintingBinding`/`ImageCache` is already `Send + Sync`; this needs **zero control-plane change** and zero tree rewrite. This is "more multithreaded" where it actually pays.

### Phase 2 — Parallel subtree layout (DEFERRED to its own ADR; four hard gates)

Parallel layout is **not** decided here. It may proceed **only** when all four gates are met, and ships **only** if a committed benchmark proves ≥1.5× on the target workload:

- **G1 — User-owned-fork-point API.** The layout fork point is owned by *user* code: `render_object.perform_layout_raw(erased)` (`owner.rs:1796`) calls the user's body, which invokes `ctx.layout_child(idx, c)` in arbitrary order (Flex flex-factor, baseline, Wrap, intrinsic sizing read child[0].size before deciding child[1]'s constraints). The engine cannot `rayon::scope` children it does not own. Requires a new **opt-in** `ctx.layout_children_independent(&[(id, constraints)])` API the RenderObject author calls only when children are provably independent. This is a control-inversion, not a tuning knob.
- **G2 — Relayout-boundary oracle.** `bootstrap_relayout_boundary` hardcodes `compute_relayout_boundary(true, false, …)` (`box_protocol.rs:122-123`); per-render-object layout-dependency reporting is **deferred to Core.2**. Without it, no subtree can be *proven* independent (flui's analogue of Servo's `impacted_by_floats`). Must land first.
- **G3 — `FONT_SYSTEM` sharding.** Text shaping funnels through one global `OnceLock<Mutex<FontSystem>>` (`flui-painting/src/text_layout/layout.rs:48`). Parallel layout buys ~0 on text-bearing trees until `FontSystem` is sharded per-worker (cosmic-text supports a shared `fontdb` `Arc` + per-thread shaper). **Any parallel-layout benchmark before this measures lock contention, not layout — misleading in both directions.**
- **G4 — Committed benchmark.** The 205µs/1000-node figure is from an **uncommitted, memory-allocation-only** Wave-1 harness. A committed, regression-guarded benchmark (deep tree + wide list + realistic text) is a prerequisite to any go/no-go.

When built: partition the dirty-root set into mutually-non-ancestral subtrees **once on the owner thread** (`get_subtree_mut` takes `&mut self` — two workers cannot each call it), then **`rayon::scope`** over the disjoint `NodePtr` pools — **each worker constructs its OWN `SubtreeBorrows` pool from its own disjoint slice on its own thread** (never share one owner-thread pool). Use a **dedicated frame-local `ThreadPool`**, never the global pool, never shared with image-decode/glyph-raster (WebRender Bugzilla #1595767 priority inversion; gendignoux-2024 ~48% idle `sched_yield` tax). Coarse-grained `par_iter` over independently-dirty top-level subtrees with a node-count cutoff (~200–500); sequential below it (Servo Layout-2020's "same code, opt-in per loop"). Box protocol only (Sliver layout is a no-op stub). **Keep display-list/paint construction sequential** (Servo: "too fast to benefit," cache-friendly) except across `RepaintBoundary` subtrees.

---

## The Flutter-beating lever, assessed honestly

**Where flui genuinely beats Flutter (unconditional, always-on):**

1. **GC-free** (project research #1 structural win) — independent of this ADR, dwarfs parallel layout.
2. **Control-plane lock deletion (Phase 1).** Flutter's UI isolate is GC-shared mutability; flui's `!Send` control plane is lock-free single-owner. This is lower latency on the µs-scale build/gesture/event path **every frame and every input event, regardless of app size** — the GC-free-equivalent structural win for the latency budget. It also deletes ~300 incidental `Arc`s, ~25 scheduler lock fields, the `DashMap` re-entrancy dance, and the `unsafe impl`s, and turns *incidental* `Send + Sync` into *deliberate* `!Send`.
3. **Image decode off-thread (Phase 1.5)** — embarrassingly parallel, `Send`-ready today, zero control-plane change. Flutter does this via background isolates; flui does it with zero-copy `Send` handoff.

**Where "multithreaded" is cargo-cult for flui:**

- **Parallelizing the control plane.** Gesture arena, hit-test, pointer routing, focus, recogniser FSMs are µs-scale causally-ordered logic where determinism and latency beat throughput. Parallelizing adds lock contention and non-determinism for zero gain. Events arrive single-threaded from one OS pump — causal order *is* the correctness contract.
- **Speculative parallel layout on typical UIs.** Quantified vs the **16.67ms (60Hz) budget**: a typical app frame is dozens of layout nodes at ~205µs single-threaded — **~1.2% of budget**, far below Rayon break-even; parallelizing *regresses* it (Servo's measured 221→264ms going 1→4 threads on small/medium trees). The win exists only on the list/grid/dashboard shape (hundreds of visually-independent items), and even there the ceiling is **<15% of total pipeline** (Meyerovich WWW2010) — i.e. **<2.5ms of the 16.67ms budget at perfect scaling.** flui is *structurally a better candidate than HTML* (no CSS floats = no global serialization hazard; box constraints have no lateral sibling dependency; `RepaintBoundary` + relayout boundaries mark partition points), so it degrades less than Servo on typical trees — but that advantage only matters once G1/G2 exist.

**vs GC-free:** GC-free removes whole-frame stalls (unbounded, unpredictable); parallel layout shaves <15% off a 1.2%-of-budget cost on most frames. GC-free is the headline; parallel layout is a conditional differentiator for data-heavy apps only.

---

## Where gestures land (absorbing ADR-001)

Gestures are a **consequence** of the engine-wide boundary, not a separate decision. `GestureBinding`, the arena, and recognisers are **control plane → `!Send`/thread-affine.** This deletes the lock/`Arc` tax (`DashMap`→`RefCell`, `Arc<Mutex>`→`Rc<RefCell>`) on a path that is single-producer to begin with.

Two carve-outs, stated explicitly so the payoff is not oversold:

- **The C3 arena re-entrancy deadlock is already structurally mitigated** — `arena/mod.rs:381,385` disambiguates by `Arc::ptr_eq`, and mutators return `PendingNotifications` dispatched **after** the per-entry lock is released (`arena/mod.rs:390-392`), with a worker-thread regression test. **Phase 1 is therefore justified on lock deletion + deliberate thread-affinity + deleted `unsafe impl`s — NOT on "erasing a live deadlock."** Going `!Send` *does* erase the deadlock *class* structurally (re-entrancy becomes a logic bug, not a hang), which is a real robustness improvement, but the bug itself is not live today.
- **`!Send` does NOT unlock a consuming-typestate gesture redesign.** Winner disambiguation is `Arc::ptr_eq` over aliased members (`arena/mod.rs:381`); `Rc::ptr_eq` behaves identically. The blocker is *aliasing/identity*, not `Send`-ness. A consuming typestate requires a separate arena-by-ID redesign — **orthogonal, a separate ADR.** This ADR delivers lock deletion on the gesture path, not a consuming-gesture model.

This supersedes ADR-001's framing: ADR-001's "Option B (`!Send`) is necessary-but-not-sufficient for compile-time gesture safety" is correct and preserved; the `!Send` flip itself is now decided engine-wide here.

---

## Migration (incremental, value-first, never big-bang)

**Track ordering: ship Phase 1.5 + Phase 1 first; defer Phase 2.** Phase 1.5 (image decode) is additive and can land *before or in parallel with* Phase 1 since it touches only the already-`Send` data plane.

| Step | What flips | Risk | Stays untouched |
|---|---|---|---|
| **0. Image decode off-thread** | wire `ImageCache` fill onto the existing idle `BackgroundExecutor` | low (additive) | everything else |
| **1. Gesture binding `!Send`** | `GestureBinding` + arena + recognisers behind thread-local shim | low | data plane |
| **2. Widgets binding `!Send`** | `WidgetsBinding`/`BuildOwner`/`ElementTree` | medium | data plane |
| **3. App + Renderer-orchestration `!Send`** | `AppBinding`, delete `unsafe impl`s, un-`Arc<Mutex>` renderer | medium | data plane |
| **4. Scheduler `!Send` (LAST)** | bespoke owned-handle/channel ticker cancellation; drop `Ticker` `Send`, relax `TickerProvider: Send+Sync` | medium-high (non-uniform) | data plane |
| **5. Supertrait flip** | remove `Send + Sync` from `BindingBase`; finalize `thread_local!` storage | low (mechanical once all impls shimmed) | data plane |
| **— Phase 2 —** | parallel layout, gated G1–G4, feature-flagged, ship iff ≥1.5× | high; **separate ADR** | control plane |

**What becomes parallel-ready vs actually-parallel:** After Phase 1, the data plane stays *parallel-ready* (`Send` types, disjoint-subtree primitive) but *actually-parallel* only for image decode (Phase 1.5). Layout/paint stay single-threaded until Phase 2's gates are met. **What stays untouched throughout:** the data-plane types (`RenderObject`/`Scene`/`NodePtr`/`LayerTree`/`ImageCache`), the `get_subtree_mut`/`NodePtr` primitive, the `BackgroundExecutor`, raw window handle `Send`, and `flui-painting/tests/thread_safety.rs` assertions on `ImageCache` (which correctly stays `Send`).

---

## The 80/20

**Smallest set that honestly delivers "more multithreaded + faster + safer than Flutter":**

- **More multithreaded:** image decode on the existing idle `BackgroundExecutor` (Phase 1.5) — the one genuine off-thread win, zero tree rewrite.
- **Faster:** control-plane lock deletion (Phase 1) — fewer locks on the µs latency-critical path than Flutter's GC-shared model, every frame.
- **Safer:** deliberate `!Send` + deleted hand-written `unsafe impl Send/Sync` + structurally-erased re-entrancy deadlock class.

This is **high-ROI, always-on, low-risk.** It captures essentially all the real win at a fraction of the cost of the **high-risk parallel-layout bet** (sub-15% ceiling, prerequisite-blocked, ecosystem-unprecedented, regresses typical frames). Distinguish sharply: **fund Phase 1 + 1.5 now; rank Phase 2 BELOW GC-free + frame-pacing + sparse-strip** for funding — its prerequisite G3 (`FONT_SYSTEM` sharding) is independently useful and should be funded on its own merit regardless of whether parallel layout ever ships.

---

## Non-goals (what NOT to parallelize) and open questions

**Explicit non-goals:**

- **Do NOT parallelize the control plane:** gesture arena/recognisers, hit-test dispatch, pointer routing, focus, event ordering, build/reconcile, scheduler orchestration. Causally-ordered µs-scale work; parallelism adds contention + non-determinism for zero gain.
- **Do NOT parallelize the element/build tree.** It is parent-owned nested `Box<dyn ElementBase>` (not the clean slab) and is causally entangled with the control plane via Flutter's build-during-layout interlock. *Only* the `flui-rendering` `RenderId`-slab is a parallel-layout candidate — conflating the two trees is the most likely architectural error.
- **Do NOT parallelize display-list/paint construction** except across `RepaintBoundary` subtrees (Servo: too fast to benefit, cache-friendly).
- **Do NOT share one global Rayon pool** across layout/decode/raster (priority inversion + idle-poll tax).
- **Do NOT remove `check_thread()` (`owner.rs:1560`) without the per-worker-pool redesign** — it currently backstops single-thread soundness; removing it on the parallel path is a NEW concurrent-aliasing soundness obligation requiring fresh Miri + Loom, not a flag flip.
- **Do NOT frame `!Send` as unlocking consuming gestures** (aliasing blocks it, not `Send`-ness).

**Open questions to re-verify before each phase:**

1. **Scheduler ticker cancellation** — does the owned-handle/channel design preserve cancel-through-vended-handle semantics that `Arc<Scheduler>` currently provides (`scheduler.rs:1537`)? Design before migrating.
2. **Two desktop event loops** — the migration must touch both the live Win32 `WindowsPlatform` and the legacy winit stub; reasoning only about winit would miss the live path.
3. **`thread_local!` `is_initialized()` semantics** — per-thread cells change `is_initialized()` from process-global to thread-scoped; audit the ~104 call sites for any that assume process-global init state.
4. **Phase 2 soundness re-audit** — re-run Miri AND Loom on the parallel layout path as a merge gate; the existing Miri-clean proof covers only the single-threaded walk.
5. **`FONT_SYSTEM` sharding (G3)** — verify cosmic-text per-thread `FontSystem` over shared `fontdb` `Arc` before any parallel-layout measurement.

---

## Alternatives Considered

| Option | Why rejected |
|---|---|
| **Status quo (universal `Send + Sync`)** | Pays the lock/atomic/`unsafe-impl` tax for concurrency no platform delivers; the worst of both worlds — neither Flutter's GC simplicity nor Rust's parallelism payoff. |
| **Make parallel layout the headline lever** | Sub-15% pipeline ceiling, negative below ~200–500 nodes, ~0 for text until `FONT_SYSTEM` shards, two unbuilt prerequisites, no production Rust GUI ships it. Regresses typical app UIs. A trap if sold as primary. |
| **One coupled "engine-wide threading rewrite" ADR** | The two halves are orthogonal (opposite moves on disjoint type sets) with opposite risk/payoff. Coupling makes the always-on win hostage to the speculative one and the ADR unshippable. Hence: boundary decided here, parallel layout deferred to its own ADR. |
| **Bevy's `NonSend`-in-`Send`-World pattern** | Bevy is itself migrating away from it (issue #17517). A natively `!Send` control-plane struct that owns thread-affine state directly is cleaner — zero `NonSend` tax, two clean types, zero shared world. |
| **`thread_local!` for data-plane services too** | The data plane *should* stay `Send` — it is the parallelism enabler (image decode now, layout later). Flipping it would foreclose the one genuine win. |

---

## References

- Root cause: `crates/flui-foundation/src/binding.rs:106` (`BindingBase` supertrait), `:187-188` (`OnceLock` storage)
- Data-plane enablers: `crates/flui-rendering/src/traits/render_object.rs:142`; `crates/flui-rendering/src/storage/tree.rs:351`; `crates/flui-rendering/src/pipeline/owner.rs:1459-1583`
- Throughput ceiling: `crates/flui-painting/src/text_layout/layout.rs:48` (FONT_SYSTEM)
- Oracle deferral: `crates/flui-rendering/src/protocol/box_protocol.rs:122-123`
- User-owned fork point: `crates/flui-rendering/src/pipeline/owner.rs:1796`
- C3 mitigation: `crates/flui-interaction/src/arena/mod.rs:381-392`
- Superseded: `crates/flui-interaction/docs/ADR-001-gesture-binding-threading-model.md`; research `docs/research/2026-06-09-adr-001-gesture-binding-threading.md`
- External: Servo parallel layout (arXiv 2002.03850; Layout Engines Report; pcwalton 2014); Meyerovich WWW2010 (~15% pipeline, ~80x microbench ceiling); GPUI ownership (zed.dev/blog/gpui-ownership, zed-decoded-async-rust); Bevy pipelined rendering + issue #17517; WebRender PR #2362/#2998 + Bugzilla #1595767; gendignoux-2024 Rayon profiling; rust-lang/rust#95985 (`PhantomData<*const ()>`)
- Project research: beat-Flutter plan (`docs/research/2026-06-08-beat-flutter-*`) — GC-free #1, sparse-strip defensible, compute-beats-Skia not defensible

---

## Amendments

### 2026-06-09 — `flui-animation` controller: scoped `Send + Sync` exception

The `flui-animation` redesign (`docs/research/2026-06-09-flui-animation-redesign.md`)
keeps `AnimationController` and `Animation<T>` `Send + Sync` (`Arc<Mutex<…>>`) as a
**recorded, scoped exception** to the control-plane `!Send` boundary, rather than
flipping them as part of that work.

**Why deferred, not flipped now:** the controller drives off `flui_scheduler::Ticker`,
whose callback contract is `Send`-bound at the source — `TickerCallback = Box<dyn FnMut(f64) + Send>`
(`crates/flui-scheduler/src/ticker.rs:80`), `Ticker::start<F: FnMut(f64) + Send + 'static>`
(`:355`), `TickerProvider: Send + Sync` (`:96`). A `!Send` `Rc<RefCell>` controller cannot
drive that without flipping the scheduler's threading — which *is* the Phase-1 `!Send`
work decided above. Bundling it into the animation correctness rescue would risk a partial
landing that strands the crate.

**Conditions to retire the exception (do the flip):** when the Phase-1 `!Send` migration
relaxes `Ticker`/`TickerProvider`'s `Send` bound (UI-thread vsync needs no cross-thread
callback), migrate the controller to `Rc<RefCell>`/`Cell`. The redesign keeps this
mechanical by holding all controller mutation behind one lock boundary, using RAII `Drop`
subscriptions (work in both models), and keeping the new `Lerp` data trait
**data-plane-neutral** (no `Send + Sync` bound, so it does not over-constrain `Copy`
geometry primitives).

**Meanwhile:** the cost is one uncontended `parking_lot` lock per `value()`/`status()`
read. Status listeners are fired only after the inner lock is released, so a re-entrant
status callback cannot deadlock.
