# ADR-0003: Generic virtualization core (`flui-virtualization`) + re-entrant build foundation

*Extract windowing math into a standalone, protocol-agnostic crate that any windowed UI can reuse, and design the on-demand child-build contract to be mid-pass-capable from day one — so the market-best true-mid-pass build is never locked out, even if v1 ships a next-frame backend.*

---

- **Status:** Accepted
- **Date:** 2026-06-12
- **Deciders:** @vanyastaff
- **Scope:** new crate `flui-virtualization`; `flui-rendering` (slivers, `LayoutContextApi`, lazy `SliverList`); `flui-view` (lazy widgets, future)
- **Relates to:** ADR-0002 (engine-wide threading) — inherits the U20/U20.1 disjoint-subtree borrow discipline and the deferred-mutation queue as the borrow-safe substrate for re-entrant build

---

## Verdict

**Target architecture (one paragraph).** flui gains a new **standalone, protocol-agnostic crate `flui-virtualization`** that owns *windowing math only*: given a `ScrollWindow { offset, main_extent, cache_before, cache_after }` and a set of per-item extents (measured or estimated), it answers "which item indices are visible (plus cache buffer)?" in `O(log n)`, and corrects the scroll anchor when a measured extent differs from its estimate. It depends only on the three foundation crates (`flui-types`, `flui-foundation`, `flui-geometry`) and on **no** render, sliver, or protocol type. The correct-but-uncalled `FenwickExtents` (currently `crates/flui-rendering/src/slivers/fenwick.rs`) **moves into this crate** as its prefix-sum backbone. `flui-rendering` becomes a *consumer*: it adapts `SliverConstraints → ScrollWindow`, drives the `Virtualizer` for the visible range, and builds/lays-out only the visible-plus-cache children. The on-demand child-build mechanism is exposed as a **new, mid-pass-capable `LayoutContextApi` capability** whose contract permits true mid-pass re-entrant build (Compose `SubcomposeLayout` style) without a later breaking change — even though the **v1 backend may materialize children via the existing next-frame deferred-mutation queue plus cache-region prefetch**. Recycling in v1 is **dispose-on-scroll-off behind a pluggable hook**, not RecyclerView's two-level pool.

**Why this is decided now, and recorded durably.** flui is a Flutter→Rust framework built to *beat* Flutter, and breaking changes are allowed **now** while the contracts are not yet locked. The explicit decision driver is to avoid Flutter's ossification trap: contracts locked early cannot later adopt the market's better solutions. The two load-bearing commitments here — **(1)** the virtualization core is *agnostic* (protocol-free and build-free), and **(2)** the build contract is *mid-pass-capable from day one* — are precisely the commitments that keep the future-proof, market-best abstraction reachable. They are recorded here so a future session with zero memory cannot accidentally re-couple the core to the sliver protocol, or quietly settle for a next-frame-only build contract that locks out true mid-pass. This is **first-class, not a vague "later."**

---

## Context

### What virtualization is, and why flui needs a real one

A virtualized (a.k.a. windowed or lazy) list renders only the items intersecting the viewport plus a small cache buffer, instead of all `n` items. Doing this well requires three pieces of windowing math: (a) an `O(log n)` map between scroll offset and item index over a running prefix-sum of item extents; (b) an *estimate* for items not yet measured, so the total scroll extent and the scrollbar are stable before every item has been laid out; and (c) an *anchor correction* that keeps the on-screen content from jumping when a measured extent turns out to differ from its estimate. These three concerns are a self-contained abstraction layer with nothing render-specific about them — they are equally the math behind a virtualized list, grid, data table, timeline, or text view.

### Current state of the code (verified via scout, 2026-06-12)

- **`FenwickExtents`** (`crates/flui-rendering/src/slivers/fenwick.rs`) is **correct, ASM-verified, and fully self-contained** — it depends only on `Vec<f32>` and has *zero* coupling to any render or protocol type. It is the prefix-sum backbone a virtualizer needs. It has **zero callers** today. (It lives in the right *shape* but the wrong *crate*: nothing in the render layer is forced to host it.)
- **`SliverConstraints` / `SliverGeometry`** are tightly bound to `SliverProtocol` and to the viewport walk. They are *not* a neutral windowing value type; building the virtualization core on top of them would couple it to the render layer and the sliver protocol.
- **The deferred-mutation queue** (`crates/flui-rendering/src/pipeline/deferred.rs` + `PipelineOwner::apply_deferred_mutation`, `crates/flui-rendering/src/pipeline/owner.rs:2021`, drained at the *end* of `run_layout`) is fully wired, but it is **next-frame materialization**: an `Insert` yields its `RenderId` at apply-time, *after* layout has finished — it is **not** mid-pass re-entrancy. It is, however, a sound, borrow-safe insertion path that can serve as a v1 build backend (see Decision 2).
- **Existing sliver lists** (`crates/flui-rendering/src/objects/sliver_fixed_extent_list.rs`, `crates/flui-rendering/src/objects/sliver_fill_viewport.rs`) lay out **all** children eagerly, `O(n)`.
- **The viewport** (`crates/flui-rendering/src/objects/viewport.rs`, `layout_child_sequence` at `:325`) drives children eagerly with a per-child constraint cache (`try_cached_sliver_geometry` at `:612`) but has **no "skip this child" hook** — i.e., no way for a sliver to lay out only a sub-range of its children.

