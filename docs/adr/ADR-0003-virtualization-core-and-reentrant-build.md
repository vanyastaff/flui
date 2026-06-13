# ADR-0003: Generic virtualization core (deep, protocol-agnostic `virtualization` module in `flui-rendering`, crate-extractable when a 2nd direct consumer appears) + re-entrant build foundation

*Build windowing math as a deep, protocol-agnostic `virtualization` module inside `flui-rendering` — extractable into a standalone `flui-virtualization` crate as a cheap, non-breaking lift once a 2nd direct consumer appears — and design the on-demand child-build contract to be mid-pass-capable from day one — so the market-best true-mid-pass build is never locked out, even if v1 ships a next-frame backend.*

---

- **Status:** Accepted
- **Date:** 2026-06-12
- **Deciders:** @vanyastaff
- **Scope:** new `virtualization` module in `flui-rendering` (crate-extraction to `flui-virtualization` deferred to a 2nd-direct-consumer trigger); `flui-rendering` (slivers, `LayoutContextApi`, lazy `SliverList`); `flui-view` (lazy widgets, future)
- **Relates to:** ADR-0002 (engine-wide threading) — inherits the U20/U20.1 disjoint-subtree borrow discipline and the deferred-mutation queue as the borrow-safe substrate for re-entrant build

---

## Verdict

**Target architecture (one paragraph).** flui gains a new **deep, protocol-agnostic `virtualization` module *inside* `flui-rendering`** that owns *windowing math only*: given a `ScrollWindow { offset, main_extent, cache_before, cache_after }` and a set of per-item extents (measured or estimated), it answers "which item indices are visible (plus cache buffer)?" in `O(log n)`, and corrects the scroll anchor when a measured extent differs from its estimate. Its public surface names **no** render, sliver, or protocol type — only `ScrollWindow` / `VisibleRange` / `AnchorCorrection` / `f32` extents — so it stays a general-purpose abstraction (enforced by the API-GATE). The correct-but-uncalled `FenwickExtents` (currently `crates/flui-rendering/src/slivers/fenwick.rs`) is **reorganized into this module** — staying in `flui-rendering`, no cross-crate move — as its prefix-sum backbone. `flui-rendering` **hosts** the agnostic module and **consumes** it via a thin `SliverConstraints → ScrollWindow` adapter (which lives in `flui-rendering` but *outside* the agnostic module): it drives the `Virtualizer` for the visible range and builds/lays-out only the visible-plus-cache children. The on-demand child-build mechanism is exposed as a **new, mid-pass-capable `LayoutContextApi` capability** whose contract permits true mid-pass re-entrant build (Compose `SubcomposeLayout` style) without a later breaking change — even though the **v1 backend may materialize children via the existing next-frame deferred-mutation queue plus cache-region prefetch**. Recycling in v1 is **dispose-on-scroll-off behind a pluggable hook**, not RecyclerView's two-level pool. The windowing-math **abstraction is built now** and locked into its agnostic shape; only the reversible **packaging** as a standalone `flui-virtualization` crate is deferred — a cheap, mechanical, non-breaking lift once a 2nd *direct* consumer appears.

**Why this is decided now, and recorded durably.** flui is a Flutter→Rust framework built to *beat* Flutter, and breaking changes are allowed **now** while the contracts are not yet locked. The explicit decision driver is to avoid Flutter's ossification trap: contracts locked early cannot later adopt the market's better solutions. The two load-bearing commitments here — **(1)** the virtualization core is *agnostic* (protocol-free and build-free), and **(2)** the build contract is *mid-pass-capable from day one* — are precisely the commitments that keep the future-proof, market-best abstraction reachable. They are recorded here so a future session with zero memory cannot accidentally re-couple the core to the sliver protocol, or quietly settle for a next-frame-only build contract that locks out true mid-pass. This is **first-class, not a vague "later."**

---

## Context

### What virtualization is, and why flui needs a real one

