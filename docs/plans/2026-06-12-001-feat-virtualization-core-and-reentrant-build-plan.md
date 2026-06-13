---
date: 2026-06-12
title: "feat: Virtualization core + re-entrant build foundation"
type: feat
status: active
adr: docs/adr/ADR-0003-virtualization-core-and-reentrant-build.md
---

# feat: Virtualization core + re-entrant build foundation

## Summary

Stand up a new **protocol-agnostic `flui-virtualization` crate** that owns windowing math (visible-range query, estimate-for-unmeasured, anchor correction on measured-extent change), move the correct-but-uncalled `FenwickExtents` into it, then make `flui-rendering` a *consumer*: a `SliverConstraints → ScrollWindow` adapter, a **mid-pass-capable on-demand child-build contract** on `LayoutContextApi`, and a lazy `SliverList` that builds/lays-out only the visible-plus-cache children. Binding architecture: [`docs/adr/ADR-0003-virtualization-core-and-reentrant-build.md`](../adr/ADR-0003-virtualization-core-and-reentrant-build.md).

Delivered as **4 units** (U1 → U2 → U3, plus U4 recorded-future). The two load-bearing invariants the ADR locks in — **(1)** the core is agnostic (no render/sliver/protocol type), and **(2)** the build contract is mid-pass-capable from day one (true mid-pass never locked out, even if v1 ships a next-frame backend) — are the gates this plan must not silently relax.

---

## Problem Frame

flui's virtualization substrate is half-present and mis-shaped (verified via scout, 2026-06-12):

- `FenwickExtents` (`crates/flui-rendering/src/slivers/fenwick.rs`) — the hard, correct, ASM-verified `O(log n)` prefix-sum backbone — is **self-contained (`Vec<f32>`-only) but has zero callers** and lives in the wrong crate.
- `SliverConstraints` / `SliverGeometry` are tightly bound to `SliverProtocol` and the viewport walk — *not* a neutral windowing value type.
- The deferred-mutation queue (`crates/flui-rendering/src/pipeline/deferred.rs` + `PipelineOwner::apply_deferred_mutation` at `crates/flui-rendering/src/pipeline/owner.rs:2021`, drained at end of `run_layout`) is fully wired but is **next-frame** materialization, not mid-pass re-entrancy.
- Existing sliver lists (`crates/flui-rendering/src/objects/sliver_fixed_extent_list.rs`, `.../sliver_fill_viewport.rs`) lay out **all** children eagerly `O(n)`.
- The viewport (`crates/flui-rendering/src/objects/viewport.rs`, `layout_child_sequence` at `:325`, `try_cached_sliver_geometry` at `:612`) drives children eagerly with a per-child constraint cache but has **no "skip this child" hook**.

The risk this plan defends against: building the core on `SliverConstraints` (couples it to the render layer + sliver protocol, creates a dependency cycle, kills reuse), or settling for a next-frame-only build contract (permanently locks out true mid-pass build — the ossification trap the project exists to avoid).

---

## Stakeholder and Impact

- **`flui-rendering` slivers** — direct consumer; gains the adapter, the build contract, and the first lazy `SliverList`. The eager lists stay until the lazy list is benched as a win against them.
- **`LayoutContextApi` (all `RenderObject` authors)** — a breaking **addition** (the re-entrant build hook). Surface widens; gated by ARCH-GATE so a future session cannot silently reshape it away from mid-pass-capable.
- **`flui-view` lazy widgets (future)** — downstream consumer of the lazy render objects; out of scope here, recorded in U4.
- **General windowed UIs (future)** — virtualized text, data grid, table, timeline — the reason the core is agnostic; they consume `flui-virtualization` directly.
- **Workspace** — one new crate (`flui-virtualization`) added to `[workspace.members]`/`default-members`; one breaking import-path move (`FenwickExtents`, zero callers).

---

## High-Level Technical Design

> Directional guidance for review, not implementation specification. The implementing agent treats it as context, not code to reproduce. The authoritative public surface is fixed at the API-GATE (U1) and ARCH-GATE (U2).

### The agnostic core (U1)

`flui-virtualization` is generic over a plain `ScrollWindow { offset, main_extent, cache_before, cache_after }` value type plus `f32` extents, and depends on `flui-types` / `flui-foundation` / `flui-geometry` **only**. The `Virtualizer` answers two questions and nothing else:

