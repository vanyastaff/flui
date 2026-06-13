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

**Target architecture (one paragraph).** flui gains a new **deep, protocol-agnostic `virtualization` module *inside* `flui-rendering`** that owns *windowing math only*: given a `ScrollWindow { offset, main_extent, cache_before, cache_after }` and a set of per-item extents (measured or estimated), it answers "which item indices are visible (plus cache buffer)?" in `O(log n)` — **and supports `O(log n)` structural edits (mid-list insert/delete via `set_count`)** — and corrects the scroll anchor, by **item identity** rather than raw pixel, when a measured extent differs from its estimate. Its public surface names **no** render, sliver, or protocol type — only `ScrollWindow` / `VisibleRange` / `AnchorCorrection` / `Extent` / `f32` extents — so it stays a general-purpose abstraction (enforced by the API-GATE). Its backbone is a **focused augmented B+-tree (SumTree-style) over extents** — each item is type-level `Unmeasured { hint } | Measured { extent }`, every internal node carries a `{ count, total_extent }` summary — giving `O(log n)` seek in **both** directions (offset→index and index→offset) **and** `O(log n)` insert/delete. The correct-but-uncalled `FenwickExtents` (currently `crates/flui-rendering/src/slivers/fenwick.rs`) is **DELETED** in U1 (zero callers; a Fenwick/BIT is the structurally-wrong tool for a dynamic list — see Current state and Rejected alternatives) and replaced by this SumTree/B+-tree. `flui-rendering` **hosts** the agnostic module and **consumes** it via a thin `SliverConstraints → ScrollWindow` adapter (which lives in `flui-rendering` but *outside* the agnostic module): it drives the `Virtualizer` for the visible range and builds/lays-out only the visible-plus-cache children. The on-demand child-build mechanism is exposed as a **new, mid-pass-capable `LayoutContextApi` capability** whose contract permits true mid-pass re-entrant build (Compose `SubcomposeLayout` style) without a later breaking change — even though the **v1 backend may materialize children via the existing next-frame deferred-mutation queue plus cache-region prefetch**. Recycling in v1 is **dispose-on-scroll-off behind a pluggable hook**, not RecyclerView's two-level pool. The windowing-math **abstraction is built now** and locked into its agnostic shape; only the reversible **packaging** as a standalone `flui-virtualization` crate is deferred — a cheap, mechanical, non-breaking lift once a 2nd *direct* consumer appears.

**Why this is decided now, and recorded durably.** flui is a Flutter→Rust framework built to *beat* Flutter, and breaking changes are allowed **now** while the contracts are not yet locked. The explicit decision driver is to avoid Flutter's ossification trap: contracts locked early cannot later adopt the market's better solutions. The two load-bearing commitments here — **(1)** the virtualization core is *agnostic* (protocol-free and build-free), and **(2)** the build contract is *mid-pass-capable from day one* — are precisely the commitments that keep the future-proof, market-best abstraction reachable. They are recorded here so a future session with zero memory cannot accidentally re-couple the core to the sliver protocol, or quietly settle for a next-frame-only build contract that locks out true mid-pass. This is **first-class, not a vague "later."**

---

## Context

### What virtualization is, and why flui needs a real one

A virtualized (a.k.a. windowed or lazy) list renders only the items intersecting the viewport plus a small cache buffer, instead of all `n` items. Doing this well requires four pieces of windowing math: (a) an `O(log n)` map between scroll offset and item index, in **both** directions, over a running sum of item extents; (b) `O(log n)` **structural edits** — mid-list insert/delete — so dynamic lists (`set_count`, infinite feeds, reorder) stay cheap (the requirement that rules out a flat-array Fenwick and selects an augmented B+-tree / SumTree); (c) an *estimate* for items not yet measured, so the total scroll extent and the scrollbar are stable before every item has been laid out; and (d) an *anchor correction* — keyed on **item identity**, not raw pixel — that keeps the on-screen content from jumping when a measured extent turns out to differ from its estimate. These concerns are a self-contained abstraction layer with nothing render-specific about them — they are equally the math behind a virtualized list, grid, data table, timeline, or text view.