A virtualized (a.k.a. windowed or lazy) list renders only the items intersecting the viewport plus a small cache buffer, instead of all `n` items. Doing this well requires three pieces of windowing math: (a) an `O(log n)` map between scroll offset and item index over a running prefix-sum of item extents; (b) an *estimate* for items not yet measured, so the total scroll extent and the scrollbar are stable before every item has been laid out; and (c) an *anchor correction* that keeps the on-screen content from jumping when a measured extent turns out to differ from its estimate. These three concerns are a self-contained abstraction layer with nothing render-specific about them — they are equally the math behind a virtualized list, grid, data table, timeline, or text view.

### Current state of the code (verified via scout, 2026-06-12)

- **`FenwickExtents`** (`crates/flui-rendering/src/slivers/fenwick.rs`) is **correct, ASM-verified, and fully self-contained** — it depends only on `Vec<f32>` and has *zero* coupling to any render or protocol type. It is the prefix-sum backbone a virtualizer needs. It has **zero callers** today. (It lives in the right *shape* but the wrong *module*: it sits under `slivers/` rather than in a neutral windowing module — a reorganization *within* `flui-rendering`, not a cross-crate move.)
- **`SliverConstraints` / `SliverGeometry`** are tightly bound to `SliverProtocol` and to the viewport walk. They are *not* a neutral windowing value type; building the virtualization core on top of them would couple it to the render layer and the sliver protocol.
- **The deferred-mutation queue** (`crates/flui-rendering/src/pipeline/deferred.rs` + `PipelineOwner::apply_deferred_mutation`, `crates/flui-rendering/src/pipeline/owner.rs:2021`, drained at the *end* of `run_layout`) is fully wired, but it is **next-frame materialization**: an `Insert` yields its `RenderId` at apply-time, *after* layout has finished — it is **not** mid-pass re-entrancy. It is, however, a sound, borrow-safe insertion path that can serve as a v1 build backend (see Decision 2).
- **Existing sliver lists** (`crates/flui-rendering/src/objects/sliver_fixed_extent_list.rs`, `crates/flui-rendering/src/objects/sliver_fill_viewport.rs`) lay out **all** children eagerly, `O(n)`.
- **The viewport** (`crates/flui-rendering/src/objects/viewport.rs`, `layout_child_sequence` at `:325`) drives children eagerly with a per-child constraint cache (`try_cached_sliver_geometry` at `:612`) but has **no "skip this child" hook** — i.e., no way for a sliver to lay out only a sub-range of its children.

So the substrate is *half-present*: the hard, correct math (`FenwickExtents`) exists but is unused and lives under `slivers/` rather than in a neutral windowing module; the sliver/viewport machinery is eager and protocol-coupled; and the insertion path that exists is next-frame, not mid-pass.

### Competitive landscape (what to beat, what to borrow, what to ignore)

- **TanStack Virtual** — measured-extent + estimate virtualization. Confirms the *estimate-then-correct* model is the right one for variable-height items. **Borrow:** the estimate-for-unmeasured + correct-on-measure shape.
- **GPUI `SumTree`** — a B+-tree carrying summary dimensions (e.g. `Count`, `Height`) that yields `O(log n)` pixel↔index seeks. Confirms the `O(log n)` offset↔index requirement and that a summed-dimension structure is the right tool. `FenwickExtents` is flui's (simpler, ASM-verified) equivalent of the height-summary seek.
- **Jetpack Compose `LazyColumn` + `SubcomposeLayout`** — the reference implementation of **true mid-pass build**: a layout can compose (build) children *during its own measure pass*, deciding what to build based on available size. **This is the market-best capability we refuse to lock out.** Compose's own docs warn `SubcomposeLayout` is costly (it defers composition and can cost a frame), which is *why* flui keeps a cheaper next-frame backend available as a stepping stone — but the *contract* matches Compose's capability.
- **Android `RecyclerView`** — `GapWorker` idle-time prefetch + a two-level view-recycling pool. Its *justifying constraint* is that creating an Android `View` is expensive and the platform is GC'd, so reuse pays for itself. **That constraint does not hold for a Rust arena with `can_update`** (cheap in-place reconcile, no GC). Borrow the *idle/cache-region prefetch* idea (latency hidden by the cache buffer); **reject the two-level pool** as cargo-cult for this runtime.
- **Flutter `RenderSliverList`** — the thing to beat. Its weaknesses: linked-list child tracking with **estimate-based scroll-extent jitter**; `cacheExtent` computed **synchronously in the scroll frame**; and **no precise anchor correction on resize**. flui's core fixes exactly these: a Fenwick/`SumTree`-class structure instead of a linked list, an explicit `AnchorCorrection` output, and a cache-region model that does not have to be recomputed inline every scroll frame.