So the substrate is *half-present*: the hard, correct math (`FenwickExtents`) exists but is unused and mis-located; the sliver/viewport machinery is eager and protocol-coupled; and the insertion path that exists is next-frame, not mid-pass.

### Competitive landscape (what to beat, what to borrow, what to ignore)

- **TanStack Virtual** — measured-extent + estimate virtualization. Confirms the *estimate-then-correct* model is the right one for variable-height items. **Borrow:** the estimate-for-unmeasured + correct-on-measure shape.
- **GPUI `SumTree`** — a B+-tree carrying summary dimensions (e.g. `Count`, `Height`) that yields `O(log n)` pixel↔index seeks. Confirms the `O(log n)` offset↔index requirement and that a summed-dimension structure is the right tool. `FenwickExtents` is flui's (simpler, ASM-verified) equivalent of the height-summary seek.
- **Jetpack Compose `LazyColumn` + `SubcomposeLayout`** — the reference implementation of **true mid-pass build**: a layout can compose (build) children *during its own measure pass*, deciding what to build based on available size. **This is the market-best capability we refuse to lock out.** Compose's own docs warn `SubcomposeLayout` is costly (it defers composition and can cost a frame), which is *why* flui keeps a cheaper next-frame backend available as a stepping stone — but the *contract* matches Compose's capability.
- **Android `RecyclerView`** — `GapWorker` idle-time prefetch + a two-level view-recycling pool. Its *justifying constraint* is that creating an Android `View` is expensive and the platform is GC'd, so reuse pays for itself. **That constraint does not hold for a Rust arena with `can_update`** (cheap in-place reconcile, no GC). Borrow the *idle/cache-region prefetch* idea (latency hidden by the cache buffer); **reject the two-level pool** as cargo-cult for this runtime.
- **Flutter `RenderSliverList`** — the thing to beat. Its weaknesses: linked-list child tracking with **estimate-based scroll-extent jitter**; `cacheExtent` computed **synchronously in the scroll frame**; and **no precise anchor correction on resize**. flui's core fixes exactly these: a Fenwick/`SumTree`-class structure instead of a linked list, an explicit `AnchorCorrection` output, and a cache-region model that does not have to be recomputed inline every scroll frame.

---

## Decision

Three decisions, plus the layering that ties them together. They are deliberately separable: Decision 1 (crate boundary) can land and be tested with no consumer; Decision 2 (build contract) is the breaking `LayoutContextApi` addition; Decision 3 (recycling) is a pluggable v1 default.

### Decision 1 — Crate boundary: a deep, protocol-agnostic `flui-virtualization`

Create a **new standalone crate `flui-virtualization`** that is **protocol-agnostic**. Its public surface is generic over a plain value type and `f32` extents — it never names a render, sliver, or protocol type:

```rust
// Illustrative public shape (authoritative surface is fixed by the API-GATE in the plan).
pub struct ScrollWindow {
    pub offset: f32,        // scroll offset of the leading visible edge, in main-axis pixels
    pub main_extent: f32,   // size of the viewport along the main axis
    pub cache_before: f32,  // extra main-axis pixels to keep built ahead of the leading edge
    pub cache_after: f32,   // extra main-axis pixels to keep built past the trailing edge
}

pub struct VisibleRange {
    pub first: usize,       // first item index in [visible + cache] band
    pub last: usize,        // last item index (inclusive) in [visible + cache] band
    // leading edge offset of `first`, for the consumer to place children
    pub leading_edge: f32,
}

pub struct AnchorCorrection {
    pub delta: f32,         // scroll-offset adjustment to apply so on-screen content does not jump
}
```

- **Dependencies:** `flui-types`, `flui-foundation`, `flui-geometry` **only**. **Not** `flui-rendering`, and **not** any sliver or protocol type.
- **`FenwickExtents` moves here** from `flui-rendering` and becomes the crate's prefix-sum backbone (the move is breaking but has zero callers — see Consequences).
- **`flui-rendering` adapts**, not the other way around: a thin `SliverConstraints → ScrollWindow` adapter lives in `flui-rendering`, so the dependency arrow points `flui-rendering → flui-virtualization`.