- `window_query(window: ScrollWindow) -> VisibleRange` — given the scroll window and the current extents (Fenwick prefix-sum of measured extents, falling back to an estimate for unmeasured indices), return the `[first, last]` item band covering visible + `cache_before`/`cache_after`, plus the leading edge of `first`. `O(log n)` offset↔index via the moved `FenwickExtents`.
- `update_measured(index, extent) -> AnchorCorrection` — record a newly-measured item extent; if it differs from the estimate previously used at or before the anchor, return the `delta` the consumer applies to the scroll offset so on-screen content does not jump.
- estimate-for-unmeasured: a single average/seed extent used for indices not yet measured, so total scroll extent and the scrollbar are stable before every item is laid out (TanStack-style estimate-then-correct).

The core is **build-agnostic** — it never builds, lays out, or names a child render object. It is pure windowing arithmetic over indices and extents.

### The `SliverConstraints → ScrollWindow` adapter (U3, lives in `flui-rendering`)

A thin function maps the sliver protocol's scroll/viewport fields onto `ScrollWindow` (offset, main-axis extent, leading/trailing cache). This is the *only* place the sliver protocol meets the core; the dependency arrow points `flui-rendering → flui-virtualization`.

### The mid-pass-capable build contract (U2, on `LayoutContextApi`)

A new `LayoutContextApi` capability lets a lazy sliver, **during its own layout**, request materialization of a child by index/key and obtain its laid-out geometry. The **contract signature and semantics must permit true mid-pass re-entrant build** (build child *now*, get geometry back, decide next child) — Compose `SubcomposeLayout` capability — **without a later breaking change.** Borrow-safe by construction: a mid-pass-materialized child is reached through the same disjoint-subtree borrow primitive (U20/U20.1) the recursive layout walk already uses; mid-pass marks drain through the existing per-iteration side queue. v1 *backend* choice is stated explicitly in U2; **mid-pass is the target**, next-frame is the permitted stepping stone.

### The lazy `SliverList` consumer (U3)

Adapts `SliverConstraints → ScrollWindow`, asks the `Virtualizer` for the `VisibleRange`, builds/lays-out **only** visible-plus-cache children (via the U2 contract), feeds measured extents back via `update_measured`, applies any `AnchorCorrection`, and **disposes children that leave the band** (Decision 3, behind a pluggable hook). Contrast with the eager `sliver_fixed_extent_list` / `sliver_fill_viewport` which lay out all `n`.

---

## Output Structure

New files / directories created during this work:

```
crates/
└── flui-virtualization/                  (NEW crate: U1)
    ├── Cargo.toml                         (deps: flui-types, flui-foundation, flui-geometry only)
    ├── benches/
    │   └── virtualizer.rs                 (criterion: O(log n) offset↔index, anchor-correction)
    └── src/
        ├── lib.rs                         (ScrollWindow, VisibleRange, AnchorCorrection, Virtualizer)
        └── fenwick.rs                     (MOVED from crates/flui-rendering/src/slivers/fenwick.rs)

crates/flui-rendering/
├── src/
│   ├── slivers/
│   │   └── fenwick.rs                     (DELETED — moved to flui-virtualization in U1)
│   ├── objects/
│   │   └── sliver_list_lazy.rs            (NEW: U3 — lazy, virtualized SliverList consumer)
│   └── (LayoutContextApi surface)         (MODIFY: U2 — mid-pass-capable build hook)
└── (render_viewport integration harness)  (MODIFY: U3 — synthetic-children + real-frame bench)
```

Per-unit `**Files:**` sections below are authoritative for what each unit creates or modifies.

---

## Implementation Units

> Each U-ID is stable; reordering or splitting does not renumber. Serial dependency: U1 → U2 → U3; U4 is recorded-future. Each unit ships as atomic commit(s) and must pass `cargo fmt` + `cargo clippy --all-targets --all-features -- -D warnings` + its stated tests before the next unit starts.

### U1 — `flui-virtualization` crate + protocol-agnostic `Virtualizer` core