---

## Decision

Three decisions, plus the layering that ties them together. They are deliberately separable: Decision 1 (module boundary) can land and be tested with no consumer; Decision 2 (build contract) is the breaking `LayoutContextApi` addition; Decision 3 (recycling) is a pluggable v1 default.

### Decision 1 — Boundary: a deep, protocol-agnostic `virtualization` module in `flui-rendering` (crate-extractable later)

Create a **new `virtualization` module *inside* `flui-rendering`** that is **protocol-agnostic**. Its public surface is generic over a plain value type and `f32` extents — it never names a render, sliver, or protocol type:

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

- **No new workspace crate** is added in this delivery. The module is **agnostic by API contract** (its `pub` surface names no render/sliver/protocol type), so it can be lifted into a standalone `flui-virtualization` crate later — cheaply, mechanically, and non-breakingly — once a 2nd *direct* consumer appears. The agnostic *shape* (the part that ossifies into a contract) is locked correctly **now**; the crate boundary (reversible packaging, *not* a contract) is deferred.
- **`FenwickExtents` is reorganized into this module** — it **stays in `flui-rendering`** (it already lives at `crates/flui-rendering/src/slivers/fenwick.rs`) and is moved into the new `virtualization` module as its prefix-sum backbone. **No cross-crate move, no breaking import for any other crate** (and it has zero callers regardless — see Consequences).
- **`flui-rendering` hosts the agnostic module and consumes it via an adapter:** a thin `SliverConstraints → ScrollWindow` adapter lives in `flui-rendering` but **outside** the `virtualization` module — the module must never see `SliverConstraints`. The consumption arrow is intra-crate: `objects/slivers + adapter → virtualization module`.

**Rationale (sole direct consumer → a crate now is premature).** The **only direct consumer** of the windowing core is `flui-rendering` itself (its sliver list/grid/staggered render objects). `flui-view` lazy widgets — and any data-grid, text, table, or timeline — are **sliver-based widgets that consume the core *through* `flui-rendering`'s slivers, not directly.** With a single direct consumer, a separate **crate** is premature decomposition: a crate boundary's cost (a compile unit, a workspace member, versioning, cross-crate friction) is not paid back without **≥2 direct consumers**.

**Rationale (A Philosophy of Software Design, Ousterhout — applied correctly).** This is a *deep module*: a small interface (window query, extent update, anchor correction) hiding substantial complexity (Fenwick prefix sums, the estimate model for unmeasured items, anchor-correction arithmetic). It enforces **information hiding** — callers never touch the prefix-sum representation. Ousterhout's point is *"deep modules, not numerous (shallow) components"* — and a **one-consumer crate is precisely a shallow component**. A standalone crate now would *over-apply* Ousterhout to justify a crate boundary the consumer count does not earn; a deep **module** is the faithful reading. Keeping windowing math a distinct module from the sliver protocol still keeps each interface narrow and each layer independently testable — the crate boundary is not required for that.