### Current state of the code (verified via scout, 2026-06-12)

- **`FenwickExtents`** (`crates/flui-rendering/src/slivers/fenwick.rs`) is **correct as a BIT, ASM-verified, and fully self-contained** — it depends only on `Vec<f32>` and has *zero* coupling to any render or protocol type, with **zero callers** today. But it is the **structurally wrong tool for a dynamic list**: a Fenwick/BIT gives `O(log n)` point-update + prefix-sum over a *flat array*, so mid-list **insert/delete is `O(n)`** (index shift). It works only for append/truncate-at-tail with stable indices — not flui's target of dynamic lists (`set_count`, infinite feeds, reorder). It is therefore **to be DELETED in U1**, replaced by a SumTree/augmented-B+-tree (which gives `O(log n)` structural edits). It was scaffold for a structurally-wrong plan. (Web research, 2026-06-12: see Competitive landscape and References.)
- **`SliverConstraints` / `SliverGeometry`** are tightly bound to `SliverProtocol` and to the viewport walk. They are *not* a neutral windowing value type; building the virtualization core on top of them would couple it to the render layer and the sliver protocol.
- **The deferred-mutation queue** (`crates/flui-rendering/src/pipeline/deferred.rs` + `PipelineOwner::apply_deferred_mutation`, `crates/flui-rendering/src/pipeline/owner.rs:2021`, drained at the *end* of `run_layout`) is fully wired, but it is **next-frame materialization**: an `Insert` yields its `RenderId` at apply-time, *after* layout has finished — it is **not** mid-pass re-entrancy. It is, however, a sound, borrow-safe insertion path that can serve as a v1 build backend (see Decision 2).
- **Existing sliver lists** (`crates/flui-rendering/src/objects/sliver_fixed_extent_list.rs`, `crates/flui-rendering/src/objects/sliver_fill_viewport.rs`) lay out **all** children eagerly, `O(n)`.
- **The viewport** (`crates/flui-rendering/src/objects/viewport.rs`, `layout_child_sequence` at `:325`) drives children eagerly with a per-child constraint cache (`try_cached_sliver_geometry` at `:612`) but has **no "skip this child" hook** — i.e., no way for a sliver to lay out only a sub-range of its children.

So the substrate is *half-present and mis-shaped*: the one piece of self-contained math that exists (`FenwickExtents`) is unused **and is the wrong structure** (a flat-array BIT, `O(n)` mid-list insert) — it will be deleted, not relocated; the sliver/viewport machinery is eager and protocol-coupled; and the insertion path that exists is next-frame, not mid-pass.

### Competitive landscape (what to beat, what to borrow, what to ignore)

Web research (2026-06-12; primary sources in References) produced a **decisive backbone verdict: a SumTree (augmented B+-tree), not a Fenwick tree.** The per-engine survey:

| Engine | Backing structure | Offset↔index seek | Mid-list insert/delete | Anchor model |
|---|---|---|---|---|
| **GPUI / Zed** (`list.rs`) | **`SumTree<ListItem>`** — `ListItem = Unmeasured { size_hint } \| Measured { size }`, `{ count, height }` summary at every node | **`O(log n)` both directions** | **`O(log n)`** | item-identity (`ListOffset { item_ix, offset_in_item }`) |
| **TanStack Virtual** | flat array + binary search over a measurement cache | `O(log n)` search | **`O(n)` rebuild** (array shift / recompute) | item-key |
| **Flutter `RenderSliverList`** | linked-list dead-reckoning + `scrollOffsetCorrection` | linear walk from a known child | linked-list splice | **raw-pixel** → jitter (#97676) |
| **Jetpack Compose `LazyColumn`** | **no persistent structure** — re-derives per frame | n/a (recomputed) | n/a | **item-key** |
| **egui** | uniform row height only | `O(1)` (`offset = index × extent`) | trivial (uniform) | n/a (no variable extents) |
| **Xilem** | fixpoint measure loop | iterative | iterative | fixpoint convergence |

**The Fenwick-vs-SumTree verdict.** A Fenwick/BIT gives `O(log n)` point-update + prefix-sum but is a *flat array*, so **mid-list insert/delete is `O(n)`** — it is correct only for append/truncate-at-tail with stable indices. flui targets Flutter-port quality (dynamic lists, `set_count`, infinite feeds, reorder), all of which need `O(log n)` structural edits; building on Fenwick would lock in an `O(n)`-insert structure (the ossification trap). **GPUI/Zed ships exactly the chosen structure in production Rust** — `SumTree<ListItem>` with the `Unmeasured | Measured` enum and a `{count, height}` summary, giving `O(log n)` in both seek directions *and* `O(log n)` edits (primary source: `crates/gpui/src/elements/list.rs`; design write-up: Zed's rope/SumTree blog). That is the decisive evidence the SumTree is the right tool and that it ships in prod Rust.

What to **borrow / beat / ignore** from the survey:

- **GPUI / Zed `SumTree`** — *adopt the structure.* The `Unmeasured | Measured` enum makes estimated-vs-measured **type-level** (illegal states unrepresentable, not a side boolean), and the item-identity anchor (`ListOffset`) is what keeps it jitter-free. flui's backbone is a *focused* augmented B+-tree (summaries: count + total extent), not GPUI's fully-generic `SumTree<T, Summary>` — generality lives at the `Virtualizer`'s public boundary, internals stay a deep module.
- **TanStack Virtual** — *borrow the estimate-then-correct model* (estimate-for-unmeasured + correct-on-measure for variable heights); *do not* copy its flat-array backing (`O(n)` rebuild on structural change).
- **Jetpack Compose `LazyColumn` + `SubcomposeLayout`** — the reference implementation of **true mid-pass build**: a layout composes (builds) children *during its own measure pass*, deciding what to build from available size. **This is the market-best capability we refuse to lock out.** Compose anchors by **item-key**, confirming the identity-anchor choice. Its own docs warn `SubcomposeLayout` is costly (defers composition, can cost a frame) — *why* flui keeps a cheaper next-frame backend as a stepping stone while the *contract* matches Compose's capability.
- **Android `RecyclerView`** — `GapWorker` idle-time prefetch + a two-level view-recycling pool. Its *justifying constraint* is that creating an Android `View` is expensive and the platform is GC'd, so reuse pays for itself. **That constraint does not hold for a Rust arena with `can_update`** (cheap in-place reconcile, no GC). Borrow the *idle/cache-region prefetch* idea (latency hidden by the cache buffer); **reject the two-level pool** as cargo-cult for this runtime.
- **Flutter `RenderSliverList`** — the thing to beat. Its weaknesses: linked-list child tracking with **estimate-based scroll-extent jitter**; `cacheExtent` computed **synchronously in the scroll frame**; and a **raw-pixel anchor** that silently jumps when an item above the viewport is re-measured (issue #97676). flui's core fixes exactly these: a SumTree/B+-tree instead of a linked list, an explicit `AnchorCorrection` keyed on **item identity** `(index, sub_offset)` rather than raw pixel, and a cache-region model that does not have to be recomputed inline every scroll frame.
- **egui / Xilem** — egui supports *uniform* row height only (`offset = index × extent` is `O(1)`, no tree needed — flui's fixed-extent path agrees); Xilem resolves sizing with a fixpoint measure loop (the cautionary case behind the `scroll_to_item` fixpoint-measure caveat for an unmeasured target).

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
    // tight visible band [first, last)
    pub first: usize,
    pub last: usize,
    // extended cache band [cache_first, cache_last) — so callers prioritize render vs measure
    pub cache_first: usize,
    pub cache_last: usize,
    // first item's offset minus window.offset (≤ 0) — for placing the leading child
    pub leading_offset: f32,
}

pub struct AnchorCorrection {
    // SIGNED pixel delta; the caller accumulates and chooses sync/deferred apply
    pub delta: f32,
}

// Per-item extent is type-level (illegal states unrepresentable) — GPUI's enum pattern,
// NOT a side `is_measured` boolean.
pub enum ItemExtent {
    Unmeasured { hint: f32 },
    Measured { extent: f32 },
}

// Total extent distinguishes "all measured" from "still estimated" for scrollbar stability.
pub enum Extent {
    Exact(f32),
    Estimated(f32),
}
```

**Backbone — a focused augmented B+-tree (SumTree-style), not a Fenwick tree.** The module's internal structure is an augmented B+-tree over per-item extents, carrying a `{ item count, total extent }` summary at every internal node. This gives `O(log n)` seek in **both** directions (offset→index and index→offset) **and** `O(log n)` insert/delete — the structural-edit capability a dynamic list (`set_count`, infinite feeds, reorder) requires and that a Fenwick/BIT cannot provide. Each item is the GPUI enum pattern — `Unmeasured { hint } | Measured { extent }` — so estimated-vs-measured is **type-level**, not a side boolean. The tree is kept a **focused** augmented B+-tree (summaries = count + total extent), not a fully-generic `SumTree<T, Summary>`: internals stay a deep module, and generality lives at the `Virtualizer`'s public boundary, not in maximal internal genericity. (**U1 sub-decision:** build this in-house — lean for control + zero-dep — *or* vet an existing crate via `/add-dep`; record the choice in U1.)

**Anchor is item-identity, not raw pixel.** The scroll anchor is `(index, sub_offset)`, mirroring every jitter-free engine (GPUI `ListOffset { item_ix, offset_in_item }`, Compose item-key, TanStack item-key). A raw-pixel anchor silently jumps when an item *above* the viewport is re-measured — Flutter's #97676 bug. `AnchorCorrection { delta }` is a **signed** pixel delta the caller accumulates and applies sync or deferred at its discretion.

**Refined public `Virtualizer` surface (the API-GATE surface — names no render/sliver/protocol type):**

- `new(item_count, default_estimate)`, `len`, `is_empty`.
- `set_count(n)` — insert/remove items, `O(log n)` via the tree (not an array shift).
- `set_measured(index, extent, anchor: (usize, f32)) -> Option<AnchorCorrection>` — replace an estimate with the real extent; returns a correction **iff** the change shifts content above the anchor item.
- `query(&ScrollWindow) -> VisibleRange` — `O(log n)`, returns the **dual** range (tight visible `[first, last)` + cache `[cache_first, cache_last)`) plus `leading_offset`.
- `offset_of(index) -> f32`, `is_measured(index) -> bool`.
- `invalidate_from(index)` — watermark; extents after a structural change recompute lazily, `O(log n)` in the tree.
- `anchor_item() -> (usize, f32)` — getter so the consumer restores position after a layout invalidation.
- `total_extent() -> Extent` where `Extent = Exact(f32) | Estimated(f32)`; plus `measured_count()` / `estimated_count()` for scrollbar stability (Flutter #97676 is the cautionary tale of average-based jumpiness).
- `scroll_to_item(index, alignment)` — note the fixpoint-measure caveat when scrolling to an *unmeasured* target.

- **No new workspace crate** is added in this delivery. The module is **agnostic by API contract** (its `pub` surface names no render/sliver/protocol type), so it can be lifted into a standalone `flui-virtualization` crate later — cheaply, mechanically, and non-breakingly — once a 2nd *direct* consumer appears. The agnostic *shape* (the part that ossifies into a contract) is locked correctly **now**; the crate boundary (reversible packaging, *not* a contract) is deferred.
- **`FenwickExtents` is DELETED in U1** — it has **zero callers**, and the research verdict is that a Fenwick/BIT is the structurally-wrong tool for a dynamic list (`O(n)` mid-list insert). It is removed from `crates/flui-rendering/src/slivers/fenwick.rs` and **replaced by the SumTree/augmented-B+-tree** backbone described above (built in the new `virtualization` module). Fixed-extent lists need no tree at all (`offset = index × extent` is `O(1)`); only variable-extent lists need the SumTree. (No cross-crate move; the deletion breaks no import because there are no callers — see Consequences.)
- **`flui-rendering` hosts the agnostic module and consumes it via an adapter:** a thin `SliverConstraints → ScrollWindow` adapter lives in `flui-rendering` but **outside** the `virtualization` module — the module must never see `SliverConstraints`. The consumption arrow is intra-crate: `objects/slivers + adapter → virtualization module`.

**Rationale (sole direct consumer → a crate now is premature).** The **only direct consumer** of the windowing core is `flui-rendering` itself (its sliver list/grid/staggered render objects). `flui-view` lazy widgets — and any data-grid, text, table, or timeline — are **sliver-based widgets that consume the core *through* `flui-rendering`'s slivers, not directly.** With a single direct consumer, a separate **crate** is premature decomposition: a crate boundary's cost (a compile unit, a workspace member, versioning, cross-crate friction) is not paid back without **≥2 direct consumers**.

**Rationale (A Philosophy of Software Design, Ousterhout — applied correctly).** This is a *deep module*: a small interface (window query, extent update, anchor correction) hiding substantial complexity (the augmented B+-tree / SumTree over extents, the estimate model for unmeasured items, anchor-correction arithmetic). It enforces **information hiding** — callers never touch the tree or the summary representation. Ousterhout's point is *"deep modules, not numerous (shallow) components"* — and a **one-consumer crate is precisely a shallow component**. A standalone crate now would *over-apply* Ousterhout to justify a crate boundary the consumer count does not earn; a deep **module** is the faithful reading. Keeping windowing math a distinct module from the sliver protocol still keeps each interface narrow and each layer independently testable — the crate boundary is not required for that.

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

**Consumer note (lives with the consumer, U3 — *not* in the agnostic core).** The lazy `SliverList` must **suppress anchor correction while scrolling backward** — applying corrections during an upward scroll is the canonical "items jump while scrolling up" bug — and must **reset the correction accumulator on user-initiated scroll**. The agnostic `Virtualizer` only *emits* the signed `AnchorCorrection`; the *policy* of when to apply, defer, or suppress it (and when to zero the accumulator) is a consumer concern and belongs in U3, not the core.

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
- **`FenwickExtents` is DELETED** (0 callers, structurally wrong for a dynamic list — `O(n)` mid-list insert). No other crate's import path changes because nothing imported it.
- **A focused augmented B+-tree / SumTree is built (or an existing crate vetted via `/add-dep`) — the core's main implementation cost.** The structure (count + total-extent summaries, `Unmeasured | Measured` items, `O(log n)` seek-both-ways + `O(log n)` edits) replaces the deleted Fenwick and is the bulk of U1's work. The in-house-vs-crate choice is recorded as a U1 sub-decision (lean in-house for control + zero-dep).
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
| **(d) RecyclerView two-level recycling pool now** | Cargo-culted. Its justifying constraint (expensive `View` creation + GC) is absent in a Rust arena with cheap `can_update`. **Reinforced by the research:** the pool exists to amortize Android `View` *inflation* cost under GC; in a Rust arena reconcile is cheap and there is no GC, so the rationale does not transfer. Adds real complexity for a benefit unproven on this runtime. Revisit only if a real-frame bench proves the need. |
| **(e) Fenwick / BIT backbone** | Rejected. A Fenwick/BIT gives `O(log n)` point-update + prefix-sum but is a flat array — **mid-list insert/delete is `O(n)`** (index shift). It is fine only for append-only, stable-index lists; flui needs dynamic lists (`set_count`, infinite feeds, reorder), which require `O(log n)` structural edits. **GPUI/Zed proves a `SumTree` (augmented B+-tree) ships in production Rust** with `O(log n)` in both seek directions *and* `O(log n)` edits. The existing `FenwickExtents` (zero callers) is deleted in U1 and replaced by the SumTree/B+-tree. (Chosen instead: the focused augmented B+-tree of Decision 1.) |

---

## References

- Backbone to **delete** (zero callers; structurally wrong — flat-array BIT, `O(n)` mid-list insert): `crates/flui-rendering/src/slivers/fenwick.rs` (`FenwickExtents`). Replaced in U1 by a focused augmented B+-tree / SumTree (count + total-extent summaries) in the new `virtualization` module.
- Protocol-coupled types (do **not** build the core on these): `SliverConstraints` / `SliverGeometry` in `flui-rendering`.
- Next-frame insertion path (v1 build backend candidate): `crates/flui-rendering/src/pipeline/deferred.rs`; `PipelineOwner::apply_deferred_mutation` at `crates/flui-rendering/src/pipeline/owner.rs:2021` (drained at end of `run_layout`).
- Eager consumers to replace/augment: `crates/flui-rendering/src/objects/sliver_fixed_extent_list.rs`; `crates/flui-rendering/src/objects/sliver_fill_viewport.rs`.
- Eager viewport walk lacking a skip-child hook: `crates/flui-rendering/src/objects/viewport.rs` — `layout_child_sequence` at `:325`, `try_cached_sliver_geometry` at `:612`.
- Foundation crates the core's surface stays within (and the only deps a future extracted `flui-virtualization` crate would take): `crates/flui-types`, `crates/flui-foundation`, `crates/flui-geometry`.
- Borrow-safety discipline inherited: ADR-0002 (U20/U20.1 disjoint-subtree borrow primitive; deferred-queue mid-pass-marks handling).
- Design principle: Ousterhout, *A Philosophy of Software Design* (deep modules, information hiding).

### External prior art / research citations (web research, 2026-06-12)

- **GPUI / Zed `list.rs`** — `SumTree<ListItem>`, `ListItem = Unmeasured { size_hint } | Measured { size }`, `{ count, height }` summary; `O(log n)` seek both ways + `O(log n)` edits; item-identity anchor `ListOffset { item_ix, offset_in_item }`. **Primary source / decisive evidence the SumTree ships in prod Rust.** <https://github.com/zed-industries/zed/blob/main/crates/gpui/src/elements/list.rs>
- **Zed SumTree / rope design write-up** — why an augmented B+-tree (SumTree) is Zed's general structure. <https://zed.dev/blog/zed-decoded-rope-sumtree>
- **Flutter #97676** — raw-pixel-anchor jitter ("items jump") on re-measure above the viewport; the cautionary tale behind item-identity anchoring + scrollbar stability. <https://github.com/flutter/flutter/issues/97676>
- **TanStack Virtual internals** — flat array + binary search over a measurement cache (`O(n)` rebuild on structural change); estimate-then-correct model worth borrowing. <https://deepwiki.com/TanStack/virtual>
- **WICG Scroll Anchoring** — the web platform's anchor-node model (anchor by element, not pixel) corroborating the identity-anchor choice. <https://github.com/WICG/ScrollAnchoring>
- Other surveyed engines: Jetpack Compose `LazyColumn` + `SubcomposeLayout` (true mid-pass build, item-key anchor, costly per its own docs); Android `RecyclerView` `GapWorker` prefetch + two-level pool (justifying constraint = expensive `View` + GC, absent in a Rust arena); egui (uniform row height only); Xilem (fixpoint measure loop).

---

## Implementation

See the sequenced plan: [`docs/plans/2026-06-12-001-feat-virtualization-core-and-reentrant-build-plan.md`](../plans/2026-06-12-001-feat-virtualization-core-and-reentrant-build-plan.md). Units U1 (`virtualization` module + SumTree/B+-tree-backed core; delete `FenwickExtents`), U2 (mid-pass-capable build contract), U3 (lazy `SliverList` consumer, incl. backward-scroll correction suppression), U4 (future: grid/staggered, `flui-view` lazy widgets, crate-extraction on 2nd direct consumer, recycling-if-benched), with ARCH-GATE on U2's contract, API-GATE on the **module's** public surface, and QA via the `render_viewport` harness.