- **Goal**: create the new crate, move `FenwickExtents` into it, and build the agnostic `Virtualizer` (`ScrollWindow`, `VisibleRange`, `window_query`; `update_measured → AnchorCorrection`; estimate-for-unmeasured). Verify in isolation.
- **Depends on**: none.
- **Files**:
  - `crates/flui-virtualization/Cargo.toml` (NEW — depends on `flui-types`, `flui-foundation`, `flui-geometry` **only**; standard Layer-0/Core.1 metadata + lints mirroring `crates/flui-geometry/Cargo.toml`).
  - `crates/flui-virtualization/src/lib.rs` (NEW — `ScrollWindow`, `VisibleRange`, `AnchorCorrection`, `Virtualizer` + `window_query` / `update_measured`).
  - `crates/flui-virtualization/src/fenwick.rs` (NEW — `git mv` of `crates/flui-rendering/src/slivers/fenwick.rs`; update internal `use crate::` paths; preserve the existing ASM-verified Fenwick tests).
  - `crates/flui-rendering/src/slivers/fenwick.rs` (DELETE — moved).
  - `crates/flui-rendering/src/slivers/mod.rs` (MODIFY — drop the `fenwick` module declaration/re-export; verify zero callers so nothing else breaks).
  - `crates/flui-virtualization/benches/virtualizer.rs` (NEW — criterion).
  - `Cargo.toml` (MODIFY — add `"crates/flui-virtualization"` to `[workspace.members]` + `default-members`).
- **Approach**: `git mv` the Fenwick file to preserve history; the move is breaking but has **zero callers**, so it is contained. Build the `Virtualizer` on top of the moved Fenwick as the prefix-sum backbone. Keep the surface generic over `ScrollWindow` + `f32` extents — it must not name any render, sliver, or protocol type.
- **Patterns to follow**: `crates/flui-geometry/Cargo.toml` for crate shape (low-level crate, no `flui-rendering` dep); the existing Fenwick tests for the `O(log n)` assertions.
- **Test scenarios**:
  - Happy path: `window_query` returns the correct `[first, last]` band for a known extents vector + window, including the cache buffer.
  - `O(log n)` offset↔index proven (criterion bench + a test asserting seek cost scales sub-linearly / matches the Fenwick contract).
  - Anchor-correction: feeding a measured extent that differs from its estimate produces the `AnchorCorrection.delta` that keeps the anchored item stationary; a measure *equal* to the estimate produces zero delta.
  - Estimate-for-unmeasured: total scroll extent is stable and well-defined when only a prefix of items has been measured.
- **Verification**: `cargo build -p flui-virtualization` exits 0; `cargo test -p flui-virtualization` exits 0; `cargo clippy -p flui-virtualization --all-targets --all-features -- -D warnings` exits 0; `cargo build -p flui-rendering` still exits 0 after the Fenwick move (proves zero-caller claim).
- **Gates**:
  - **API-GATE** — the crate's public surface (`ScrollWindow`, `VisibleRange`, `AnchorCorrection`, `Virtualizer` method signatures) is reviewed and fixed here. Invariant to enforce: **no render / sliver / protocol type appears in the public API** (keeps the core agnostic + acyclic).
- **Acceptance**: `O(log n)` offset↔index proven; anchor-correction on extent change tested; crate depends on the three foundation crates only.

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
- **Approach**: build the adapter, drive the `Virtualizer`, materialize only the visible+cache band through the U2 contract, feed measured extents back via `update_measured`, apply `AnchorCorrection`, and dispose on scroll-off via the pluggable hook (Decision 3 — **not** a two-level pool). Keep the eager lists in place; this unit *adds* the lazy one and proves it.
- **Patterns to follow**: the eager `sliver_fixed_extent_list.rs` / `sliver_fill_viewport.rs` for the sliver-object scaffolding; `viewport.rs` `layout_child_sequence` for the child-driving shape it replaces with a windowed sub-range.
- **Test scenarios**:
  - Happy path (harness): with N synthetic children and a viewport showing K, only K + cache children are built/laid-out (assert built-count ≪ N).
  - Scroll: scrolling shifts the built band; children leaving the band are disposed (pluggable hook fires); newly-visible children materialize.
  - Anchor on resize: a child whose measured extent differs from its estimate triggers an `AnchorCorrection` that keeps on-screen content stationary (the Flutter `RenderSliverList` weakness being beaten).
  - Bench: real-frame criterion vs the eager list shows the virtualized list does asymptotically less work as N grows.