**Rationale (A Philosophy of Software Design, Ousterhout).** This is a *deep module*: a small interface (window query, extent update, anchor correction) hiding substantial complexity (Fenwick prefix sums, the estimate model for unmeasured items, anchor-correction arithmetic). It enforces **information hiding** — callers never touch the prefix-sum representation. And **windowing math is a different abstraction layer than the sliver protocol**: keeping them in separate crates keeps each interface narrow and each layer independently testable.

**Rationale (dependency acyclicity).** `SliverConstraints` lives *in* `flui-rendering`. A virtualization core that named it would have to depend on `flui-rendering`, which already (transitively) wants to depend on the virtualization core — a cycle. Making the core agnostic and pushing the adapter down into `flui-rendering` breaks the cycle by construction.

**Rationale (reuse).** A protocol-agnostic core is reusable by: the render layer (sliver lists, now); `flui-view` lazy widgets (future); and general windowed UIs — virtualized text, data grids, tables, timelines. A sliver-bound core could serve only slivers.

### Decision 2 — Re-entrant build: a mid-pass-capable contract from day one

The `Virtualizer` core is **build-agnostic**: it answers *visible-range* and *anchor* only. It knows nothing about *how* or *when* children are built. Build is a separate concern, expressed as a **contract**, not baked into the core.

The contract is a **new `LayoutContextApi` capability** by which a lazy sliver, *during its own layout*, requests on-demand materialization of a specific child (by index/key). **This contract is designed to be mid-pass-capable from day one** — its signature and semantics must permit a true mid-pass re-entrant build (build a child *now*, mid-measure, and get its laid-out geometry back before deciding the next child), in the style of Compose `SubcomposeLayout`, **without a later breaking change to the contract or to the core.**

This is recorded as **first-class and explicit, not a vague "later":**

