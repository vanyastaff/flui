[← Roadmap](ROADMAP.md) · [Foundations](FOUNDATIONS.md) · [Port Methodology](PORT.md) · [Back to README](../README.md)

# FLUI Roadmap Tracker

> Live execution checklist for [`docs/ROADMAP.md`](ROADMAP.md). The roadmap is the *plan*; this file is the *queue* — every deliverable broken into a row with a status, an objective gate, a link to the SDD/plan artifact that owns it, and any hard sequence constraints.

**This file is the single source of truth for "what's in flight, what's done, what's next."** Update it in the `archive` phase of every SDD change, or when a Mythos receipts/plan transitions state. Do not let it drift — a stale tracker is worse than no tracker.

---

## How to use this file

- **One row = one piece of objectively verifiable work** with an exit gate copied from ROADMAP.md.
- **Status legend:**
  - `☐ todo` — not started
  - `◐ in-progress` — SDD change / plan / Mythos cycle is actively open; row links to it
  - `✓ done` — exit gate is verified green (test passes, command exits 0, file exists)
  - `⚠ verify` — believed done but unconfirmed; **first action** for a row in this state is an audit, not new work
  - `🛇 blocked` — cannot start; row lists what unblocks it
- **Owner link** points to the canonical artifact: `specs/NNN-*`, `docs/plans/YYYY-MM-DD-*`, `docs/research/YYYY-MM-DD-*`, or `openspec/changes/<name>/`.
- **Update protocol:**
  1. Moving a row to `◐ in-progress` requires linking the SDD change or plan that owns it.
  2. Moving a row to `✓ done` requires the exit gate command output or test path in the row's notes.
  3. Adding a new row to Core.0 requires a roadmap amendment — Core.0 scope is locked by ROADMAP.md.