- **Verification**: `cargo build -p flui-rendering` exits 0; `cargo test -p flui-rendering` (incl. the `render_viewport` harness) exits 0; `cargo clippy -p flui-rendering --all-targets --all-features -- -D warnings` exits 0; the real-frame bench runs and reports built-count + timing vs eager.
- **Gates**:
  - **QA (via the harness)** — correctness is demonstrated through the existing `render_viewport` integration harness with synthetic children **plus** the real-frame bench against the eager list. **No `flui-view` consumer required.** Honest labeling: the bench reports the actual backend (next-frame vs mid-pass) and must not overstate the win.
- **Acceptance**: only visible-plus-cache children are built/laid-out; dispose-on-scroll-off works behind the pluggable hook; anchor correction keeps content stationary on extent change; the real-frame bench shows a win over the eager list.

### U4 — Future, recorded (not in this delivery)

Recorded so a future session continues the exact arc rather than re-litigating it:

- **Grid / staggered consumers** — additional `flui-rendering` consumers of the same agnostic `Virtualizer` (2-D / variable-span windowing on top of the same core).
- **`flui-view` lazy widgets** — the widget-layer consumers of the lazy render objects (the third layer in the ADR's layering diagram).
- **True mid-pass build** — if U2's v1 backend shipped next-frame, the true-mid-pass implementation is finished here (the contract already permits it; only the mechanism changes). **Not abandoned.**
- **Recycling (pooling/reuse)** — added **only if a real-frame benchmark proves the need**, swapped in behind the U3 pluggable hook. Explicitly **not** RecyclerView's two-level pool by default (justifying constraint absent for a Rust arena + `can_update`).
- **Gates**: API-GATE for any new public surface; ARCH-GATE if the build contract is touched; QA via the harness (and `flui-view` examples once that layer consumes it).

---

## Risks and Mitigations

- **Risk: the core gets coupled to the sliver protocol.** A `SliverConstraints` (or any render/sliver/protocol type) leaking into `flui-virtualization`'s public surface kills reuse and creates a dependency cycle. **Mitigation:** API-GATE on U1 explicitly rejects any such type in the public API; the adapter lives on the `flui-rendering` side.
- **Risk: the build contract ships next-frame-*only*.** That permanently locks out true mid-pass — the ossification trap. **Mitigation:** ARCH-GATE on U2 reviews the contract *shape* (not just the v1 backend) for mid-pass capability; a next-frame-only contract is a gate failure even when the v1 backend is next-frame.
- **Risk: false performance claims.** Reporting a next-frame backend's bench as if it were mid-pass, or asserting a virtualization win without measuring against the eager baseline. **Mitigation:** honest scaffold-vs-wired labeling is required (ADR Consequences); U3's QA gate mandates the real-frame bench vs eager and accurate backend labeling.
- **Risk: a borrow-safety hazard in the mid-pass hook.** On-demand build during layout could create overlapping `&mut` to the arena. **Mitigation:** the mechanism reuses the U20/U20.1 disjoint-subtree primitive and the existing mid-pass-marks drain; U2's tests (and Miri where the walk is touched) cover it.

---

## Definition of Done (this delivery: U1–U3)

- `flui-virtualization` exists, depends on `flui-types` / `flui-foundation` / `flui-geometry` only, hosts the moved `FenwickExtents`, and its `Virtualizer` proves `O(log n)` offset↔index + anchor correction in isolation (U1, API-GATE passed).
- `LayoutContextApi` carries a mid-pass-capable on-demand build hook; the v1 backend is implemented and honestly labeled; borrow-safety holds (U2, ARCH-GATE passed).
- A lazy `SliverList` builds only visible-plus-cache children, disposes on scroll-off behind a pluggable hook, corrects the anchor on extent change, and beats the eager list on a real-frame bench via the `render_viewport` harness — no `flui-view` consumer required (U3, QA passed).
- `cargo fmt`, `cargo clippy --all-targets --all-features -- -D warnings`, and the per-unit tests are green across `flui-virtualization` and `flui-rendering`.
- U4 is recorded for the next session.
