---
date: 2026-06-12
title: "feat: Virtualization core + re-entrant build foundation"
type: feat
status: active
adr: docs/adr/ADR-0003-virtualization-core-and-reentrant-build.md
---

# feat: Virtualization core + re-entrant build foundation

## Summary

Stand up a new **protocol-agnostic `virtualization` module *inside* `flui-rendering`** that owns windowing math (dual visible/cache-range query, estimate-for-unmeasured, item-identity anchor correction on measured-extent change), backed by a **focused augmented B+-tree (SumTree-style)** — `O(log n)` seek both directions **and** `O(log n)` insert/delete — **deleting the correct-but-structurally-wrong `FenwickExtents`** (a flat-array BIT, `O(n)` mid-list insert, zero callers). Then have `flui-rendering` *consume* it: a `SliverConstraints → ScrollWindow` adapter (outside the agnostic module), a **mid-pass-capable on-demand child-build contract** on `LayoutContextApi`, and a lazy `SliverList` that builds/lays-out only the visible-plus-cache children. A standalone `flui-virtualization` crate is **not** created now — the module is kept gate-enforced agnostic so it can be extracted later, cheaply and non-breakingly, once a 2nd *direct* consumer appears. Binding architecture: [`docs/adr/ADR-0003-virtualization-core-and-reentrant-build.md`](../adr/ADR-0003-virtualization-core-and-reentrant-build.md).

Delivered as **4 units** (U1 → U2 → U3, plus U4 recorded-future). The two load-bearing invariants the ADR locks in — **(1)** the core is agnostic (no render/sliver/protocol type), and **(2)** the build contract is mid-pass-capable from day one (true mid-pass never locked out, even if v1 ships a next-frame backend) — are the gates this plan must not silently relax.

---

## Problem Frame

flui's virtualization substrate is half-present and mis-shaped (verified via scout, 2026-06-12):