- **Parity scoreboard** lives in [`ROADMAP.md`](ROADMAP.md#the-destination), not here. This file tracks **construction**, not parity.

---

## Hard sequence constraints (must hold before queuing)

These are written into ROADMAP.md and are non-negotiable — violating them creates merge conflicts or contract-flaw rework:

1. **Cycle 5 closure → Phase 0 of `specs/004-view-element-core`.** Both touch `crates/flui-view/src/element/` and `crates/flui-view/src/tree/`. Cycle 5 must hand off the files before Phase 0 starts.
2. **Core.0 exit → Core.1 entry.** Core.1's vertical slice depends on locked contracts (C2 / C3 / C4+C6) and the wired layout/composite/paint pipeline.
3. **Cross.H D-9 (`BuildContext.new_minimal` hole) → Catalog.1.** Material #1 is an `InheritedWidget` consumer.
4. **Cross.H D-7 (layer lifecycle protocol) → App.1.**
5. **Cross.P mobile backends → Cross.D `flui-build`.** Build pipeline targets cannot ship without the platform backends.
6. **Widget → render-object map (Core.0 deliverable) → Core.2 entry.** Cannot start render-object catalog without knowing what `flui-widgets` will demand.

---

## Core.0 — Spine to target spec  *(current phase)*

### NEW work — unowned by any prior Mythos plan

| # | Deliverable | Status | Owner artifact | Exit gate (from ROADMAP) |
|---|---|---|---|---|
| N1 | **D-1 layout phase wired** — `layout_node_with_children` invokes per-node `RenderEntry::layout` with constraints propagated parent→child | ◐ in-progress | [`plans/2026-05-23-001-feat-pipeline-wiring-d-block-plan.md`](plans/2026-05-23-001-feat-pipeline-wiring-d-block-plan.md) | Integration test: `Padding → Center → ColoredBox` 3-level tree lays out with correct constraints and sizes |
| N2 | **D-3 `run_compositing`** — subtree compositing-bits walk implemented | ◐ in-progress | same plan as N1 | Integration test: layer subtree marked dirty triggers compositing-bits propagation |
| N3 | **D-4 `run_paint` dirty-flag fix** — clear `needs_paint` only on nodes actually painted | ◐ in-progress | same plan as N1 | Integration test: `RepaintBoundary`-isolated repaint clears `needs_paint` only on painted nodes |
| N4 | **D-2 keyed reconciliation** — `key: Option<Key>` on `ElementNode`; route variable-arity reconciliation through keyed algorithm; delete positional path | ☐ todo | (folded into `specs/004-view-element-core` Phase 1/2) | Integration test: `[A(key=1), B(key=2)]` reordered to `[B, A]` preserves element identity (no remount) |
| N5 | **Unified contracts spec — `specs/004-view-element-core`** covering C2 (heterogeneous children), C3 (widget-authoring API), C4+C6 (View trait / element storage / keyed reconciliation) | ◐ in-progress | [`specs/004-view-element-core/spec.md`](../specs/004-view-element-core/spec.md), [`plans/2026-05-22-005-feat-view-element-core-contracts-plan.md`](plans/2026-05-22-005-feat-view-element-core-contracts-plan.md) | 4-PR sequence (Phase 0 benchmarks → 1 storage → 2 reconciler → 3 IntoView) each merged with green gates |
| N5.0 | ↳ Phase 0 — 3-day spec-validation benchmarks (S1 KeyId interning + S2 static-path sketch) | 🛇 blocked | gated by Cycle 5 closure (M3) | Benchmark report; if FR-022 / FR-016 invert, Phase 1 re-opens |
| N5.1 | ↳ Phase 1 — storage shape + key field + self-validation round-trip tests | ☐ todo | (Phase 0 first) | `cargo test -p flui-view` green; element storage layout matches spec |
| N5.2 | ↳ Phase 2 — keyed reconciler completion + `ElementCore` rewiring + `ReconcileEvent` trace stream | ☐ todo | (Phase 1 first) | Variable-arity reconciliation passes keyed and positional fallback tests |
| N5.3 | ↳ Phase 3 — `IntoView` surface + `downcast_ref` elimination + derive macros + port-check triggers | ☐ todo | (Phase 2 first) | `downcast_ref` count = 0 in framework code; derive macros produce expected impls |
| N6 | **Refusal triggers #8–#13** installed in [`PORT.md`](PORT.md); mechanically-detectable ones become `port-check.sh` gates | ⚠ verify | [`scripts/port-check.sh`](../scripts/port-check.sh) (triggers 8–13 are present in script) | `bash scripts/port-check.sh -v` exits 0 with all 13 triggers reporting green; PORT.md cross-reference confirms #8–#13 documented |
| N7 | **Merge `flui-log` → `flui-foundation`** | ✓ done | n/a (crate removed) | `crates/flui-log` absent; no `flui-log` workspace member; log helpers live in `flui-foundation` |
| N8 | **Split `flui-geometry` out of `flui-types`** | ✓ done | n/a (crate exists) | `flui-geometry` present in `crates/` and `[workspace.members]` |
| N9 | **Constitution layer table + edition/Rust-version line amended** | ⚠ verify | check [`FOUNDATIONS.md`](FOUNDATIONS.md) Part IV vs current `Cargo.toml` (`edition = "2024"`, `rust-version = "1.95"`) | Constitution version bump recorded; layer table matches FOUNDATIONS Part IV |
| N10 | **`RasterBackend` seam** in `flui-engine` (lyon stays as default implementation; future Vello swap non-breaking) | ☐ todo | — | Trait + lyon adapter compiled into `flui-engine`; engine can swap implementation via a single type parameter or factory |
| N11 | **Freeze `Scene` / `DrawCommand` contract** | ☐ todo | — | Contract documented in `docs/designs/`; CI guard: any change to the type surface requires a coordinated cross-track change note |
| N12 | **Widget → render-object mapping checklist** at `docs/research/widget-renderobject-map.md` | ☐ todo | — | File exists; every planned `flui-widgets` widget maps to its render object; **gates Core.2 entry (R2)** |
| N13 | **`flui-types/src/physics/` parity audit** vs Flutter `physics` package (Spring / Friction / Gravity) | ☐ todo | — | Audit report at `docs/research/YYYY-MM-DD-physics-parity-audit.md`; all Spring/Friction/Gravity behavior tests pass against `.flutter/` |
| N14 | **Zero `unimplemented!()` / `todo!()` in non-test code** (CI grep gate) | 🛇 blocked | current count: **42** non-test occurrences (grep result, 2026-05-24) | CI grep gate exits 0; no `unimplemented!()`/`todo!()` outside `tests/`/`#[cfg(test)]` |
| N15 | **`cargo build --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` exit 0** (Core.0 final gate) | ⚠ verify | run `just ci` | Both commands exit 0 against current `main` |

### N-geom — `flui-geometry` polish pass + math-stack reconciliation

**Owner research:** [`research/2026-05-24-flui-geometry-polish-pass-research.md`](research/2026-05-24-flui-geometry-polish-pass-research.md) (538 lines).
**Block intent:** close escape hatches in the unit system and reconcile the documented "own everything" stance with the actual `glam`/`mint` Cargo.toml integration. **Final sequence (post-spike 2026-05-25, advisor 3rd consult confirmed):** PR 1a (hygiene+SP-4, ~270 attention LOC) + PR 1b (ripple, ~182 attention LOC) → PR 2 = **Option D wrap-glam** (spike confirmed Option C exceeds 3× threshold in 4/5 scenarios + R12 RenderBox::size cascade blocker) → PR 3 kurbo bridge via mint cascade (­~5 lines) → done.

| # | Deliverable | Status | Owner artifact | Exit gate |
|---|---|---|---|---|
| **PR 1 — Polish pass (single PR, atomic per-U commits)** ||||
| N-geom.U1 | Remove `From<f32/f64/i32/u32/usize> for Pixels` | ✓ **done** `87740bed` | research §III U1 | `cargo build` + `cargo test -p flui-geometry` green; no `.into()` writing into Pixels context in flui-geometry/rendering/painting |
| N-geom.U2 | Remove cross-type `PartialEq<f32>`/`PartialOrd<f32>`/`Add<f32>`/`Sub<f32>` for Pixels | ✓ **done** `35db8a16` | research §III U2 | `compile_fail` doctest passes (e.g. `let _ = px(10.0) + 5.0;` rejected) |
| N-geom.U3 | `EdgeInsets = Edges<Pixels>` migration (**24 production sites measured 2026-05-24**, not 50) | ✓ **done** `aa31cb0f` | research §III U3 + risks R7.5 | `rg "Edges<f32>" crates/` returns 0 hits; 24 sites migrated (15 in `sliver_padding.rs`, 6 in `padding.rs`, 3 elsewhere); `EdgeInsets::all(px)`/`symmetric`/`only`/`zero` constructors added |
| N-geom.U4 | Remove `Mul<Pixels> for Pixels` (area-as-length bug) bundled with U9+U10 | ✓ **done** `274cd59e` | research §III U4/U9/U10 | `compile_fail` doctest: `let _: Pixels = a*b;` rejected; `MulAssign<Pixels>` and `DivAssign<Pixels>` also removed |
| N-geom.U5 | Deprecate `to_device_pixels(f32)` + wrapper cascade in Size/Point/Bounds | ✓ **done** `bc85c43a` | research §III U5 | `#[deprecated]` on 3 unit fns + 3 wrapper fns; all internal callers use typed `to_device(ScaleFactor)` |
| N-geom.U6 | Remove dead `FloatPoint`/`FloatVec2`/`FloatSize`/`FloatOffset` aliases (SP-4) | ✓ **done** `a322e35b` | research §III U6 | `rg 'Float(Point|Vec2|Size|Offset)' crates/` returns 0 hits |
| N-geom.U6.1 | **Delete `ScaledPixels` and all `Scaled*` aliases** (SP-4, decision in research §VIII DevicePixels representation) | ✓ **done** `6b726ee2` | research §VIII DevicePixels decision | `rg 'ScaledPixels|scaled_px|ScaledPoint|ScaledVec2|ScaledSize' crates/` returns 0 hits; final 2-tier shape `Pixels(f32)` + `DevicePixels(i32)` |
| N-geom.U7 | **Delete** `ScaleFactor::transform_scalar<T>` (its doc-example contradicts its own type safety) | ✓ **done** `532bb669` | research §III U7 | Function removed; existing typed `Pixels::to_device(ScaleFactor)` covers the use case |
| N-geom.U11 | Audit `From<Pixels> for i32/u32/usize` lossy conversions (follow-up commit, lower priority) | ✓ **done** `80c0bdc8` | research §III U11 | Replace 3 `From` impls with explicit `to_i32_round()` / `to_u32_round_clamped()` / `to_usize_round_clamped()` |
| N-geom.U12 | Install `port-check.sh` refusal trigger for unit-barrier regression | ✓ **done** `95420b13` | research §III U12 | New trigger #14 rejects `From<f32> for X` / `PartialEq<f32> for X` / `Float*` type aliases in `flui-geometry/` |
| **SPIKE — COMPLETED 2026-05-25 (decision: Option D)** ||||
| N-geom.U17 | ~~2-day spike: wrapper + Padding migration measurement~~ | ✓ **done** | [research/2026-05-25-u17-spike-report.md](research/2026-05-25-u17-spike-report.md) | **Measured wrapper 1,396 LOC + per-widget 37 LOC (with shims) / ~7 LOC projected. 4 of 5 scenarios over 2,250 threshold. Central scenario E = 2,900 LOC (over by 650). NEW R12 blocker: `RenderBox::size(&self) -> &Size` requires cascade migration. R13 (from_lengths inference) + R14 (ZERO ambiguity) = codebase-wide tax. → Option D approved (advisor 3rd consult confirmed).** |
| **PR 2 — Option D SELECTED (post-spike 2026-05-25)** ||||
| N-geom.U14 | **(SELECTED)** Option D — wrap `Matrix4` / `Vec2<U>` / `Transform*` over `glam::Mat4`/`Vec2`/`Affine*` internals; preserve public API. Document as deliberate choice ("flui owns unit-typed wrappers for polish discipline; glam handles SIMD math"). | ✓ **done** `600f4182` (U14.1) | research §VIII Option D + spike report §6 | `#[repr(transparent)]` wrappers; `size_of::<Matrix4>() == size_of::<glam::Mat4>()` test; **126 public fns** → **500–850 LOC delegation**; `inverse()` keeps `Option<Matrix4>`; hand-written `mul_simd_sse`/`mul_simd_neon` deleted; `simd` Cargo feature removed; `glam = { features = ["bytemuck", "mint"] }` → engine Pod-conversion shim deletable; mint cascade auto-bridges kurbo (PR 3 = ~5 lines). |
| N-geom.U14C | ~~Option C — full euclid+glam+kurbo+mint hybrid~~ | 🚫 **not selected** | spike rejected per decision rule + R12 | Available as future on-ramp if R12 (`RenderBox::size` trait surface migration) is decoupled by a different motivation. Spike findings preserved in [research/2026-05-25-u17-spike-report.md](research/2026-05-25-u17-spike-report.md). |
| N-geom.U15 | Update `flui-types/README.md:280` FAQ on glam/euclid | ✓ **done** `a69868d7` | research §VIII | FAQ explains chosen path; mint as bridge; Flutter-compat as extension traits |
| N-geom.U16 | Audit `flui-engine` direct `glam::Vec2` imports; align with bridge policy | ✓ **done** `a69868d7` | research §VIII | Either typed-wrapper import or explicit `flui_geometry::raw::Vec2` re-export; no random direct glam imports |
| **PR 3 — kurbo bridge (Core.2 entry preconditions)** ||||
| N-geom.U8 | `feature = "kurbo"` bridge module in `flui-geometry/src/bridges/kurbo.rs` | ☐ todo (gates Core.2) | research §III U8 | If Option D → ~5-line mint cascade pass-through (glam mint feature + kurbo mint feature). If Option C → explicit `From<flui::Point<Pixels>> for kurbo::Point` (lossless f32→f64) + `TryFrom<kurbo::Point> for flui::Point<Pixels>` (fallible f64→f32 with `KurboBridgeError::OutOfRange`); same for Size/Rect/Affine; all `as` casts marked `PORT-CHECK-OK-SP3` |
| **PR 4 — Deferred indefinitely** | Option D is the destination. Re-open only if euclid migration motivation appears later. ||||
| N-geom.U13 | Monitor [zed#32339](https://github.com/zed-industries/zed/pull/32339) for `DevicePixels + ScaledPixels → PhysicalPixels<S>` unification | ◐ watching | research §III U13 | If upstream merges, consider how euclid's `Length<i32, DevicePixelsUnit>` aligns with their `PhysicalPixels<S>` pattern; may inform future tweaks |

### Mythos closures inside Core.0

| # | Deliverable | Status | Owner artifact | Exit gate |
|---|---|---|---|---|
| M1 | **Cycle 4** (rendering × engine) closures | ⚠ verify | [`research/2026-05-22-cycle4-wave2-design.md`](research/2026-05-22-cycle4-wave2-design.md), [`research/2026-05-22-cycle4-wave2-receipts.md`](research/2026-05-22-cycle4-wave2-receipts.md) | All audit findings in wave-2 receipts marked closed; verify no open follow-ups |
| M2 | **Cycle 5** (painting × view) closures | ⚠ verify | [`plans/2026-05-22-005-refactor-painting-view-cycle5-plan.md`](plans/2026-05-22-005-refactor-painting-view-cycle5-plan.md), [`research/2026-05-22-cycle5-receipts.md`](research/2026-05-22-cycle5-receipts.md) | Audit findings closed; `flui-view/src/element/` and `flui-view/src/tree/` handed off cleanly; **gates N5.0** |
| M3 | **Layer / semantics repair plan** landed | ◐ in-progress | [`plans/2026-05-22-004-feat-layer-semantics-repair-plan.md`](plans/2026-05-22-004-feat-layer-semantics-repair-plan.md), [`research/2026-05-22-flui-layer-semantics-audit.md`](research/2026-05-22-flui-layer-semantics-audit.md) | Plan tasks complete; layer + semantics audit findings closed |

---

## Cross-tracks — continuous

### Cross.A — Animation / assets / physics re-entry

| # | Deliverable | Status | Owner | Exit |
|---|---|---|---|---|
| A1 | Re-enable `flui-animation` (needed by Core.1 slice) | 🛇 blocked | gated by Core.1 entry | crate in `[workspace.members]`; tests pass |
| A2 | Re-enable `flui-assets` (needed by `Image` widget in Business.1) | 🛇 blocked | gated by Business.1 entry | crate in `[workspace.members]`; tests pass |
| A3 | `flui-types/src/physics/` parity audit | ☐ todo | same as N13 | see N13 |

### Cross.P — Platform breadth

| # | Deliverable | Status | Owner | Exit |
|---|---|---|---|---|
| P1 | Finish Windows backend in `flui-platform` | ☐ todo | — | trivial app runs on Windows; per-platform smoke test green |
| P2 | Finish macOS backend | ☐ todo | — | trivial app runs on macOS |
| P3 | Complete `winit` fallback | ☐ todo | — | trivial app runs via winit on any host |
| P4 | Native **Android** backend (`STRATEGY.md` first-class commitment) | ☐ todo | examples scaffolds present (`examples/android_*`) | trivial app runs on Android device/emulator |
| P5 | Native **iOS** backend (`STRATEGY.md` first-class commitment) | ☐ todo | — | trivial app runs on iOS device/simulator |
| P6 | Wayland support | ☐ todo | — | trivial app runs on Wayland |
| P7 | Engine backend breadth — DX12 / Metal / Vulkan / WebGPU surface management | ☐ todo | `flui-engine` | per-backend smoke test |

### Cross.D — Developer tooling

| # | Deliverable | Status | Owner | Notes |
|---|---|---|---|---|
| D1 | Re-enable `flui-devtools` (inspector, frame profiler) | 🛇 blocked | inspector after Core.0; **frame profiler blocked on App.1** (needs wired vsync) | crate in members; inspector functional |
| D2 | Re-enable `flui-build` (Android/iOS/Desktop/Web) | 🛇 blocked | **blocked on Cross.P mobile backends** (P4, P5) | `flui build` works for all platform targets |
| D3 | Re-enable `flui-cli` (`flui new` / `build` / `run`) | ☐ todo | — | scaffolding command works; runs hello-world |
| D4 | Harden `flui-hot-reload` (preserve scene state) | ◐ partial | crate is `ACTIVE`; example `desktop_scene` works | scene state preserved across reload; documented |

### Cross.H — Foundation hardening (standing discipline)

| # | Deliverable | Status | Owner | Notes / gate |
|---|---|---|---|---|
| H1 | **D-7 layer lifecycle protocol** | 🛇 part of M3 | layer/semantics repair plan | **gates App.1** |
| H2 | **D-8 parallel-type collapses** | ☐ todo | — | `port-check.sh` parallel-type trigger green |
| H3 | **D-9 `BuildContext.new_minimal` hole** | ☐ todo | — | **gates Catalog.1**; no `new_minimal` callers outside tests |
| H4 | **D-10 focus / tab navigation** | ☐ todo | — | focus traversal contract documented + tested |
| H5 | **D-11 `TreeWrite::remove` cascade** | ☐ todo | — | removal cascade tested under nested-tree scenarios |
| H6 | **D-12 Ticker lifecycle** | ☐ todo | gated near Core.1 (animation re-entry) | Ticker dispose order documented + tested |
| H7 | **Speculative-scaffolding feature-gating** | ☐ todo | — | feature flags audited; no leak of speculative code into stable builds |

---

## Core.1 — Vertical slice  *(entry: Core.0 exit)*

| # | Deliverable | Status | Notes |
|---|---|---|---|
| C1.0 | Create `flui-widgets` skeleton crate (L6) | 🛇 blocked | gated by Core.0 |
| C1.1 | Re-enable `flui-animation` (A1) | 🛇 blocked | required for slice |
| C1.2 | `Container` / `Padding` / `Center` widgets | 🛇 blocked | box layout |
| C1.3 | `Column` / `Row` widgets | 🛇 blocked | exercises C2 + C6 |
| C1.4 | `Text` widget (forces `RenderParagraph` over cosmic-text) | 🛇 blocked | leaf + paint |
| C1.5 | `GestureDetector` widget | 🛇 blocked | input / hit-testing |
| C1.6 | `SingleChildScrollView` widget | 🛇 blocked | viewport/offset path |
| C1.7 | **Dynamic-count `ListView`** (Vec-driven children) — **mandatory**, validates C2 dynamic `Vec<BoxedView>` path | 🛇 blocked | without it the slice skips where Material lives |
| C1.8 | `AnimatedContainer` or `AnimatedOpacity` (implicit animation) | 🛇 blocked | exercises `flui-animation` + `memoize`/`can_update` |
| C1.9 | `StatefulView` counter | 🛇 blocked | exercises C1 (`setState`) |
| C1.10 | Demo app assembled entirely from slice widgets, running on one desktop platform with real frame loop | 🛇 blocked | Core.1 ultimate gate |
| C1.11 | Per-contract test pass: C1 / C2 (both tuple + Vec) / C3 / C4 / C5 / C6 / C7 | 🛇 blocked | report at `docs/research/2026-XX-XX-phase1-contract-validation.md` |
| C1.12 | Frame-time histogram ≤ 16ms median over 5-second animation run | 🛇 blocked | proves real `Ticker` |
| C1.13 | Ported Flutter test scaffolding at `crates/flui-widgets/tests/parity/` | 🛇 blocked | parity oracle infrastructure goes live |

---

## Core.2 — Render-object catalog  *(entry: Core.1 exit + N12)*

Roughly **73 render objects** to build. Tracked by family — full enumeration deferred to the Core.2 task spec.

| Family | Status | Notes |
|---|---|---|
| Box layout (`RenderStack`, `RenderPositioned`, `RenderConstrainedBox`, `RenderLimitedBox`, `RenderAspectRatio`, `RenderBaseline`, `RenderWrap`, `RenderFlow`, `RenderTable`, `RenderFractionallySizedBox`) | 🛇 blocked | gated by Core.1 |
| Paint effects (`RenderClipRect/RRect/Path/Oval`, `RenderDecoratedBox`, `RenderOpacity` variants, `RenderTransform` family, `RenderCustomPaint`, `RenderRepaintBoundary`) | 🛇 blocked | partial: see existing `flui-rendering/src/objects/` |
| Slivers (`RenderViewport`, `RenderSliverList/Grid/Padding/FillViewport/ToBoxAdapter`) | 🛇 blocked | sliver constraint protocol already typed |
| Input / leaf (`RenderParagraph`, `RenderImage`, `RenderMouseRegion`, `RenderPointerListener`, `RenderListBody`) | 🛇 blocked | `RenderParagraph` likely lands in Core.1 |

**Exit:** widget→render-object checklist complete; per-RO layout + paint tests; intrinsic-size tests where applicable; 1000-item sliver scroll test green; `flui-rendering` coverage ≥ 80%.

---

## Business.1 — Widget catalog  *(entry: Core.2 exit)*

| # | Deliverable | Status |
|---|---|---|
| B1.1 | Full `flui-widgets` catalog beyond slice (layout, `RichText`, `Icon`, scrolling, input, `Navigator`/routing, implicit animations, `Hero`, `MediaQuery`, `LayoutBuilder`, `FutureBuilder`/`StreamBuilder`) | 🛇 blocked |
| B1.2 | Re-enable `flui-assets` (A2) | 🛇 blocked |
| B1.3 | Non-trivial sample app built entirely from `flui-widgets` | 🛇 blocked |
| B1.4 | `Hero` + `GlobalKey` reparenting end-to-end | 🛇 blocked |
| B1.5 | `flui-widgets` coverage ≥ 85% | 🛇 blocked |

---

## Catalog.1 — Material ∥ Cupertino  *(entry: Business.1 exit + H3)*

| # | Deliverable | Status |
|---|---|---|
| K1 | Create `flui-localizations` (shared) | 🛇 blocked |
| K2 | Create `flui-material` (Material 3) — phased: theming → buttons → inputs → navigation → data display | 🛇 blocked |
| K3 | Create `flui-cupertino` (iOS) | 🛇 blocked |
| K4 | Material sample app interactive (Scaffold + AppBar + FAB + ListView of Cards + Dialog) | 🛇 blocked |
| K5 | Cupertino sample app interactive (CupertinoTabScaffold + CupertinoNavigationBar + CupertinoPageRoute swipe-back) | 🛇 blocked |
| K6 | `ThemeData` change in tree of ≥1000 widgets repaints exactly the dependents | 🛇 blocked |

---

## App.1 — Application integration  *(entry: Catalog.1 exit + H1)*

| # | Deliverable | Status |
|---|---|---|
| App.1 | `flui-app` parity — `WidgetsBinding`/`RendererBinding`, `runApp`-equivalent, full frame loop | 🛇 blocked |
| App.2 | `flui-platform` capability traits (`PlatformTextInput`, `PlatformSystemChrome`, `PlatformHaptics`) | 🛇 blocked |
| App.3 | `flui` facade crate + `flui::prelude` | 🛇 blocked |
| App.4 | Mythos cycle on `flui-app` (it has had none) | 🛇 blocked |
| App.5 | Full Material app on a native platform, real vsync (`ControlFlow::Wait`), IME working | 🛇 blocked |
| App.6 | Constitution coverage gates met across stack | 🛇 blocked |

---

## Ordering risks (from ROADMAP §Ordering risks)

| # | Risk | Mitigation owner |
|---|---|---|
| R1 | Catalog built on spine not at target spec | Core.0 hard gate + Core.1 slice |
| R2 | Render-object catalog under-scoped → Business.1 stalls | **N12** (widget→render-object map) |
| R3 | Contract flaw inside `flui-material` (210k LOC) | Core.1 vertical slice contract-validation report |
| R4 | `flui-material` monolithic | Phased internally + parallel with `flui-cupertino` |
| R5 | `Scene`/`DrawCommand` drift breaks engine track | **N11** (contract freeze) |
| R6 | Platform backends slip blocking phase exit | Phase exits met on desktop first, mobile follow-on |

---

## Conventions

- **Adding a row.** New Core.0 deliverable requires a ROADMAP.md amendment first. Cross-track rows can be added as new D-codes (H8, P8, D5, …) without amending Core.0 scope.
- **Promoting `⚠ verify` to `✓ done`.** Run the exit-gate command (test, grep, port-check, build) and paste the verifying command output or path into the row's notes column.
- **Demoting `✓ done` back.** If a regression is found, re-open with `◐ in-progress` + a new owner link to the fix change. Never delete the row.
- **SDD change naming.** Use `core0-<id>` for Core.0 rows (e.g. `core0-n1-d1-layout-phase`), `core1-<id>` for Core.1, etc., so `openspec/changes/` filenames mirror tracker IDs.
- **Engram memory.** Save significant per-row discoveries with `topic_key: roadmap/<phase>/<id>` (e.g. `roadmap/core0/n1`) so cross-session context is recoverable.

---

[← Roadmap](ROADMAP.md) · [Foundations](FOUNDATIONS.md) · [Port Methodology](PORT.md) · [Back to README](../README.md)
