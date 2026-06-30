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
| N1 | **D-1 layout phase wired** — `layout_node_with_children` invokes per-node `RenderEntry::layout` with constraints propagated parent→child | ✓ done | [`plans/2026-05-23-001-feat-pipeline-wiring-d-block-plan.md`](plans/2026-05-23-001-feat-pipeline-wiring-d-block-plan.md) | Integration test: `Padding → Center → ColoredBox` 3-level tree lays out with correct constraints and sizes. **Verified 2026-06-30:** `crates/flui-rendering/tests/u23_run_layout_wiring.rs` + `pipeline_scenarios.rs` + `deep_tree_stack.rs` green in full nextest run (4789/4789 passed, 0 failed). |
| N2 | **D-3 `run_compositing`** — subtree compositing-bits walk implemented | ✓ done | same plan as N1 | Integration test: layer subtree marked dirty triggers compositing-bits propagation. **Verified 2026-06-30:** `crates/flui-rendering/tests/u34_compositing_bits_walk.rs` green in full nextest run. |
| N3 | **D-4 `run_paint` dirty-flag fix** — clear `needs_paint` only on nodes actually painted | ✓ done | same plan as N1 | Integration test: `RepaintBoundary`-isolated repaint clears `needs_paint` only on painted nodes. **Verified 2026-06-30:** `crates/flui-rendering/tests/u35_paint_dirty_flag_discipline.rs` + `root_resize_repaint.rs` green in full nextest run. |
| N4 | **D-2 keyed reconciliation** — `key: Option<Key>` on `ElementNode`; route variable-arity reconciliation through keyed algorithm; delete positional path | ☐ todo | (folded into `specs/004-view-element-core` Phase 1/2) | Integration test: `[A(key=1), B(key=2)]` reordered to `[B, A]` preserves element identity (no remount) |
| N5 | **Unified contracts spec — `specs/004-view-element-core`** covering C2 (heterogeneous children), C3 (widget-authoring API), C4+C6 (View trait / element storage / keyed reconciliation) | ◐ in-progress | [`specs/004-view-element-core/spec.md`](../specs/004-view-element-core/spec.md), [`plans/2026-05-22-005-feat-view-element-core-contracts-plan.md`](plans/2026-05-22-005-feat-view-element-core-contracts-plan.md) | 4-PR sequence (Phase 0 benchmarks → 1 storage → 2 reconciler → 3 IntoView) each merged with green gates |
| N5.0 | ↳ Phase 0 — 3-day spec-validation benchmarks (S1 KeyId interning + S2 static-path sketch) | ☐ todo (UNBLOCKED 2026-06-30) | gate cleared: M2+M3 closed | Benchmark report; if FR-022 / FR-016 invert, Phase 1 re-opens. **Now the critical path: with M1/M2/M3/N9 done, N5 is the only remaining Core.0 blocker.** |
| N5.1 | ↳ Phase 1 — storage shape + key field + self-validation round-trip tests | ☐ todo | (Phase 0 first) | `cargo test -p flui-view` green; element storage layout matches spec |
| N5.2 | ↳ Phase 2 — keyed reconciler completion + `ElementCore` rewiring + `ReconcileEvent` trace stream | ☐ todo | (Phase 1 first) | Variable-arity reconciliation passes keyed and positional fallback tests |
| N5.3 | ↳ Phase 3 — `IntoView` surface + `downcast_ref` elimination + derive macros + port-check triggers | ☐ todo | (Phase 2 first) | `downcast_ref` count = 0 in framework code; derive macros produce expected impls |
| N6 | **Refusal triggers #8–#13** installed in [`PORT.md`](PORT.md); mechanically-detectable ones become `port-check.sh` gates | ✓ done | [`scripts/port-check.sh`](../scripts/port-check.sh) (triggers 8–13 are present in script) | `bash scripts/port-check.sh -v` exits 0 with all 13 triggers reporting green; PORT.md cross-reference confirms #8–#13 documented. **Verified 2026-06-30:** `scripts/port-check.sh` reports "all 20 refusal triggers + FR-033 grep clean" (script grew past the original 13). |
| N7 | **Merge `flui-log` → `flui-foundation`** | ✓ done | n/a (crate removed) | `crates/flui-log` absent; no `flui-log` workspace member; log helpers live in `flui-foundation` |
| N8 | **Split `flui-geometry` out of `flui-types`** | ✓ done | n/a (crate exists) | `flui-geometry` present in `crates/` and `[workspace.members]` |
| N9 | **Constitution layer table + edition/Rust-version line amended** | ✓ done | [`FOUNDATIONS.md`](FOUNDATIONS.md) Part IV vs `Cargo.toml` | Constitution version bump recorded; layer table matches FOUNDATIONS Part IV. **Verified 2026-06-30:** `Cargo.toml` `edition = "2024"`, `rust-version = "1.96"` (matches `rust-toolchain.toml` 1.96.0 and FOUNDATIONS Part IV line 220); layer table matches actual `[workspace.members]` (`flui-localizations`/`flui-material`/`flui-cupertino` correctly deferred to Catalog.1). *Corrected: earlier note cited "1.95" — actual and intended is 1.96.* |
| N10 | **`RasterBackend` seam** in `flui-engine` (lyon stays as default implementation; future Vello swap non-breaking) | ✓ done | [`docs/designs/2026-06-30-rasterbackend-seam.md`](designs/2026-06-30-rasterbackend-seam.md) | Trait + lyon adapter compiled into `flui-engine`; engine can swap implementation via a single type parameter or factory. **Done 2026-06-30** (adversarially scoped — a doc-only/lyon-wrapper version would have gamed the gate): (1) fixed an abstract→concrete layering inversion (`CommandRenderer::superellipse_path` no longer reaches into `crate::wgpu`; geometry moved to `crate::superellipse`); (2) added the driver-level `RasterBackend` trait (`raster.rs`), impl for `wgpu::Renderer`, **adopted generically in flui-app** (`render_frame<R: RasterBackend>`) so a backend swap changes only the construction line; (3) lyon made an optional dep gated behind `wgpu-backend` (verified absent via `cargo tree --no-default-features`); (4) port-check **trigger #21** enforces lyon stays confined to `wgpu/tessellator.rs`. Gates: workspace clippy `-D warnings` 0, nextest 4830 passed/0 failed, port-check 21/21. Honest remaining gap documented: backdrop-filter fast-path still wgpu-specific. |
| N11 | **Freeze `Scene` / `DrawCommand` contract** | ✓ done | [`docs/designs/2026-06-30-scene-drawcommand-contract.md`](designs/2026-06-30-scene-drawcommand-contract.md) | Contract documented in `docs/designs/`; CI guard: any change to the type surface requires a coordinated cross-track change note. **Done 2026-06-30:** contract doc freezes the 31-variant `DrawCommand` wire format (`#[non_exhaustive]` confirmed); CI tripwire = exhaustive-match guard `contract_freeze` in `crates/flui-painting/src/display_list/command.rs` (adding/removing/renaming a variant is a compile error). Verified: `cargo test -p flui-painting --lib contract_freeze` → 1 passed. |
| N12 | **Widget → render-object mapping checklist** at `docs/research/widget-renderobject-map.md` | ✓ done | [`docs/research/widget-renderobject-map.md`](research/widget-renderobject-map.md) | File exists; every planned `flui-widgets` widget maps to its render object; **gates Core.2 entry (R2)**. **Verified+reconciled 2026-06-30:** doc existed but was stale ("24 existing"); corrected against authoritative `RENDER_OBJECT_TYPES` catalog → **48 render objects exist, ≈23 remain** (only `RenderSliverGrid` blocks Business.1). Core.2 entry: READY. |
| N13 | **`flui-types/src/physics/` parity audit** vs Flutter `physics` package (Spring / Friction / Gravity) | ✓ done | [`docs/research/2026-06-30-physics-parity-audit.md`](research/2026-06-30-physics-parity-audit.md) | Audit report exists; all Spring/Friction/Gravity behavior tests pass against `.flutter/`. **Done 2026-06-30:** 2 real parity bugs fixed (spring `SpringType` now uses Flutter's `c²−4mk` discriminant; `Tolerance::DEFAULT.velocity` 0.01→0.001 = Flutter's value), 3 intentional Rust-native divergences documented (Friction reparameterized to a clear `decay_rate` k instead of Flutter's confusing `drag`; signed Gravity target; single-bound BoundedFriction), 40 Flutter-derived numeric parity tests added. Adversarially reviewed (caught + fixed an integrity overclaim in a regression test). `cargo test -p flui-types` 118 passed / 0 failed; clippy + fmt clean. |
| N14 | **Zero `unimplemented!()` / `todo!()` in non-test code** (CI grep gate) | ✓ done | port-check trigger #8 | CI grep gate exits 0; no `unimplemented!()`/`todo!()` outside `tests/`/`#[cfg(test)]`. **Verified 2026-06-30:** 40 total occurrences remain, all sanctioned — 35 are `flui-platform` linux/ios platform-init stubs (the documented exemption), 5 are doc-comment examples (`///`/`//`) in flui-rendering (3) + flui-assets (2). Zero real production holes; port-check trigger #8 green. The "42 / 2026-05-24" baseline was stale. |
| N15 | **`cargo build --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` exit 0** (Core.0 final gate) | ✓ done | run `just ci` | Both commands exit 0 against current `main`. **Verified 2026-06-30:** `cargo check --workspace --all-targets` (0 warnings), `cargo clippy --workspace --all-targets -- -D warnings` (exit 0), `cargo nextest run --workspace --exclude flui-platform --test-threads 1` (4789 passed, 4 skipped, 0 failed), `port-check.sh` (20/20 + FR-033). |

### N-geom — `flui-geometry` polish pass + math-stack reconciliation

**Owner research:** [`research/2026-05-24-flui-geometry-polish-pass-research.md`](research/2026-05-24-flui-geometry-polish-pass-research.md) (538 lines).
**Block intent:** close escape hatches in the unit system and reconcile the documented "own everything" stance with the actual `glam`/`mint` Cargo.toml integration. **Sequence:** polish (PR 1 ✓) → **U17 euclid spike (risk gate) ✓ → selected Option D** → PR 2 = Option D (wrap `glam` under the stable public API) → kurbo bridge → done. *(2026-05-24 plan defaulted to Option C; the 2026-05-29 U17 spike measured Option C past the 3× ceiling and flipped the default to D — see below.)*
**Progress (2026-05-29):** **PR 1 landed** (U1/U2/U3/U4(+U9/U10)/U6/U7/U12; barrier enforced by `port-check.sh` trigger #14; U5+U6.1 → PR 2, U11 optional). **U17 spike done** → [report](research/2026-05-29-u17-euclid-spike-report.md): measured Option C ≈ 4,120 LOC (rule) / ~6,000 (census-corrected, ~2,379 forced field→method sites) vs Option D ~750 + 0 downstream churn → **decision gate selected Option D**. **PR 2 (Option D) landed** (`8f2e5ca`): `Matrix4` now delegates to `glam::Mat4` (SIMD + `bytemuck::Pod`), all hand-rolled `unsafe` SIMD deleted, `glam` non-optional with `bytemuck`+`mint`. Scope correction (row `U14.scope`): only the SIMD-heavy, method-API `Matrix4` is wrapped; `Transform` rides it transitively; field-exposing scalar types (`Vec2`/`Point`/`Size`/`Offset`, 1,551 downstream field reads) and the 0-consumer `Transform2D` are intentionally left — wrapping them would reintroduce the Option-C field→method churn D exists to avoid. **Next: PR 3 = kurbo bridge (U8) via the now-enabled `mint` cascade.**

| # | Deliverable | Status | Owner artifact | Exit gate |
|---|---|---|---|---|
| **PR 1 — Polish pass (single PR, atomic per-U commits)** ||||
| N-geom.U1 | Remove `From<f32/f64/i32/u32/usize> for Pixels` | ✓ done | research §III U1 | 5 `From<scalar> for Pixels` impls removed (`units.rs`); `px`/`Pixels::new`/`from_i32` are the only blessed paths; generic geometry math rerouted through new `FloatUnit::from_f32` bridge trait; `compile_fail` doctest `Pixels (line 78)` green; `cargo build --workspace --all-targets` exit 0 |
| N-geom.U2 | Remove cross-type `PartialEq<f32>`/`PartialOrd<f32>`/`Add<f32>`/`Sub<f32>` for Pixels | ✓ done | research §III U2 | 8 cross-type impls removed (both directions); `compile_fail` doctest `Pixels (line 72)` (`px(10.0) == 10.0` rejected) green; `Mul<f32>`/`Div<f32>` scaling kept |
| N-geom.U3 | `EdgeInsets = Edges<Pixels>` migration (**24 production sites measured 2026-05-24**, not 50) | ✓ done (after U1+U2) | research §III U3 + risks R7.5 | alias flipped to `Edges<Pixels>`; `rg "Edges<f32>" crates/` (prod) = 0 hits; sites migrated in `padding.rs`/`sliver_padding.rs`/`box_constraints.rs`; `Edges::all`/`symmetric`/`only_*`/`ZERO` constructors cover ergonomics |
| N-geom.U4 | Remove `Mul<Pixels> for Pixels` (area-as-length bug) bundled with U9+U10 | ✓ done | research §III U4/U9/U10 | `Mul<Pixels>`/`MulAssign<Pixels>`/`DivAssign<Pixels>` for Pixels removed; `compile_fail` doctest `Pixels (line 66)` (`px*px → Pixels` rejected) green; internal area sites fixed via `.get()`/`Size::area()` |
| N-geom.U5 | Deprecate `to_device_pixels(f32)` + wrapper cascade in Size/Point/Bounds | ✓ done | research §III U5 | Surface shrank after U6.1 (the Size/Point/Bounds `to_device_pixels()` cascade + `from_scaled_pixels` were already removed). `#[deprecated]` added to the 2 surviving raw-f32 conversions (`Pixels::to_device_pixels(f32)`, `Pixels::from_device_pixels`), steering to the typed `Pixels::to_device(ScaleFactor)` / `DevicePixels::to_logical(ScaleFactor)` — so new production use is now blocked under `-D warnings`. The module headline doctest switched to the typed path. The 6 `flui-types` test/example/bench targets that intentionally exercise the raw API carry a file-level `#![allow(deprecated)]` (clippy `-D warnings` clean confirms no leaked use elsewhere). |
| N-geom.U6 | Remove dead `FloatPoint`/`FloatVec2`/`FloatSize`/`FloatOffset` aliases (SP-4) | ✓ done | research §III U6 | 4 aliases removed from `lib.rs`; `rg 'type Float(Point|Vec2|Size|Offset)' crates/` = 0 hits |
| N-geom.U6.1 | **Delete `ScaledPixels` and all `Scaled*` aliases** (SP-4, decision in research §VIII DevicePixels representation) | ✓ done | research §VIII DevicePixels decision | Final 2-tier shape reached: `Pixels(f32)` + `DevicePixels(i32)`. Removed the `ScaledPixels` type + all impls, `scaled_px`, the `ScaledPoint`/`ScaledVec2`/`ScaledSize` aliases, the per-type `scale_to_scaled`/`to_scaled`/`from_scaled_pixels`/`to_scaled_pixels` methods, and the `ScaledPixels` entries in the trait macros; `Pixels::scale` now returns `Pixels`. flui-types example/README/4 test files updated (ScaledPixels-dedicated tests deleted). `rg 'ScaledPixels\|Scaled(Point\|Vec2\|Size)\|scaled_px' crates/` = 0. Gates green; also re-blessed the `mixed_units` compile-fail snapshot (E0277→E0308 diagnostic drift; assertion intent unchanged). |
| N-geom.U7 | **Delete** `ScaleFactor::transform_scalar<T>` (its doc-example contradicts its own type safety) | ✓ done | research §III U7 | function removed; `ScaleFactor` doc-example rewritten to `logical.to_device(scale)`; 0 production callers (research §"U7 collision check") |
| N-geom.U11 | Audit `From<Pixels> for i32/u32/usize` lossy conversions (follow-up commit, lower priority) | ☐ todo (optional) | research §III U11 | Not in PR 1 (research: optional follow-up). The 3 lossy integer `From` impls remain; replace with explicit `to_*_round*()` methods in a later hygiene sweep. |
| N-geom.U12 | Install `port-check.sh` refusal trigger for unit-barrier regression | ✓ done | research §III U12 | Trigger #14 added (`scripts/port-check.sh`) banning `From<f32/f64>` / cross-type `f32` ops / `Float*` aliases in `flui-geometry/src`; `PORT-CHECK-OK-UNIT:` allowlist marker; documented in `PORT.md`; `bash scripts/port-check.sh` exits 0 with all 14 triggers green |
| **SPIKE — BEFORE PR 2 (risk gate on Option C; widened to 2 days after advisor R-PreFlight)** ||||
| N-geom.U17 | **~2-day spike**: (1) build wrapper crate scaffold — `flui::Length<T, U>(euclid::Length<T, U>)`, `flui::Point<T, U>`, `flui::Size<T, U>`, `flui::Rect<T, U>` with U1–U12 invariants reimposed + Flutter-API parity methods + bytemuck::Pod derives; (2) migrate one widget (`flui-rendering::Padding`) to the wrapper. **Measure BOTH wrapper LOC AND per-widget migration LOC.** | ✓ done → **selects Option D** | [`research/2026-05-29-u17-euclid-spike-report.md`](research/2026-05-29-u17-euclid-spike-report.md) | **Measured:** wrapper 7.63 code-only LOC/fn (79 fns built) → full 477-fn surface ≈ **3,640 LOC**; Padding migration = **6 lines** (all field→method). Decision rule `3,640 + 6×80 = 4,120 > 2,250` → **Option D**. Census also found **~2,379 geometry field-access sites** that Option C forces into method calls (euclid components are `f32`, not `Pixels`) — a cost the `per_widget×80` rule under-modelled. Spike crate was throwaway (removed); numbers reproducible from the report. |
| **PR 2 — Option D (per U17 spike, 2026-05-29) — confirmed by user** ||||
| N-geom.U14 | **Option D** — back the SIMD-heavy, method-API linear-algebra type(s) with `glam`, preserving the public API (0 downstream churn) | ✓ done (commit `8f2e5ca`) | research §VIII Option D + [U17 report](research/2026-05-29-u17-euclid-spike-report.md) | **`Matrix4` → `glam::Mat4`**: `Mul`/`try_inverse`/`determinant` delegate to glam; hand-rolled `mul_simd_sse`/`mul_simd_neon` + dead `transform_points_simd_*` deleted (all `unsafe` SIMD gone); `#[repr(C)]` + `bytemuck::Pod`/`Zeroable` (engine can `cast_slice` a `Matrix4` directly); `try_inverse` keeps `Option` via determinant guard; `glam` non-optional (`features=["bytemuck","mint"]`); dead `simd` feature removed; stale `glam`/`simd` forwards dropped from flui-types; contract tests added. **−273 LOC.** Gates green (all-targets build, clippy `-D warnings`, fmt, port-check 14/14, geometry tests). |
| N-geom.U14.scope | **Scope correction (2026-05-29):** which types Option D wraps | ✓ resolved | this tracker | The original row said "wrap Matrix4 **/ Vec2 / Transform** ". Post-step-1 analysis: (1) `Matrix4` was the **only** type with hand-rolled SIMD/heavy math — wrapping it captures the entire SIMD + Pod + mint-enable win. (2) `Transform` is an enum that compiles to `Matrix4` → benefits transitively, no change. (3) `Vec2`/`Point`/`Size`/`Offset` expose **public `x/y/dx/dy/width/height` fields read in 1,551+ downstream sites**; wrapping them in `glam::Vec2` (f32 components, not typed `Pixels` fields) would force exactly the field→method churn Option D was chosen to **avoid** → left as own scalar code (no SIMD to delete anyway). (4) `Transform2D` has **0 consumers** + no SIMD → swap is risk-without-reward, left as-is. (5) No engine Matrix4→Pod shim exists today (engine transforms on CPU); `Matrix4: Pod` is now available for a future MVP-uniform. **Net: Option D = `Matrix4`-over-glam + glam Pod/mint enablement; field-exposing scalar types stay — that is precisely why D beats C.** |
| N-geom.U14C | **(DEFERRED by U17)** Option C — thin newtype wrappers over `euclid::{Length,Point2D,Rect,Transform2D}`. | 🛇 deferred (gated on a future field→accessor-method pass) | research §VIII Option C + [U17 report](research/2026-05-29-u17-euclid-spike-report.md) | U17 measured Option C total ≈ 4,120 LOC (rule) / ~6,000 (census-corrected) — exceeds the 2,250 (3×D) ceiling. Blocker is the **~2,379-site field→method conversion** the euclid newtype forces, not euclid itself. Revisit only if/when a standalone field→accessor refactor makes a euclid swap low-churn; **not a PR-2 prerequisite**. |
| N-geom.U15 | Update `flui-types/README.md:280` FAQ on glam/euclid | ☐ todo (with U14 or U14C) | research §VIII | FAQ explains chosen path; mint as bridge; Flutter-compat as extension traits |
| N-geom.U16 | Audit `flui-engine` direct `glam::Vec2` imports; align with bridge policy | ☐ todo (with U14 or U14C) | research §VIII | Either typed-wrapper import or explicit `flui_geometry::raw::Vec2` re-export; no random direct glam imports |
| **PR 3 — kurbo bridge (Core.2 entry preconditions)** ||||
| N-geom.U8 | `feature = "kurbo"` bridge module in `flui-geometry/src/bridges/kurbo.rs` | ✓ done (PR 3) | research §III U8 | **Explicit-bridge path taken** (not the mint cascade): under Option D the flui coordinate types stay own scalar structs — not glam-backed — so `mint` can't auto-bridge them; the typed boundary is explicit. `From<Point/Offset/Size/Rect/Matrix4> for kurbo::{Point,Vec2,Size,Rect,Affine}` (lossless `f32→f64`) + `TryFrom<kurbo::…>` (fallible `f64→f32` via `KurboBridgeError::OutOfRange`, range+finite checked); `Matrix4`↔`kurbo::Affine` maps the 2D affine subset. All casts marked `PORT-CHECK-OK-SP3`. Gated behind `feature = "kurbo"` (absent from default builds; `PORT-CHECK-OK-SP4` marker as it precedes its Core.2 consumer by design). Tests: 5 round-trip/rejection + affine-vs-transform_point parity. Gates green incl. `--features kurbo` clippy `-D warnings`. |
| **PR 4 — Not part of the plan** | Option C IS the destination. No further migration. ||||
| N-geom.U13 | Monitor [zed#32339](https://github.com/zed-industries/zed/pull/32339) for `DevicePixels + ScaledPixels → PhysicalPixels<S>` unification | ◐ watching | research §III U13 | If upstream merges, consider how euclid's `Length<i32, DevicePixelsUnit>` aligns with their `PhysicalPixels<S>` pattern; may inform future tweaks |

### Mythos closures inside Core.0

| # | Deliverable | Status | Owner artifact | Exit gate |
|---|---|---|---|---|
| M1 | **Cycle 4** (rendering × engine) closures | ✓ done | [`research/2026-05-22-cycle4-wave2-design.md`](research/2026-05-22-cycle4-wave2-design.md), [`research/2026-05-22-cycle4-wave2-receipts.md`](research/2026-05-22-cycle4-wave2-receipts.md) | All audit findings in wave-2 receipts marked closed; verify no open follow-ups. **Verified 2026-06-30:** R-6/R-7/R-8/R-9 parallel-type consolidation done (`rg flui_rendering::hit_testing::HitTestResult` / `flui_rendering::input` → 0 production); E-2 backdrop filter wired through offscreen pipeline; `cargo test -p flui-rendering --lib` 278 passed/0 failed. |
| M2 | **Cycle 5** (painting × view) closures | ✓ done | [`plans/2026-05-22-005-refactor-painting-view-cycle5-plan.md`](plans/2026-05-22-005-refactor-painting-view-cycle5-plan.md), [`research/2026-05-22-cycle5-receipts.md`](research/2026-05-22-cycle5-receipts.md) | Audit findings closed; `flui-view/src/element/` and `flui-view/src/tree/` handed off cleanly; **gates N5.0**. **Verified 2026-06-30:** all 15 units / 23 commits across 8 waves merged; keyed reconciliation in production path; parallel surfaces deleted (`canvas::sugar`, `text_layout::fallback`, `NotificationNode`, flat `inherited_elements` → 0 refs); `cargo test -p flui-view --lib` 213 passed/0 failed. **N5.0 UNBLOCKED.** |
| M3 | **Layer / semantics repair plan** landed | ✓ done | [`plans/2026-05-22-004-feat-layer-semantics-repair-plan.md`](plans/2026-05-22-004-feat-layer-semantics-repair-plan.md), [`research/2026-05-22-flui-layer-semantics-audit.md`](research/2026-05-22-flui-layer-semantics-audit.md) | Plan tasks complete; layer + semantics audit findings closed. **Verified 2026-06-30 (code+tests):** layer lifecycle phase 1+2 (`LayerNode` `disposed`/`Drop`/assert-alive, `needs_add_to_scene` dirty-bit propagation), semantics `send_event` callback wired+tested, slab-tree auto-detach + cascade-remove. `cargo test -p flui-layer/-semantics --lib` green. **Bookkeeping gap (non-blocking):** the formal `2026-05-22-flui-layer-semantics-receipts.md` ledger was never filed — code is complete, only the atomic-commit receipts doc is missing. |

---

## Cross-tracks — continuous

### Cross.A — Animation / assets / physics re-entry

| # | Deliverable | Status | Owner | Exit |
|---|---|---|---|---|
| A1 | Re-enable `flui-animation` (needed by Core.1 slice) | 🛇 blocked | gated by Core.1 entry | crate in `[workspace.members]`; tests pass |
| A2 | Re-enable `flui-assets` (needed by `Image` widget in Business.1) | 🛇 blocked | gated by Business.1 entry | crate in `[workspace.members]`; tests pass |
| A3 | `flui-types/src/physics/` parity audit | ✓ done | same as N13 | see N13 (done 2026-06-30) |

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

> **Status note (2026-06-30 reconciliation).** Core.1 is **not** "all blocked" — substantial implementation has landed ahead of the formal Core.0 exit. `crates/flui-widgets/src/` already contains 15 widget families (`container`, `flex`, `text`, `scroll`, `animated`, `stack`, `clip`, `wrap`, `image`, `transitions`, `paint`, `layout`, `interaction`, `app`) with hundreds of passing tests, `flui-animation` is re-enabled in `[workspace.members]`, and production vsync + lazy slivers run end-to-end in a real window (PRs #320–#324). The rows below track the formal **exit gates** (running demo app + per-contract validation report), which remain to be verified as a unit — they are gated on the **N5 contracts spec** (keyed reconciliation / `IntoView` / `downcast_ref` elimination), the one genuinely multi-session Core.0 blocker. Per-widget implementation status should be re-audited row-by-row before promotion.

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