- The **agnostic core** + a **mid-pass-capable contract** *together* guarantee true mid-pass build is **never locked out.** Either piece alone would not: a protocol-bound core or a next-frame-only contract would each foreclose it.
- **v1 backend may be next-frame as a stepping stone.** The v1 *implementation* behind the contract may use the existing **next-frame deferred-mutation queue** (`pipeline/deferred.rs` + `apply_deferred_mutation`) **plus cache-region prefetch** (RecyclerView/Flutter-style: build the cache band ahead of need so the one-frame insertion latency is hidden by the buffer). This is a legitimate stepping stone — *but the contract stays mid-pass-shaped*, and **true mid-pass is a planned, recorded implementation unit (see the plan's U2 and U4), not an abandoned aspiration.**
- **Borrow-safe by construction.** The mechanism inherits the U20/U20.1 disjoint-subtree borrow discipline and the deferred-queue's mid-pass-marks handling (the side queue drained per layout iteration). On-demand build must not create an aliasing hazard: a child materialized mid-pass is reached through the same disjoint-borrow primitive the recursive layout walk already uses.

**Contrast (why flui's must be different):**

- **Flutter `invokeLayoutCallback`** performs re-entrant build during layout, but it is **unsafe re-entrancy** — guarded only by a debug flag, with no compile-time aliasing guarantee.
- **Compose `SubcomposeLayout`** does true mid-pass build with a **slot-id model** and **deactivate-not-dispose** for off-screen slots — sound, but in a GC'd runtime.
- **flui's contract must be borrow-safe by construction** — the Rust arena + disjoint-borrow primitive is what lets flui offer Compose's capability *without* Flutter's unsafety and *without* a GC. That is the differentiator, and it is the reason the contract shape is load-bearing enough to be an ADR decision rather than an implementation detail.

### Decision 3 — Recycling: dispose-on-scroll-off behind a pluggable hook (v1)

v1 recycling is **dispose-on-scroll-off**: when a child leaves the visible-plus-cache band, it is disposed (simple, Flutter-like). This sits **behind a pluggable hook** so a different policy can be swapped in later without touching the consumer's core logic.

flui explicitly does **not** adopt **RecyclerView's two-level recycling pool** in v1. Its justifying constraint — expensive `View` creation + GC pressure — **does not hold** for a Rust arena with cheap `can_update` reconcile. Recycling (pooling/reuse) may be added later **only if a real-frame benchmark proves the need**; absent that evidence, pooling is unjustified complexity.

### Layering

```
flui-virtualization        (core; Core.1-level; protocol-agnostic windowing math)
        ▲
        │  SliverConstraints → ScrollWindow adapter
        │  lazy SliverList consumer
        │  re-entrant build mechanism (LayoutContextApi capability)
flui-rendering
        ▲
        │  lazy widgets (future)
flui-view
```

All dependency arrows point upward only — **acyclic**. The agnostic core sits below the render layer; the render layer adapts the sliver protocol onto it; `flui-view` (future) consumes the lazy render objects.

---

## Consequences

- **A new crate joins the workspace.** `flui-virtualization` is added to `[workspace.members]` and `default-members`; it carries the standard Layer-0/Core.1 crate metadata, lints, and CI shape (mirroring an existing low-level crate such as `flui-geometry`).
- **`FenwickExtents`'s import path moves** (breaking) — but it has **zero callers**, so the break is contained to the move itself. Acceptable per the "breaking changes allowed now" mandate.
- **A breaking *addition* to `LayoutContextApi`** — the re-entrant build hook. This widens the layout-context surface every `RenderObject` author sees; it is gated (see below) because it is part of the public contract a future session must not silently reshape.
- **The `Virtualizer` core must be verifiable in isolation** (unit tests + a criterion bench in the new crate) **and** through the existing `render_viewport` integration harness, **plus** a real-frame bench against the eager list. **No `flui-view` consumer is required** to validate the core or the first lazy `SliverList`.
- **Honest scaffold-vs-wired labeling is required.** Where v1 ships the next-frame backend rather than true mid-pass, docs and code comments must say so plainly. **No false performance claims** — a bench that measures the next-frame backend must not be reported as if it measured mid-pass build, and a virtualized list's win must be demonstrated against the eager baseline on a real frame, not asserted.

---

## Rejected alternatives

| Option | Why rejected |
|---|---|
| **(a) Virtualization as a submodule inside `flui-rendering`** | Shallow module; couples windowing math to the render layer *and* the sliver protocol; not reusable by `flui-view` lazy widgets or by general windowed UIs (text, grid, table, timeline). Violates the deep-module / information-hiding rationale. |
| **(b) A `SliverProtocol`-bound `Virtualizer`** | Special-purpose and shallow — it could serve only slivers. Also forces a dependency on `flui-rendering` (where `SliverConstraints` lives), creating a cycle with the render layer's own dependence on the core. |
| **(c) Next-frame-only build with no mid-pass contract** | Locks out the market-best *true* mid-pass build (Compose `SubcomposeLayout` capability) permanently — exactly the ossification trap the future-proof mandate exists to avoid. A next-frame *backend* is fine as a stepping stone; a next-frame-only *contract* is not. |
| **(d) RecyclerView two-level recycling pool now** | Cargo-culted. Its justifying constraint (expensive `View` creation + GC) is absent in a Rust arena with cheap `can_update`. Adds real complexity for a benefit unproven on this runtime. Revisit only if a real-frame bench proves the need. |

---

## References

- Mislocated backbone (to move): `crates/flui-rendering/src/slivers/fenwick.rs` (`FenwickExtents`, correct, ASM-verified, zero callers, `Vec<f32>`-only).
- Protocol-coupled types (do **not** build the core on these): `SliverConstraints` / `SliverGeometry` in `flui-rendering`.
- Next-frame insertion path (v1 build backend candidate): `crates/flui-rendering/src/pipeline/deferred.rs`; `PipelineOwner::apply_deferred_mutation` at `crates/flui-rendering/src/pipeline/owner.rs:2021` (drained at end of `run_layout`).
- Eager consumers to replace/augment: `crates/flui-rendering/src/objects/sliver_fixed_extent_list.rs`; `crates/flui-rendering/src/objects/sliver_fill_viewport.rs`.
- Eager viewport walk lacking a skip-child hook: `crates/flui-rendering/src/objects/viewport.rs` — `layout_child_sequence` at `:325`, `try_cached_sliver_geometry` at `:612`.
- Foundation crates the core may depend on: `crates/flui-types`, `crates/flui-foundation`, `crates/flui-geometry`.
- Borrow-safety discipline inherited: ADR-0002 (U20/U20.1 disjoint-subtree borrow primitive; deferred-queue mid-pass-marks handling).
- External prior art: TanStack Virtual (measured-extent + estimate); GPUI `SumTree` (`O(log n)` pixel↔index summary seek); Jetpack Compose `LazyColumn` + `SubcomposeLayout` (true mid-pass build, costly per its own docs); Android `RecyclerView` `GapWorker` prefetch + two-level pool (justifying constraint = expensive `View` + GC); Flutter `RenderSliverList` (linked-list tracking, estimate jitter, synchronous `cacheExtent`, no precise anchor correction on resize).
- Design principle: Ousterhout, *A Philosophy of Software Design* (deep modules, information hiding).

---

## Implementation

See the sequenced plan: [`docs/plans/2026-06-12-001-feat-virtualization-core-and-reentrant-build-plan.md`](../plans/2026-06-12-001-feat-virtualization-core-and-reentrant-build-plan.md). Units U1 (crate + core + Fenwick move), U2 (mid-pass-capable build contract), U3 (lazy `SliverList` consumer), U4 (future: grid/staggered, `flui-view` lazy widgets, recycling-if-benched), with ARCH-GATE on U2's contract, API-GATE on the crate's public surface, and QA via the `render_viewport` harness.