**Rationale (cheap, non-breaking later extraction).** Because the module is kept agnostic (API-GATE-enforced: no protocol type in its `pub` surface), extracting it into a standalone `flui-virtualization` crate **when a 2nd direct consumer appears** is a cheap, mechanical, **non-breaking** lift — the contract-shaped part (the abstraction's surface / its protocol-freedom) is already locked, and the crate boundary itself is not a contract that ossifies. This honors the future-proof mandate (the part that ossifies is locked right now) **and** avoids the opposite error (a premature crate). A protocol-agnostic core is reusable by: the render layer (sliver lists, now, *directly*); and — *through* `flui-rendering`'s slivers — `flui-view` lazy widgets (future) and general windowed UIs (virtualized text, data grids, tables, timelines). A sliver-bound core could serve only slivers.

**Rationale (dependency acyclicity).** The agnostic-module + intra-crate-adapter split also keeps dependencies clean *if/when* the module is extracted: `SliverConstraints` lives *in* `flui-rendering`, so a core that named it would have to depend on `flui-rendering` — which would itself depend on the core — a cycle. Keeping the module agnostic and the adapter on the `flui-rendering` side breaks that cycle by construction, so the future crate-extraction stays a clean downward dependency.

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
flui-rendering
  ┌──────────────────────────────────────────────────────────────┐
  │  objects/slivers  +  SliverConstraints → ScrollWindow adapter  │
  │            │  (adapter is OUTSIDE the agnostic module)         │
  │            ▼                                                   │
  │  virtualization module   (protocol-agnostic windowing math)    │
  │            ╎                                                   │
  │            ╎  ── future: extract to crate `flui-virtualization`│
  │            ╎     when a 2nd DIRECT consumer appears (deferred) │
  └──────────────────────────────────────────────────────────────┘
        ▲
        │  lazy widgets (future) — consume slivers, NOT the core directly
flui-view
```

The core lives as an **intra-crate module** inside `flui-rendering`: the sliver objects and the `SliverConstraints → ScrollWindow` adapter consume the agnostic `virtualization` module, with the adapter kept *outside* that module so the module never sees `SliverConstraints`. No new crate node joins the workspace now. Extraction into a standalone `flui-virtualization` crate is the dashed/deferred step — triggered by a 2nd *direct* consumer, and cheap because the module is gate-enforced agnostic. `flui-view` (future) consumes the lazy render objects, i.e. it reaches the core *through* `flui-rendering`'s slivers, not directly. All dependency arrows point one way — **acyclic**, both as a module today and as a crate after extraction.

---

## Consequences

- **A new agnostic `virtualization` module is added to `flui-rendering`** (no new crate). It carries the module-level structure for the windowing core; the workspace member list and `default-members` are unchanged.
- **`FenwickExtents` is reorganized, not moved cross-crate.** It stays in `flui-rendering` and relocates into the new `virtualization` module; no other crate's import path changes. Even the intra-crate relocation is trivial because it has **zero callers** today.
- **A breaking *addition* to `LayoutContextApi`** — the re-entrant build hook. This widens the layout-context surface every `RenderObject` author sees; it is gated (see below) because it is part of the public contract a future session must not silently reshape.
- **The `Virtualizer` core must be verifiable in isolation** (unit tests + a criterion bench, in the module) **and** through the existing `render_viewport` integration harness, **plus** a real-frame bench against the eager list. **No `flui-view` consumer is required** to validate the core or the first lazy `SliverList`.
- **Crate-extraction is a recorded future trigger.** Lifting the module into a standalone `flui-virtualization` crate happens when a **2nd direct consumer** appears; it is cheap, mechanical, and non-breaking precisely because the module is gate-enforced agnostic (no protocol type in its `pub` surface). Until then, a crate would be premature decomposition — a one-consumer compile unit whose boundary cost is not paid back.
- **Honest scaffold-vs-wired labeling is required.** Where v1 ships the next-frame backend rather than true mid-pass, docs and code comments must say so plainly. **No false performance claims** — a bench that measures the next-frame backend must not be reported as if it measured mid-pass build, and a virtualized list's win must be demonstrated against the eager baseline on a real frame, not asserted.

---

## Rejected alternatives

| Option | Why rejected |
|---|---|
| **(a) A separate `flui-virtualization` crate *now*** | Premature decomposition. There is exactly **one direct consumer** (`flui-rendering`'s slivers); `flui-view` lazy widgets and general windowed UIs (text, grid, table, timeline) consume the core *through* those slivers, not directly. A crate boundary's cost (compile unit, workspace member, versioning, cross-crate friction) is **unjustified without ≥2 direct consumers**. Deferred to a 2nd-direct-consumer trigger — a cheap, mechanical, non-breaking lift later **because the module is kept gate-enforced agnostic**. (Chosen instead: a deep, protocol-agnostic `virtualization` *module* inside `flui-rendering`.) |
| **(b) A `SliverProtocol`-bound `Virtualizer`** | Special-purpose and shallow — it could serve only slivers. Also forces a dependency on `flui-rendering` (where `SliverConstraints` lives), creating a cycle with the render layer's own dependence on the core — and would block the future clean crate-extraction. |
| **(c) Next-frame-only build with no mid-pass contract** | Locks out the market-best *true* mid-pass build (Compose `SubcomposeLayout` capability) permanently — exactly the ossification trap the future-proof mandate exists to avoid. A next-frame *backend* is fine as a stepping stone; a next-frame-only *contract* is not. |
| **(d) RecyclerView two-level recycling pool now** | Cargo-culted. Its justifying constraint (expensive `View` creation + GC) is absent in a Rust arena with cheap `can_update`. Adds real complexity for a benefit unproven on this runtime. Revisit only if a real-frame bench proves the need. |

---

## References

- Backbone to reorganize (stays in `flui-rendering`, relocates into the new `virtualization` module): `crates/flui-rendering/src/slivers/fenwick.rs` (`FenwickExtents`, correct, ASM-verified, zero callers, `Vec<f32>`-only).
- Protocol-coupled types (do **not** build the core on these): `SliverConstraints` / `SliverGeometry` in `flui-rendering`.
- Next-frame insertion path (v1 build backend candidate): `crates/flui-rendering/src/pipeline/deferred.rs`; `PipelineOwner::apply_deferred_mutation` at `crates/flui-rendering/src/pipeline/owner.rs:2021` (drained at end of `run_layout`).
- Eager consumers to replace/augment: `crates/flui-rendering/src/objects/sliver_fixed_extent_list.rs`; `crates/flui-rendering/src/objects/sliver_fill_viewport.rs`.
- Eager viewport walk lacking a skip-child hook: `crates/flui-rendering/src/objects/viewport.rs` — `layout_child_sequence` at `:325`, `try_cached_sliver_geometry` at `:612`.
- Foundation crates the core's surface stays within (and the only deps a future extracted `flui-virtualization` crate would take): `crates/flui-types`, `crates/flui-foundation`, `crates/flui-geometry`.
- Borrow-safety discipline inherited: ADR-0002 (U20/U20.1 disjoint-subtree borrow primitive; deferred-queue mid-pass-marks handling).
- External prior art: TanStack Virtual (measured-extent + estimate); GPUI `SumTree` (`O(log n)` pixel↔index summary seek); Jetpack Compose `LazyColumn` + `SubcomposeLayout` (true mid-pass build, costly per its own docs); Android `RecyclerView` `GapWorker` prefetch + two-level pool (justifying constraint = expensive `View` + GC); Flutter `RenderSliverList` (linked-list tracking, estimate jitter, synchronous `cacheExtent`, no precise anchor correction on resize).
- Design principle: Ousterhout, *A Philosophy of Software Design* (deep modules, information hiding).

---

## Implementation

See the sequenced plan: [`docs/plans/2026-06-12-001-feat-virtualization-core-and-reentrant-build-plan.md`](../plans/2026-06-12-001-feat-virtualization-core-and-reentrant-build-plan.md). Units U1 (`virtualization` module + core + Fenwick reorganize), U2 (mid-pass-capable build contract), U3 (lazy `SliverList` consumer), U4 (future: grid/staggered, `flui-view` lazy widgets, crate-extraction on 2nd direct consumer, recycling-if-benched), with ARCH-GATE on U2's contract, API-GATE on the **module's** public surface, and QA via the `render_viewport` harness.