- `FenwickExtents` (`crates/flui-rendering/src/slivers/fenwick.rs`) — a correct, ASM-verified, self-contained (`Vec<f32>`-only) BIT with **zero callers** — is the **structurally wrong tool for a dynamic list**: a Fenwick/BIT is a flat array, so mid-list **insert/delete is `O(n)`**. It is to be **DELETED** (not relocated) and replaced by a SumTree/augmented-B+-tree (`O(log n)` structural edits). Web research, 2026-06-12 (GPUI/Zed `list.rs`, TanStack, Flutter #97676, Compose) produced this decisive backbone verdict — see the ADR's Competitive landscape and References.
- `SliverConstraints` / `SliverGeometry` are tightly bound to `SliverProtocol` and the viewport walk — *not* a neutral windowing value type.
- The deferred-mutation queue (`crates/flui-rendering/src/pipeline/deferred.rs` + `PipelineOwner::apply_deferred_mutation` at `crates/flui-rendering/src/pipeline/owner.rs:2021`, drained at end of `run_layout`) is fully wired but is **next-frame** materialization, not mid-pass re-entrancy.
- Existing sliver lists (`crates/flui-rendering/src/objects/sliver_fixed_extent_list.rs`, `.../sliver_fill_viewport.rs`) lay out **all** children eagerly `O(n)`.
- The viewport (`crates/flui-rendering/src/objects/viewport.rs`, `layout_child_sequence` at `:325`, `try_cached_sliver_geometry` at `:612`) drives children eagerly with a per-child constraint cache but has **no "skip this child" hook**.

The risk this plan defends against: building the core on `SliverConstraints` (couples it to the render layer + sliver protocol, creates a dependency cycle, kills reuse), or settling for a next-frame-only build contract (permanently locks out true mid-pass build — the ossification trap the project exists to avoid).

---

## Stakeholder and Impact

- **`flui-rendering` slivers** — direct consumer; gains the adapter, the build contract, and the first lazy `SliverList`. The eager lists stay until the lazy list is benched as a win against them.
- **`LayoutContextApi` (all `RenderObject` authors)** — a breaking **addition** (the re-entrant build hook). Surface widens; gated by ARCH-GATE so a future session cannot silently reshape it away from mid-pass-capable.
- **`flui-view` lazy widgets (future)** — downstream consumer of the lazy render objects; they reach the core *through* `flui-rendering`'s slivers, not directly; out of scope here, recorded in U4.
- **General windowed UIs (future)** — virtualized text, data grid, table, timeline — the reason the core is kept agnostic; they are sliver-based and consume the core *through* `flui-rendering`'s slivers. A 2nd *direct* consumer of the core is the trigger to extract a `flui-virtualization` crate (recorded in U4).
- **Workspace** — **no new crate**; a new agnostic `virtualization` module is added inside `flui-rendering`, and `FenwickExtents` is **deleted** (zero callers — no other crate's import path changes), replaced by an in-module SumTree/augmented-B+-tree. `[workspace.members]` / `default-members` are unchanged (unless the U1 sub-decision sources the tree from an external crate — default is in-house, zero-dep).

---

## High-Level Technical Design

> Directional guidance for review, not implementation specification. The implementing agent treats it as context, not code to reproduce. The authoritative public surface is fixed at the API-GATE (U1) and ARCH-GATE (U2).

### The agnostic core (U1)

The `virtualization` module in `flui-rendering` is generic over a plain `ScrollWindow { offset, main_extent, cache_before, cache_after }` value type plus `f32` extents, and its **public surface names no render/sliver/protocol type** (so it stays extractable into a standalone crate later that would depend on `flui-types` / `flui-foundation` / `flui-geometry` only). Its backbone is a **focused augmented B+-tree (SumTree-style)**: each item is type-level `Unmeasured { hint: f32 } | Measured { extent: f32 }` (GPUI pattern — illegal states unrepresentable, not a side boolean); every internal node carries a `{ count, total_extent }` summary, giving `O(log n)` seek in **both** directions and `O(log n)` insert/delete. The `Virtualizer` surface (authoritative shape fixed at API-GATE):

- `new(item_count, default_estimate)`, `len`, `is_empty`.
- `set_count(n)` — insert/remove items, `O(log n)` via the tree (not an array shift — this is the capability Fenwick could not give).
- `set_measured(index, extent, anchor: (usize, f32)) -> Option<AnchorCorrection>` — replace an estimate with the real extent; returns a correction **iff** the change shifts content above the anchor item. Anchor is **item-identity** `(index, sub_offset)`, not raw pixel.
- `query(&ScrollWindow) -> VisibleRange` — `O(log n)`; returns the **dual** range (tight visible `[first, last)` + cache `[cache_first, cache_last)`) plus `leading_offset` (first item's offset minus `window.offset`, `≤ 0`), so callers prioritize render vs measure.
- `offset_of(index) -> f32`, `is_measured(index) -> bool`.
- `invalidate_from(index)` — watermark; extents after a structural change recompute lazily, `O(log n)` in the tree.
- `anchor_item() -> (usize, f32)` — getter so the consumer restores position after a layout invalidation.
- `total_extent() -> Extent` (`Extent = Exact(f32) | Estimated(f32)`), `measured_count()` / `estimated_count()` — scrollbar stability (Flutter #97676 = the average-based-jumpiness cautionary tale).
- `scroll_to_item(index, alignment)` — note the fixpoint-measure caveat when the target is unmeasured.
- estimate-for-unmeasured: the `Unmeasured { hint }` seed keeps total scroll extent and the scrollbar stable before every item is laid out (TanStack-style estimate-then-correct).

The core is **build-agnostic** — it never builds, lays out, or names a child render object. It is pure windowing arithmetic over indices and extents.

### The `SliverConstraints → ScrollWindow` adapter (U3, lives in `flui-rendering`, outside the agnostic module)

A thin function maps the sliver protocol's scroll/viewport fields onto `ScrollWindow` (offset, main-axis extent, leading/trailing cache). This is the *only* place the sliver protocol meets the core, and it sits **outside** the `virtualization` module so the module never sees `SliverConstraints`; the consumption arrow is intra-crate (`objects/slivers + adapter → virtualization module`) and would remain a clean downward arrow if the module is later extracted to `flui-virtualization`.

### The mid-pass-capable build contract (U2, on `LayoutContextApi`)

A new `LayoutContextApi` capability lets a lazy sliver, **during its own layout**, request materialization of a child by index/key and obtain its laid-out geometry. The **contract signature and semantics must permit true mid-pass re-entrant build** (build child *now*, get geometry back, decide next child) — Compose `SubcomposeLayout` capability — **without a later breaking change.** Borrow-safe by construction: a mid-pass-materialized child is reached through the same disjoint-subtree borrow primitive (U20/U20.1) the recursive layout walk already uses; mid-pass marks drain through the existing per-iteration side queue. v1 *backend* choice is stated explicitly in U2; **mid-pass is the target**, next-frame is the permitted stepping stone.

### The lazy `SliverList` consumer (U3)

Adapts `SliverConstraints → ScrollWindow`, asks the `Virtualizer` for the `VisibleRange` (dual visible/cache band), builds/lays-out **only** visible-plus-cache children (via the U2 contract), feeds measured extents back via `set_measured`, **accumulates** the signed `AnchorCorrection` and applies it, and **disposes children that leave the band** (Decision 3, behind a pluggable hook). **Correction policy lives here, not in the core:** suppress anchor correction while scrolling *backward* (the canonical "items jump while scrolling up" bug) and reset the correction accumulator on user-initiated scroll. Contrast with the eager `sliver_fixed_extent_list` / `sliver_fill_viewport` which lay out all `n`.

---

## Output Structure

New files / directories created during this work:

```
crates/flui-rendering/
├── src/
│   ├── virtualization/                    (NEW module: U1 — protocol-agnostic windowing math)
│   │   ├── mod.rs                         (ScrollWindow, VisibleRange, AnchorCorrection, Extent, Virtualizer)
│   │   └── sumtree.rs                     (NEW: U1 — focused augmented B+-tree over Unmeasured|Measured extents)
│   ├── slivers/
│   │   ├── mod.rs                         (MODIFY: U1 — drop the `fenwick` module decl/re-export)
│   │   └── fenwick.rs                     (DELETED — flat-array BIT, O(n) mid-list insert, zero callers)
│   ├── objects/
│   │   └── sliver_list_lazy.rs            (NEW: U3 — lazy, virtualized SliverList consumer)
│   └── (LayoutContextApi surface)         (MODIFY: U2 — mid-pass-capable build hook)
├── benches/
│   └── virtualizer.rs                     (NEW: U1 — criterion: O(log n) seek both ways + insert/delete vs naive O(n))
└── (render_viewport integration harness)  (MODIFY: U3 — synthetic-children + real-frame bench)
```

> No new workspace crate. A standalone `flui-virtualization` crate is a recorded **future** extraction (U4), triggered by a 2nd *direct* consumer; it is a cheap, non-breaking lift because the module is kept gate-enforced agnostic.

Per-unit `**Files:**` sections below are authoritative for what each unit creates or modifies.

---

## Implementation Units

> Each U-ID is stable; reordering or splitting does not renumber. Serial dependency: U1 → U2 → U3; U4 is recorded-future. Each unit ships as atomic commit(s) and must pass `cargo fmt` + `cargo clippy --all-targets --all-features -- -D warnings` + its stated tests before the next unit starts.

### U1 — `virtualization` module in `flui-rendering` + SumTree/B+-tree-backed `Virtualizer` core (delete `FenwickExtents`)

- **Goal**: create a new protocol-agnostic `virtualization` module **inside `flui-rendering`**, **DELETE `FenwickExtents`** (zero callers; flat-array BIT, `O(n)` mid-list insert — the structurally wrong tool), and build the agnostic `Virtualizer` over a **focused augmented B+-tree (SumTree-style)**: `O(log n)` seek both directions **and** `O(log n)` insert/delete, with type-level `Unmeasured | Measured` extents and item-identity anchoring. Verify in isolation.
- **Depends on**: none.
- **Files**:
  - `crates/flui-rendering/src/virtualization/mod.rs` (NEW — `ScrollWindow`, `VisibleRange` (dual visible+cache range + `leading_offset`), `AnchorCorrection` (signed delta), `Extent` (`Exact | Estimated`), `Virtualizer` with the full method set; public surface names no render/sliver/protocol type).
  - `crates/flui-rendering/src/virtualization/sumtree.rs` (NEW — the focused augmented B+-tree: `Unmeasured { hint } | Measured { extent }` items, `{ count, total_extent }` node summary, `O(log n)` seek-both-ways + `O(log n)` insert/delete; this is the deleted Fenwick's replacement and the bulk of the work). *Filename is illustrative; collapse into `mod.rs` or rename if the implementation prefers.*
  - `crates/flui-rendering/src/slivers/fenwick.rs` (**DELETED** — not relocated; structurally wrong, zero callers).
  - `crates/flui-rendering/src/slivers/mod.rs` (MODIFY — drop the `fenwick` module declaration/re-export; verify zero callers so nothing else breaks).
  - `crates/flui-rendering/src/lib.rs` (MODIFY — declare the new `virtualization` module).
  - `crates/flui-rendering/benches/virtualizer.rs` (NEW — criterion).
  - **No `Cargo.toml` / `[workspace.members]` change** — no new crate is created. (**U1 sub-decision:** if the SumTree/B+-tree is sourced from an existing crate rather than built in-house, that *would* touch `Cargo.toml` — see Approach; default is in-house, zero-dep.)
- **Approach**: **delete** the Fenwick file (it has **zero callers**, so the deletion is fully contained — no other crate's imports change). Build the `Virtualizer` on a **focused augmented B+-tree** as the backbone: a `{ count, total_extent }` summary at each node gives `O(log n)` offset→index and index→offset seek *and* `O(log n)` `set_count` insert/delete (the capability a flat-array Fenwick could not provide). Keep the tree *focused* (count + total-extent summaries), not a fully-generic `SumTree<T, Summary>` — generality lives at the `Virtualizer`'s public boundary, internals stay a deep module. **U1 sub-decision (record in the commit): build the tree in-house (lean — control + zero-dep) OR vet an existing crate via `/add-dep`** (RUSTSEC/license/MSRV/features pipeline) — default to in-house. Keep the public surface generic over `ScrollWindow` + `f32` extents — it must not name any render, sliver, or protocol type (this keeps the module cheaply extractable to a `flui-virtualization` crate later).
- **Patterns to follow**: GPUI/Zed `list.rs` (`SumTree<ListItem>`, `Unmeasured | Measured`, `{count, height}` summary, `ListOffset` item-identity anchor) for the structure; an existing self-contained `flui-rendering` module for module shape.
- **Test scenarios**:
  - Happy path: `query` returns the correct dual band (tight visible `[first, last)` + cache `[cache_first, cache_last)`) and `leading_offset` for a known extents vector + window.
  - **`O(log n)` seek BOTH directions** proven: offset→index (`query`) and index→offset (`offset_of`) (criterion bench + a test asserting sub-linear scaling).
  - **`O(log n)` insert/delete** proven: `set_count` inserting/removing items *mid-list* updates summaries in `O(log n)` (criterion bench vs a naive O(n) flat-array baseline — the explicit reason Fenwick was rejected).
  - Anchor-correction: `set_measured` with an extent differing from the estimate, for an item **above the anchor**, returns the signed `AnchorCorrection.delta` that keeps the anchored item stationary; a measure *equal* to the estimate, or a change *below* the anchor, returns `None`/zero.
  - Estimated→Exact transition: `total_extent()` reports `Estimated` while a prefix is unmeasured and flips to `Exact` once all items are `Measured`; `measured_count()`/`estimated_count()` track it.
  - Estimate-for-unmeasured: total scroll extent is stable and well-defined when only a prefix of items has been measured.
- **Verification**: `cargo build -p flui-rendering` exits 0 (incl. the new `virtualization` module and after the Fenwick deletion — proves zero-caller claim); `cargo test -p flui-rendering` (incl. the new `Virtualizer` + SumTree tests) exits 0; `cargo clippy -p flui-rendering --all-targets --all-features -- -D warnings` exits 0.
- **Gates**:
  - **API-GATE** — the **module's** public surface (`ScrollWindow`, `VisibleRange`, `AnchorCorrection`, `Extent`, `Virtualizer` method signatures) is reviewed and fixed here. Invariant to enforce: **no render / sliver / protocol type appears in the module's public API** (keeps the core agnostic, keeps it cheaply crate-extractable later, and keeps the future crate-extraction acyclic).
- **Acceptance**: `O(log n)` seek **both directions** proven; `O(log n)` insert/delete (`set_count` mid-list) proven against a naive O(n) baseline; anchor-correction keeps content stationary when an above-anchor estimate is corrected; Estimated→Exact transition tested; `FenwickExtents` deleted; the `virtualization` module's public surface is agnostic (no render/sliver/protocol type) — no new crate is created.

### U2 — Mid-pass-capable re-entrant build contract on `LayoutContextApi`

- **Goal**: add the new `LayoutContextApi` capability by which a lazy sliver requests on-demand child materialization **during its own layout**, with a **contract shaped to permit true mid-pass re-entrant build without a later breaking change**; implement the v1 mechanism behind it.
- **Depends on**: U1 (core exists; the consumer in U3 will use both).
- **Files**:
  - `crates/flui-rendering/src/` `LayoutContextApi` definition + the box/sliver layout-context impls that realize it (MODIFY — exact paths fixed during scout-at-implementation; the trait and its concrete contexts).
  - `crates/flui-rendering/src/pipeline/deferred.rs` and/or `crates/flui-rendering/src/pipeline/owner.rs` (MODIFY only if the chosen v1 backend wires through the deferred queue / mid-pass marks).
- **Approach & the explicit v1 choice**: state the v1 backend choice plainly in the commit and in code comments. **Target = true mid-pass.** **Permitted v1 stepping stone = the existing next-frame deferred-mutation queue (`apply_deferred_mutation`) + cache-region prefetch** (build the cache band ahead of need so the one-frame insertion latency is hidden by the buffer — RecyclerView/Flutter-style). The **recorded choice for U2** is: *ship the contract mid-pass-shaped; implement the v1 backend as `<next-frame+prefetch>` OR `<true mid-pass>` — whichever the implementing session selects — and label it honestly.* If next-frame is chosen for v1, **true mid-pass remains a planned unit** (it becomes a follow-up within U4's recorded scope or its own unit), never abandoned. Borrow-safety: a mid-pass-materialized child is reached through the U20/U20.1 disjoint-subtree primitive; mid-pass marks drain via the existing per-iteration side queue — **no new aliasing hazard may be introduced.**
- **Patterns to follow**: the U20/U20.1 disjoint-borrow walk and the deferred-queue mid-pass-marks drain already in `pipeline/owner.rs`; Compose `SubcomposeLayout` for the *capability shape* (build-then-measure-then-decide), Flutter `invokeLayoutCallback` as the **anti-pattern** (unsafe re-entrancy) to *not* reproduce.
- **Test scenarios**:
  - Happy path: a test layout object requests a child mid-layout via the hook and receives a usable (laid-out, for mid-pass) or scheduled (for next-frame) child handle per the chosen backend.
  - Borrow-safety: the hook does not produce overlapping `&mut` to the arena — exercised under the existing test harness (and Miri if the walk is touched, consistent with prior render-pipeline units).
  - Re-entrancy guard: requesting a child does not corrupt the dirty queues; mid-pass marks drain in the same pass.
- **Verification**: `cargo build -p flui-rendering` exits 0; `cargo test -p flui-rendering` exits 0; `cargo clippy -p flui-rendering --all-targets --all-features -- -D warnings` exits 0.
- **Gates**:
  - **ARCH-GATE (this unit's contract)** — the `LayoutContextApi` hook's **signature and semantics** are reviewed against the ADR invariant: *the contract must permit true mid-pass re-entrant build without a future breaking change.* A next-frame-*only* contract shape is a gate failure even if the v1 backend is next-frame. Borrow-safety of the mechanism is part of this gate.
- **Acceptance**: the breaking `LayoutContextApi` addition is mid-pass-capable by contract; the v1 backend choice is recorded and honestly labeled; borrow-safety holds; if v1 is next-frame, true mid-pass is recorded as a planned unit.

### U3 — Lazy `SliverList` consumer

- **Goal**: a lazy, virtualized `SliverList` that adapts `SliverConstraints → ScrollWindow`, uses the `Virtualizer` for the visible range, builds/lays-out **only** visible-plus-cache children via the U2 hook, disposes children on scroll-off (behind a pluggable hook), and is proven a win against the eager list.
- **Depends on**: U1 (core) + U2 (build contract).
- **Files**:
  - `crates/flui-rendering/src/objects/sliver_list_lazy.rs` (NEW — the lazy consumer).
  - `crates/flui-rendering/src/objects/mod.rs` (MODIFY — declare/export the new object).
  - `crates/flui-rendering/src/objects/viewport.rs` (MODIFY only if a "skip this child" hook is needed on `layout_child_sequence` / `try_cached_sliver_geometry` to let the lazy list lay out a sub-range — see `:325` / `:612`).
  - `render_viewport` integration harness (MODIFY — synthetic children fixture + a real-frame criterion bench vs the eager `sliver_fixed_extent_list` / `sliver_fill_viewport`).
- **Approach**: build the adapter, drive the `Virtualizer`, materialize only the visible+cache band through the U2 contract, feed measured extents back via `set_measured`, **accumulate and apply the signed `AnchorCorrection`**, and dispose on scroll-off via the pluggable hook (Decision 3 — **not** a two-level pool). **Correction policy is the consumer's job (the core only emits the signed delta):** *suppress anchor correction while scrolling backward* — applying corrections during an upward scroll is the canonical "items jump while scrolling up" bug — and *reset the correction accumulator on user-initiated scroll*. Keep the eager lists in place; this unit *adds* the lazy one and proves it.
- **Patterns to follow**: the eager `sliver_fixed_extent_list.rs` / `sliver_fill_viewport.rs` for the sliver-object scaffolding; `viewport.rs` `layout_child_sequence` for the child-driving shape it replaces with a windowed sub-range.
- **Test scenarios**:
  - Happy path (harness): with N synthetic children and a viewport showing K, only K + cache children are built/laid-out (assert built-count ≪ N).
  - Scroll: scrolling shifts the built band; children leaving the band are disposed (pluggable hook fires); newly-visible children materialize.
  - Anchor on resize: a child whose measured extent differs from its estimate triggers an `AnchorCorrection` that keeps on-screen content stationary (the Flutter `RenderSliverList` weakness being beaten).
  - Backward-scroll suppression: while scrolling *up*, an above-viewport re-measure does **not** apply a correction (content does not jump up — the canonical bug); the accumulator resets on a user-initiated scroll.
  - Bench: real-frame criterion vs the eager list shows the virtualized list does asymptotically less work as N grows.
- **Verification**: `cargo build -p flui-rendering` exits 0; `cargo test -p flui-rendering` (incl. the `render_viewport` harness) exits 0; `cargo clippy -p flui-rendering --all-targets --all-features -- -D warnings` exits 0; the real-frame bench runs and reports built-count + timing vs eager.
- **Gates**:
  - **QA (via the harness)** — correctness is demonstrated through the existing `render_viewport` integration harness with synthetic children **plus** the real-frame bench against the eager list. **No `flui-view` consumer required.** Honest labeling: the bench reports the actual backend (next-frame vs mid-pass) and must not overstate the win.
- **Acceptance**: only visible-plus-cache children are built/laid-out; dispose-on-scroll-off works behind the pluggable hook; anchor correction keeps content stationary on extent change; backward-scroll correction is suppressed and the accumulator resets on user-initiated scroll; the real-frame bench shows a win over the eager list.

### U4 — Future, recorded (not in this delivery)

Recorded so a future session continues the exact arc rather than re-litigating it:

- **Grid / staggered consumers** — additional `flui-rendering` consumers of the same agnostic `Virtualizer` (2-D / variable-span windowing on top of the same core). These live *in* `flui-rendering`, so they are not a *second crate*-level consumer; they keep the core a single-crate module.
- **`flui-view` lazy widgets** — the widget-layer consumers of the lazy render objects (the third layer in the ADR's layering diagram). They reach the core *through* `flui-rendering`'s slivers, so they are **not a direct consumer** of the core and do **not** trigger crate-extraction on their own.
- **Crate-extraction to `flui-virtualization`** — lift the `virtualization` module into a standalone crate **when a 2nd *direct* consumer of the core appears** (a crate other than `flui-rendering` that wants the windowing math directly). This is a **mechanical, non-breaking** lift precisely because the module is gate-enforced agnostic (no render/sliver/protocol type in its `pub` surface): move the files, add the crate metadata + `[workspace.members]` entry, depend on `flui-types` / `flui-foundation` / `flui-geometry`, and re-point `flui-rendering` at the crate. Until that 2nd direct consumer exists, a crate is premature decomposition (one-consumer boundary cost not paid back).
- **Recycling (pooling/reuse)** — added **only if a real-frame benchmark proves the need**, swapped in behind the U3 pluggable hook. Explicitly **not** RecyclerView's two-level pool by default (justifying constraint absent for a Rust arena + `can_update`).
- **Gates**: API-GATE for any new public surface; ARCH-GATE if the build contract is touched; QA via the harness (and `flui-view` examples once that layer consumes it).

---

## Risks and Mitigations

- **Risk: the core gets coupled to the sliver protocol.** A `SliverConstraints` (or any render/sliver/protocol type) leaking into the `virtualization` **module's** public surface kills reuse, blocks the cheap future crate-extraction, and would create a dependency cycle if extracted. **Mitigation:** API-GATE on U1 explicitly rejects any such type in the module's public API; the adapter lives on the `flui-rendering` side, outside the module.
- **Risk: the build contract ships next-frame-*only*.** That permanently locks out true mid-pass — the ossification trap. **Mitigation:** ARCH-GATE on U2 reviews the contract *shape* (not just the v1 backend) for mid-pass capability; a next-frame-only contract is a gate failure even when the v1 backend is next-frame.
- **Risk: false performance claims.** Reporting a next-frame backend's bench as if it were mid-pass, or asserting a virtualization win without measuring against the eager baseline. **Mitigation:** honest scaffold-vs-wired labeling is required (ADR Consequences); U3's QA gate mandates the real-frame bench vs eager and accurate backend labeling.
- **Risk: a borrow-safety hazard in the mid-pass hook.** On-demand build during layout could create overlapping `&mut` to the arena. **Mitigation:** the mechanism reuses the U20/U20.1 disjoint-subtree primitive and the existing mid-pass-marks drain; U2's tests (and Miri where the walk is touched) cover it.

---

## Definition of Done (this delivery: U1–U3)

- The agnostic `virtualization` module exists **inside `flui-rendering`** (no new crate), is backed by a focused SumTree/augmented-B+-tree (`FenwickExtents` deleted, zero callers), keeps its public surface free of render/sliver/protocol types, and its `Virtualizer` proves `O(log n)` seek both directions + `O(log n)` insert/delete + anchor correction in isolation (U1, API-GATE passed).
- `LayoutContextApi` carries a mid-pass-capable on-demand build hook; the v1 backend is implemented and honestly labeled; borrow-safety holds (U2, ARCH-GATE passed).
- A lazy `SliverList` builds only visible-plus-cache children, disposes on scroll-off behind a pluggable hook, corrects the anchor on extent change, and beats the eager list on a real-frame bench via the `render_viewport` harness — no `flui-view` consumer required (U3, QA passed).
- `cargo fmt`, `cargo clippy --all-targets --all-features -- -D warnings`, and the per-unit tests are green across `flui-rendering` (which now hosts the `virtualization` module).
- U4 is recorded for the next session.
