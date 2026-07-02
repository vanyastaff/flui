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
| N4 | **D-2 keyed reconciliation** — `key: Option<Key>` on `ElementNode`; route variable-arity reconciliation through keyed algorithm; delete positional path | ✓ done (2026-06-30) | Folded into `specs/004-view-element-core` Phase 1/2 | **Verified:** `crates/flui-view/src/tree/id_reconcile.rs::tests::keyed_reorder_ids_follow_keys` and `crates/flui-view/tests/production_reconcile_emits.rs::variable_arity_reorder_through_build_scope_preserves_ids_and_emits_parent_id` prove keyed reorder preserves `ElementId`s through both the direct slab reconciler and the production `BuildOwner::build_scope` path. |
| N5 | **Unified contracts spec — `specs/004-view-element-core`** covering C2 (heterogeneous children), C3 (widget-authoring API), C4+C6 (View trait / element storage / keyed reconciliation) | ✓ done (2026-06-30) | [`specs/004-view-element-core/spec.md`](../specs/004-view-element-core/spec.md), [`plans/2026-05-22-005-feat-view-element-core-contracts-plan.md`](plans/2026-05-22-005-feat-view-element-core-contracts-plan.md) | All four phases are closed with executable evidence: Phase 0 S1/S2 spikes, Phase 1 storage/key round-trip tests, Phase 2 production keyed reconciliation + GlobalKey reparent event/render-link tests, Phase 3 `IntoView`/derive/macro/port-check authoring surface. |
| N5.0 | ↳ Phase 0 — spec-validation benchmarks (S1 KeyId interning + S2 static-path sketch) | ✓ done (2026-06-30) | [S1 report](research/2026-06-30-n5-phase0-s1-keyid-interning-spike.md) · [S2 report](research/2026-06-30-n5-phase0-s2-static-path-spike.md) | **Both spikes pass; neither contract inverts → Phase 1 greenlit.** S1: keep `Box<dyn ViewKey>` (interning's per-frame build cost exceeds its lookup win 2:1, +8.3% mem) — FR-022/FR-016/FR-024 unchanged. S2: static tuple path compiles to **zero vtable calls** (cargo-asm: devirt + const-fold + SIMD) vs one dyn-call/child on the Vec path — SC-007 confirmed, FR-016/FR-018 hold. Load-bearing: `#[inline]` on `tuple_impls::for_each`. |
| N5.1 | ↳ Phase 1 — storage shape + key field + self-validation round-trip tests | ✓ done (2026-06-30) | Commits `c0a90bff` (migration) + `2f4d82c4` (publishing metadata). `ElementNode` stores `ElementKind`; `View::create_element()` returns `ElementKind`; derives/macros/examples updated | Verified workspace-wide: `cargo check --workspace --all-targets`, clippy `-D warnings`, port-check (21 triggers + FR-033), inventory guard, **nextest 4831 passed / 0 failed** |
| N5.2 | ↳ Phase 2 — keyed reconciler completion + `ElementCore` rewiring + `ReconcileEvent` trace stream | ✓ done (2026-06-30) | **Verified:** `BuildOwner::build_scope` feeds `build_into_views` into `reconcile_children_by_id`; `production_reconcile_emits.rs::variable_arity_reorder_through_build_scope_preserves_ids_and_emits_parent_id` proves keyed reorder preserves IDs and emits the real parent id through the public rebuild path. Direct slab corpus covers keyed reorder/shrink/grow/type-mismatch/deep teardown/hash collisions/duplicate keys/S3 permutations. GlobalKey reparent now covers both ADV-1 branches: inactive-queue retake (`global_key_reparent.rs::covers_sc003_reparent_emits_single_reparent_event`) and same-frame Active→Active move with `from_parent: Some(old_parent)` (`global_key_reparent.rs::covers_sc003_active_to_active_reparent_emits_from_parent_and_preserves_state`). Render links are also production-locked by `production_reconcile_emits.rs::active_global_key_move_through_build_scope_updates_render_parent_links`. | Variable-arity keyed/positional reconciliation, `ReconcileEvent` trace stream, and GlobalKey reparent state/event/render-link behavior are covered by focused tests. |
| N5.3 | ↳ Phase 3 — `IntoView` surface + `downcast_ref` elimination + derive macros + port-check triggers | ✓ done (2026-06-30) | **Verified:** `StatelessView::build` and `ViewState::build` use `impl IntoView`; derives are implemented in `flui-macros` and re-exported from `flui_view::prelude`; `derive_smoke.rs` covers stateless/stateful/generic derives; `derive_bon_stack.rs` covers `bon` + derive stacking; `sc001_loc_golden.rs` locks the ≤7-line `Greeting`; `sc009_boxed_view_conditional.rs` locks the `.boxed()` conditional path; `trybuild_ui.rs` now uses an exact 17-child `column!` compile-fail fixture for FR-034/SC-014. `port-check.sh` carries FR-033 and FR-036. Documentation was corrected: `crates/flui-view/README.md` now describes the current surface, `crates/flui-macros/README.md` documents derives and the recursive `.boxed()` rule, and the invalid generated-`impl` `#[diagnostic::on_unimplemented]` plan is explicitly superseded because rustc accepts that attribute only on trait definitions. | View-side `downcast_ref` removal remains a separate Phase 4+ typed-config redesign; Phase 3 closes the executable IntoView/derive/macro/port-check surface without pretending the invalid impl-level diagnostic exists. |
| N6 | **Refusal triggers #8–#13** installed in [`PORT.md`](PORT.md); mechanically-detectable ones become `port-check.sh` gates | ✓ done | [`scripts/port-check.sh`](../scripts/port-check.sh) (triggers 8–13 are present in script) | `bash scripts/port-check.sh -v` exits 0 with all 13 triggers reporting green; PORT.md cross-reference confirms #8–#13 documented. **Verified 2026-06-30:** `scripts/port-check.sh` reports "all 20 refusal triggers + FR-033 grep clean" (script grew past the original 13). |
| N7 | **Merge `flui-log` → `flui-foundation`** | ✓ done | n/a (crate removed) | `crates/flui-log` absent; no `flui-log` workspace member; log helpers live in `flui-foundation` |
| N8 | **Split `flui-geometry` out of `flui-types`** | ✓ done | n/a (crate exists) | `flui-geometry` present in `crates/` and `[workspace.members]` |
| N9 | **Constitution layer table + edition/Rust-version line amended** | ✓ done | [`FOUNDATIONS.md`](FOUNDATIONS.md) Part IV vs `Cargo.toml` | Constitution version bump recorded; layer table matches FOUNDATIONS Part IV. **Verified 2026-06-30:** `Cargo.toml` `edition = "2024"`, `rust-version = "1.96"` (matches `rust-toolchain.toml` 1.96.0 and FOUNDATIONS Part IV line 220); layer table matches actual `[workspace.members]` (`flui-localizations`/`flui-material`/`flui-cupertino` correctly deferred to Catalog.1). *Corrected: earlier note cited "1.95" — actual and intended is 1.96.* |
| N10 | **`RasterBackend` seam** in `flui-engine` (lyon stays as default implementation; future Vello swap non-breaking) | ✓ done | [`docs/designs/2026-06-30-rasterbackend-seam.md`](designs/2026-06-30-rasterbackend-seam.md) | Trait + lyon adapter compiled into `flui-engine`; engine can swap implementation via a single type parameter or factory. **Done 2026-06-30** (adversarially scoped — a doc-only/lyon-wrapper version would have gamed the gate): (1) fixed an abstract→concrete layering inversion (`CommandRenderer::superellipse_path` no longer reaches into `crate::wgpu`; geometry moved to `crate::superellipse`); (2) added the driver-level `RasterBackend` trait (`raster.rs`), impl for `wgpu::Renderer`, **adopted generically in flui-app** (`render_frame<R: RasterBackend>`) so a backend swap changes only the construction line; (3) lyon made an optional dep gated behind `wgpu-backend` (verified absent via `cargo tree --no-default-features`); (4) port-check **trigger #21** enforces lyon stays confined to `wgpu/tessellator.rs`. Gates: workspace clippy `-D warnings` 0, nextest 4830 passed/0 failed, port-check 21/21. Honest remaining gap documented: backdrop-filter fast-path still wgpu-specific. |
| N11 | **Freeze `Scene` / `DrawCommand` contract** | ✓ done | [`docs/designs/2026-06-30-scene-drawcommand-contract.md`](designs/2026-06-30-scene-drawcommand-contract.md) | Contract documented in `docs/designs/`; CI guard: any change to the type surface requires a coordinated cross-track change note. **Done 2026-06-30:** contract doc freezes the 31-variant `DrawCommand` wire format (`#[non_exhaustive]` confirmed); CI tripwire = exhaustive-match guard `contract_freeze` in `crates/flui-painting/src/display_list/command.rs` (adding/removing/renaming a variant is a compile error). Verified: `cargo test -p flui-painting --lib contract_freeze` → 1 passed. |
| N12 | **Widget → render-object mapping checklist** at `docs/research/widget-renderobject-map.md` | ✓ done | [`docs/research/widget-renderobject-map.md`](research/widget-renderobject-map.md) | File exists; every planned `flui-widgets` widget maps to its render object; **gates Core.2 entry (R2)**. **Verified+reconciled 2026-07-01:** doc corrected against authoritative `RENDER_OBJECT_TYPES` catalog → **59 render objects exist, ≈13 remain**. `RenderSliverGrid`/`RenderSliverGridLazy`, `RenderShrinkWrappingViewport`, `RenderCustomSingleChildLayoutBox`, `RenderCustomMultiChildLayoutBox`, and the first `RenderEditable` visual-core slice are closed; `CustomScrollView::shrink_wrap`, `ListView::shrink_wrap`, and `GridView::shrink_wrap` now route to the shrink-wrapping viewport. Core.2 entry: READY. |
| N13 | **`flui-types/src/physics/` parity audit** vs Flutter `physics` package (Spring / Friction / Gravity) | ✓ done | [`docs/research/2026-06-30-physics-parity-audit.md`](research/2026-06-30-physics-parity-audit.md) | Audit report exists; all Spring/Friction/Gravity behavior tests pass against `.flutter/`. **Done 2026-06-30:** 2 real parity bugs fixed (spring `SpringType` now uses Flutter's `c²−4mk` discriminant; `Tolerance::DEFAULT.velocity` 0.01→0.001 = Flutter's value), 3 intentional Rust-native divergences documented (Friction reparameterized to a clear `decay_rate` k instead of Flutter's confusing `drag`; signed Gravity target; single-bound BoundedFriction), 40 Flutter-derived numeric parity tests added. Adversarially reviewed (caught + fixed an integrity overclaim in a regression test). `cargo test -p flui-types` 118 passed / 0 failed; clippy + fmt clean. |
| N14 | **Zero `unimplemented!()` / `todo!()` in non-test code** (CI grep gate) | ✓ done | port-check trigger #8 | CI grep gate exits 0; no `unimplemented!()`/`todo!()` outside `tests/`/`#[cfg(test)]`. **Verified 2026-06-30:** 40 total occurrences remain, all sanctioned — 35 are `flui-platform` linux/ios platform-init stubs (the documented exemption), 5 are doc-comment examples (`///`/`//`) in flui-rendering (3) + flui-assets (2). Zero real production holes; port-check trigger #8 green. The "42 / 2026-05-24" baseline was stale. |
| N15 | **`cargo build --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` exit 0** (Core.0 final gate) | ✓ done | run `just ci` | Both commands exit 0 against current `main`. **Verified 2026-06-30:** `cargo check --workspace --all-targets` (0 warnings), `cargo clippy --workspace --all-targets -- -D warnings` (exit 0), `cargo nextest run --workspace --exclude flui-platform --test-threads 1` (4789 passed, 4 skipped, 0 failed), `port-check.sh` (20/20 + FR-033). |

**N5.1 completion note (2026-06-30):** the atomic storage migration has landed. `ElementKind` is the live element-storage boundary, with factories for stateless/stateful/proxy/inherited/render/animated/parent-data families plus dedicated `Root`, `Error`, and `Notification` variants. `ElementKind::element()` / `element_mut()` are the bridge for lifecycle, build, reconciliation, and context access. `Box<dyn ElementBase>` remains only as explicit type-erasure utility/test surface and in historical docs; production `View::create_element()` returns `ElementKind`.

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
| N-geom.U11 | Audit `From<Pixels> for i32/u32/usize` lossy conversions (follow-up commit, lower priority) | ✓ done (2026-06-30) | research §III U11 | The lossy integer `From<Pixels>` impls are absent; `Pixels` exposes explicit `to_i32_round()`, `to_u32_round_clamped()`, and `to_usize_round_clamped()` methods. Regression guards: `units.rs::test_pixels_explicit_integer_rounding` locks rounding/clamping semantics and the `Pixels` doctest rejects `let _: i32 = px(10.0).into();`. |
| N-geom.U12 | Install `port-check.sh` refusal trigger for unit-barrier regression | ✓ done | research §III U12 | Trigger #14 added (`scripts/port-check.sh`) banning `From<f32/f64>` / cross-type `f32` ops / `Float*` aliases in `flui-geometry/src`; `PORT-CHECK-OK-UNIT:` allowlist marker; documented in `PORT.md`; `bash scripts/port-check.sh` exits 0 with all 14 triggers green |
| **SPIKE — BEFORE PR 2 (risk gate on Option C; widened to 2 days after advisor R-PreFlight)** ||||
| N-geom.U17 | **~2-day spike**: (1) build wrapper crate scaffold — `flui::Length<T, U>(euclid::Length<T, U>)`, `flui::Point<T, U>`, `flui::Size<T, U>`, `flui::Rect<T, U>` with U1–U12 invariants reimposed + Flutter-API parity methods + bytemuck::Pod derives; (2) migrate one widget (`flui-rendering::Padding`) to the wrapper. **Measure BOTH wrapper LOC AND per-widget migration LOC.** | ✓ done → **selects Option D** | [`research/2026-05-29-u17-euclid-spike-report.md`](research/2026-05-29-u17-euclid-spike-report.md) | **Measured:** wrapper 7.63 code-only LOC/fn (79 fns built) → full 477-fn surface ≈ **3,640 LOC**; Padding migration = **6 lines** (all field→method). Decision rule `3,640 + 6×80 = 4,120 > 2,250` → **Option D**. Census also found **~2,379 geometry field-access sites** that Option C forces into method calls (euclid components are `f32`, not `Pixels`) — a cost the `per_widget×80` rule under-modelled. Spike crate was throwaway (removed); numbers reproducible from the report. |
| **PR 2 — Option D (per U17 spike, 2026-05-29) — confirmed by user** ||||
| N-geom.U14 | **Option D** — back the SIMD-heavy, method-API linear-algebra type(s) with `glam`, preserving the public API (0 downstream churn) | ✓ done (commit `8f2e5ca`) | research §VIII Option D + [U17 report](research/2026-05-29-u17-euclid-spike-report.md) | **`Matrix4` → `glam::Mat4`**: `Mul`/`try_inverse`/`determinant` delegate to glam; hand-rolled `mul_simd_sse`/`mul_simd_neon` + dead `transform_points_simd_*` deleted (all `unsafe` SIMD gone); `#[repr(C)]` + `bytemuck::Pod`/`Zeroable` (engine can `cast_slice` a `Matrix4` directly); `try_inverse` keeps `Option` via determinant guard; `glam` non-optional (`features=["bytemuck","mint"]`); dead `simd` feature removed; stale `glam`/`simd` forwards dropped from flui-types; contract tests added. **−273 LOC.** Gates green (all-targets build, clippy `-D warnings`, fmt, port-check 14/14, geometry tests). |
| N-geom.U14.scope | **Scope correction (2026-05-29):** which types Option D wraps | ✓ resolved | this tracker | The original row said "wrap Matrix4 **/ Vec2 / Transform** ". Post-step-1 analysis: (1) `Matrix4` was the **only** type with hand-rolled SIMD/heavy math — wrapping it captures the entire SIMD + Pod + mint-enable win. (2) `Transform` is an enum that compiles to `Matrix4` → benefits transitively, no change. (3) `Vec2`/`Point`/`Size`/`Offset` expose **public `x/y/dx/dy/width/height` fields read in 1,551+ downstream sites**; wrapping them in `glam::Vec2` (f32 components, not typed `Pixels` fields) would force exactly the field→method churn Option D was chosen to **avoid** → left as own scalar code (no SIMD to delete anyway). (4) `Transform2D` has **0 consumers** + no SIMD → swap is risk-without-reward, left as-is. (5) No engine Matrix4→Pod shim exists today (engine transforms on CPU); `Matrix4: Pod` is now available for a future MVP-uniform. **Net: Option D = `Matrix4`-over-glam + glam Pod/mint enablement; field-exposing scalar types stay — that is precisely why D beats C.** |
| N-geom.U14C | **(DEFERRED by U17)** Option C — thin newtype wrappers over `euclid::{Length,Point2D,Rect,Transform2D}`. | 🛇 deferred (gated on a future field→accessor-method pass) | research §VIII Option C + [U17 report](research/2026-05-29-u17-euclid-spike-report.md) | U17 measured Option C total ≈ 4,120 LOC (rule) / ~6,000 (census-corrected) — exceeds the 2,250 (3×D) ceiling. Blocker is the **~2,379-site field→method conversion** the euclid newtype forces, not euclid itself. Revisit only if/when a standalone field→accessor refactor makes a euclid swap low-churn; **not a PR-2 prerequisite**. |
| N-geom.U15 | Update `flui-types/README.md:280` FAQ on glam/euclid | ✓ done (2026-06-30) | research §VIII | `crates/flui-types/README.md` FAQ now documents the selected Option D bridge policy: public geometry remains FLUI-owned for Flutter-compatible APIs + unit barriers; `glam` backs `Matrix4` with SIMD/Pod; `kurbo` is an explicit optional bridge; `mint` is interop glue; Option C/euclid was rejected due the measured ~2,379 field-access churn. |
| N-geom.U16 | Audit `flui-engine` direct `glam::Vec2` imports; align with bridge policy | ✓ done (2026-06-30) | research §VIII | `crates/flui-engine/src/wgpu/mod.rs` defines the math-backend policy: direct `glam` is sanctioned only at the wgpu engine edge for GPU/painter hot-path math, with typed `flui_geometry` converted there. Audit found no direct `glam::` / `use glam` outside `crates/flui-engine/src/wgpu/**`; `scripts/port-check.sh` now enforces that boundary as `N-geom.U16`. |
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
| D3 | Re-enable `flui-cli` (`flui new` / `build` / `run`) | ⚠ verify (2026-06-30) | crate is built out (~7.7k LOC: `create`/`build`/`run`/`doctor`/`devices`/`completions` + `templates/{basic,counter}`) | **Binary exists and is far past "todo", but the `flui new` templates are stale and generate non-compiling projects.** `templates/basic.rs` emits: a non-existent `flui_core` crate + unbuilt Material widgets (`MaterialApp`/`Scaffold`/`AppBar` — Catalog.1, blocked); the wrong View shape (`fn build(self, ctx: &BuildContext) -> impl IntoElement` vs current `StatelessView::build(&self, …) -> impl IntoView`); `edition = "2021"` / `rust-version = "1.91"` vs workspace 2024 / 1.96; local-mode path deps `crates/flui_app` (underscore) vs real `flui-app`. **Decision needed before fixing:** what should `flui new` generate today? The only working windowed bootstrap is platform-level (`examples/hello_world.rs` — no widget tree); the Material/`runApp` template the generator assumes won't compile until App.1 + Catalog.1 land. Exit gate (scaffold a project that compiles + runs hello-world) is **not** met until the template is retargeted to the current API and verified by `cargo check` on the generated output. |
| D4 | Harden `flui-hot-reload` (preserve scene state) | ◐ partial | crate is `ACTIVE`; example `desktop_scene` works | scene state preserved across reload; documented |

### Cross.H — Foundation hardening (standing discipline)

| # | Deliverable | Status | Owner | Notes / gate |
|---|---|---|---|---|
| H1 | **D-7 layer lifecycle protocol** | 🛇 part of M3 | layer/semantics repair plan | **gates App.1** |
| H2 | **D-8 parallel-type collapses** | ✓ done (2026-06-30) | `port-check` trigger #10 + `Cross.H2` | The historical collisions stay collapsed: `ViewKey` is defined only in `flui-foundation`, `IndexedSlot` only in `flui-tree`, and `TargetPlatform` only in `flui-types`. `port-check` keeps the general SP-3 duplicate-type scan and now adds explicit `Cross.H2` canonical-home guards for those three seams. |
| H3 | **D-9 `BuildContext.new_minimal` hole** | ✓ done (2026-06-30) | `flui-view` live `BuildCtx` + `port-check` `Cross.H3` | **Catalog.1 gate closed:** component builds now require the live `BuildOwner::build_scope` `BuildHandle`; the dummy `ElementBuildContext::new_minimal` factory and shared dummy owner/tree cache were deleted; `port-check` bans `new_minimal(` returning to `crates/flui-view/src`. Tests must use `ElementBuildContext::for_element` or drive `build_scope`. |
| H4 | **D-10 focus / tab navigation** | ✓ done (2026-06-30) | `flui-interaction` `FocusManager` / `FocusScopeNode` | `FocusScopeNode` backing nodes now retain a weak scope-owner link, so enclosing-scope lookup, focused-child history, active-scope Tab traversal, skip/unfocusable filtering, wraparound, and `descendants_are_focusable` ancestor gating are tested (`cargo test -p flui-interaction --all-targets`). Local `.flutter/` snapshot is absent in this checkout; behavior is validated against the existing input/frame-loop acceptance notes and current FLUI contract. |
| H5 | **D-11 `TreeWrite::remove` cascade** | ✓ done (2026-06-30) | `flui-tree` `TreeWrite` contract + tests | `TreeWrite::remove` is cascade-by-default via `try_remove`; `remove_shallow` is the explicit opt-out. Tests cover nested cascade, shallow preservation, cycle detection, missing ids, leaf/root removal, and deep-chain stack safety (`cargo test -p flui-tree --all-targets`). |
| H6 | **D-12 Ticker lifecycle** | ✓ done (2026-06-30) | `flui-scheduler` `Ticker` / `TickerFuture` | `Ticker::start` / `start_default` / `start_typed` now return the active `TickerFuture`; `stop` completes it, `dispose` / `Drop` / `reset` cancel it, and release-mode double-start no longer rewrites an active run. Tests cover normal completion, cancel-on-dispose, cancel-on-drop, reset cancellation, provider-created tickers, and post-dispose debug assertions (`cargo test -p flui-scheduler --all-targets`). Flutter source checked via upstream `ticker.dart` because local `.flutter/` is absent. |
| H7 | **Speculative-scaffolding feature-gating** | ✓ done (2026-06-30) | [`research/2026-06-30-h7-speculative-scaffolding-audit.md`](research/2026-06-30-h7-speculative-scaffolding-audit.md) | Workspace-wide `cfg(feature)`/`[features]` audit. **One real leak fixed:** `flui-devtools` `pub mod profiler;` was ungated (614 LOC compiled in every build incl. `--no-default-features` despite `default = []`) → now `#[cfg(feature = "profiling")]`, docs corrected, `profiler_demo` example given `required-features`. **Implicit-dep leaks fixed** via `dep:` (`flui-platform` desktop/winit-backend, `flui-foundation` pretty). **Hygiene:** `flui-scheduler` serde → workspace pin. Dead flags (`flui-app` android/ios/web/overlays, `flui-platform` web/wayland/x11) gate no code → not a leak; documented as forward-planning, left for maintainer. Gates: `cargo check --workspace --all-targets` 0, clippy `-D warnings` 0, port-check 21/21, fmt 0, `flui-devtools --features profiling,timeline,hot-reload` 23 passed/0 failed. |

---

## Core.1 — Vertical slice  *(entry: Core.0 exit)*

> **Status note (2026-06-30, updated post-N5).** Core.0 has **exited** — N5 contracts spec (keyed reconciliation / `IntoView` / element storage) and N15 (final gate) are ✓ done — so **Core.1 is entered, not blocked**. The earlier note called N5 "the one genuinely multi-session Core.0 blocker"; that blocker is now cleared. Substantial slice implementation has landed: `crates/flui-widgets/src/` holds 14 widget families (`container`, `flex`, `text`, `scroll`, `animated`, `stack`, `clip`, `wrap`, `image`, `transitions`, `paint`, `layout`, `interaction`, plus `app`); `flui-animation` is a `[workspace.members]` entry; production vsync + lazy slivers run end-to-end in a real window (PRs #320–#324). **Verified 2026-06-30:** `flui-widgets` 37, `flui-animation` 187, `flui-interaction` 386 lib tests pass (`cargo nextest run -p <crate> --lib`). The remaining Core.1 gates are now: **(a)** the formal demo-app run + frame-time histogram (C1.10 / C1.12 — need a desktop **display**, not verifiable in this headless checkout); **(b)** the per-contract validation report (C1.11 — **✅ MET** 2026-06-30, report at `docs/research/2026-06-30-phase1-contract-validation.md`); **(c)** parity scaffolding (C1.13 — **✅ first slice** 2026-06-30, `crates/flui-widgets/tests/parity/` scaffolding + 18 oracle-cited tests, commit 0f0e14b2; broader corpus grows incrementally). Per-widget rows are ⚠ verify pending row-by-row confirmation; only crate-existence and the animation re-enable are promoted to ✓.

| # | Deliverable | Status | Notes |
|---|---|---|---|
| C1.0 | Create `flui-widgets` skeleton crate (L6) | ✓ done | crate exists with 14 families; `flui-widgets --lib` 37 passed |
| C1.1 | Re-enable `flui-animation` (A1) | ✓ done | in `[workspace.members]`; `flui-animation --lib` 187 passed |
| C1.2 | `Container` / `Padding` / `Center` widgets | ⚠ verify | `container`/`layout` families present; per-widget confirm pending |
| C1.3 | `Column` / `Row` widgets | ⚠ verify | `flex` family present; C2+C6 confirm pending |
| C1.4 | `Text` widget (forces `RenderParagraph` over cosmic-text) | ⚠ verify | `text` family present |
| C1.5 | `GestureDetector` widget | ⚠ verify | `interaction` family + `flui-interaction` (386 lib tests) present |
| C1.6 | `SingleChildScrollView` widget | ⚠ verify | `scroll` family present |
| C1.7 | **Dynamic-count `ListView`** (Vec-driven children) — **mandatory**, validates C2 dynamic `Vec<BoxedView>` path | ⚠ verify | `scroll` family present; dynamic-Vec path confirm pending |
| C1.8 | `AnimatedContainer` or `AnimatedOpacity` (implicit animation) | ⚠ verify | `animated`/`transitions` families present |
| C1.9 | `StatefulView` counter | ⚠ verify | StatefulView/`setState` path present (N5) |
| C1.10 | Demo app assembled entirely from slice widgets, running on one desktop platform with real frame loop | 🛇 needs display | N5 cleared; gate is a formal windowed run — not verifiable in this headless checkout |
| C1.11 | Per-contract test pass: C1 / C2 (both tuple + Vec) / C3 / C4 / C5 / C6 / C7 | ✅ MET (2026-06-30) | Report at `docs/research/2026-06-30-phase1-contract-validation.md`; all 9 contracts (C1–C9) have a passing proving test (C8/C9 via port-check). Proving tests independently re-run (8/8 + C6 production 1/1 + C1 signal-independence grep). Full workspace run 4,847 passed / 4 skipped (contract-unrelated). |
| C1.12 | Frame-time histogram ≤ 16ms median over 5-second animation run | 🛇 needs display | proves real `Ticker`; needs a windowed animation run |
| C1.13 | Ported Flutter test scaffolding at `crates/flui-widgets/tests/parity/` | ✅ first slice (2026-06-30) | Scaffolding + 18 oracle-cited parity tests land (commit 0f0e14b2; plan `docs/research/2026-06-30-c1-13-parity-scaffolding-plan.md`). WidgetTester-shim (screen/find_by_render_type/find_text/pump_widget) over the existing HeadlessBinding+LaidOut harness. Gate = scaffolding exists + first slice passes ✓. Broader corpus + paint/semantics/key finders = Phase 3, grow incrementally. |

---

## Core.2 — Render-object catalog  *(entry: Core.1 exit + N12)*

Roughly **73 render objects** targeted. Tracked by family — full enumeration deferred to the Core.2 task spec.

> **Status note (2026-06-30 parity sweep).** The catalog is **not** "all blocked":
> ~50 render objects already exist (N12), and a systematic oracle parity sweep this
> day verified the major **box / flex / transform-fit / sliver / wrap** families
> faithful to Flutter and **fixed 16 real divergences** (research docs
> `2026-06-30-{box-render-object,renderflex,transform-fit,sliver,renderwrap}-parity-audit.md`).
> **Update 2026-07-01:** the `RenderSliverGrid` gap is closed (eager
> `RenderSliverGrid` plus request-strategy `RenderSliverGridLazy`, with
> `GridView.builder` lazy tests). `RenderShrinkWrappingViewport` is also closed
> and backs both the low-level `ShrinkWrappingViewport` widget and high-level
> `CustomScrollView`/`ListView`/`GridView` `shrink_wrap` composition. The genuine
> `RenderIndexedStack` gap is closed as well: it now backs the public
> `IndexedStack` widget and has harness coverage for selected-child paint,
> hit-test, `None`, and baseline behavior. `RenderCustomPaint` is now in the
> object catalog with harness coverage for preferred sizing, painter paint order,
> and foreground hit-test precedence; repaint-listenable/semantics/cache hints
> remain documented deferred edges. `RenderListBody` is now in the object catalog,
> backs the public `ListBody` widget, and has harness coverage for axis-direction
> layout, reverse positioning, hit testing, dry layout, and dry-baseline behavior.
> `RenderMouseRegion` now backs the public `MouseRegion` widget with harness
> coverage for childless sizing, cursor/annotation hit-entry propagation, hover
> dispatch, MouseTracker enter/hover/exit callback flow, and `opaque = false`
> behind-region behavior now that hit-entry registration is decoupled from
> sibling blocking in the hit-test pipeline.
> `RenderListener` (Flutter `RenderPointerListener`) is also re-verified: the
> childless live/dry layout edge now uses `constraints.biggest`, and the public
> `Listener` routes buttonless `PointerEvent::Move` through hover callbacks,
> FLUI's concrete `PointerEvent::Scroll` through its pointer-signal callback,
> and `PointerEvent::Gesture` through `PointerPanZoomEvent::Update`.
> Its remaining edge is the still-missing pan/zoom start/end callback surface;
> `HitTestBehavior::Translucent` now contributes a self-entry without blocking
> siblings visually behind it.
> `RenderEditable` now exists as the single-line visual core for `EditableText`:
> it owns text layout, collapsed-caret paint, self hit testing, and replaces the
> previous widget-side `Row`/`Text`/`ColoredBox` caret composition. IME,
> composing ranges, selection rendering, scrolling overflow, multiline viewport
> behavior, and platform text input remain explicit App.1/platform work.
> `RenderCustomSingleChildLayoutBox` now exists, backs the public
> `CustomSingleChildLayout` widget, and un-gates `SingleChildLayoutDelegate`;
> harness/widget coverage pins delegated sizing/constraints/position, hit-test
> localization, dry layout/intrinsics, and dry/live baseline forwarding.
> `RenderCustomMultiChildLayoutBox` now exists, backs the public
> `CustomMultiChildLayout` + `LayoutId` widgets, and un-gates
> `MultiChildLayoutDelegate`; harness/widget coverage pins child-id lookup,
> per-child delegated constraints/offsets, reverse-order hit testing, and
> dry layout/intrinsics.
> `RenderTable` now exists, backs the public `Table`/`TableRow`/`TableCell`
> widgets, and fixed a pre-existing type-debt bug in the process
> (`TableCellParentData.vertical_alignment` was non-optional, silently
> breaking "unset cell follows the table's default"; now
> `Option<TableCellVerticalAlignment>`, consolidated onto the single
> `flui_types` enum). Harness coverage pins the 4-pass column-width
> algorithm (including the oracle's own adversarial flex/shrink scenario),
> per-cell geometry, row-decoration/children/border paint order, border
> line placement, per-cell hit testing, and baseline row alignment.
> `MaxColumnWidth`/`MinColumnWidth` combinators, RTL column order, and
> `TableBorder.border_radius` remain documented deferred edges.
> `RenderAnimatedSize` now exists, backs the public `AnimatedSize` widget, and closes the
> render-object-ticker architectural gap via `ADR-0013`
> (`docs/adr/ADR-0013-render-object-attach-self-dirty-handle.md`): a defaulted `attach`/`detach`
> lifecycle pair on `RenderObject`/`RenderBox`/`RenderSliver` lets a render object subscribe to its
> own injected `AnimationController` and self-mark layout dirty, decoupled from widget rebuilds.
> Harness coverage drives the retarget state machine (`Start`/`Stable`/`Changed`/`Unstable`) across
> real multi-frame interpolation, clip-on-overflow, alignment, baseline, and the tight/no-child fast
> path; a widget-level regression test proves an unrelated rebuild does not reset an in-flight resize.
> **The *secondary-query* architectural gap is CLOSED** (verified 2026-07-01 — this tracker
> entry was stale; `ADR-0010`/`ADR-0011`/`ADR-0012` were already implemented and committed
> before this pass, this note just hadn't caught up). `ADR-0010`
> (`docs/adr/ADR-0010-secondary-query-parent-data-accessor.md`) gave `BoxDryLayoutCtx`/
> `BoxIntrinsicsCtx`/`BoxDryBaselineCtx` a type-erased per-child parent-data accessor
> (`child_parent_data`/`child_parent_data_as::<T>`), unifying production and test-harness
> access without a ~122-signature ripple across every `compute_*` overrider. `ADR-0011`
> (D-C) wired the layout-time child-intrinsic channel into the harness, closing
> `RenderIntrinsicWidth`/`Height`'s default (no-`step_width`) forcing gap. `ADR-0012` (D-B)
> gave the live baseline path an eager-record channel, closing flex's container-baseline
> report (`compute_distance_to_actual_baseline`) that previously returned `None`.
> `RenderFlex`/`RenderStack`/`RenderWrap` all now implement `compute_dry_layout` returning
> real sizes (not `Size::ZERO`); flex's `compute_dry_baseline`/`compute_distance_to_actual_baseline`
> both return real values matching the committed layout. 34 harness/unit tests cover this
> (`harness_flex_dry_layout_returns_real_size`, `harness_flex_dry_baseline_equals_committed`,
> `harness_stack_dry_layout_*` ×4, `harness_render_wrap_dry_layout_*` ×2,
> `harness_intrinsic_width_forces_filling_child`, `harness_dry_layout_child_intrinsic_channel_matches_standalone_query`,
> among others) — all independently re-run and passing.
> The genuine remaining work is: **(a)** the
> still-unbuilt family members listed below (now just the deferred/documented edges within
> otherwise-existing families, not missing objects — the render-object catalog itself is
> 74/74 complete per `docs/research/widget-renderobject-map.md`); **(b)** maintainer decisions
> on the documented *intentional* divergences. Per-RO promotion still requires the Core.2 exit
> gate (per-RO tests + 1000-item sliver scroll + coverage).

| Family | Status | Notes |
|---|---|---|
| Box layout (`RenderConstrainedBox`, `RenderLimitedBox`, `RenderAspectRatio`, `RenderBaseline`, `RenderWrap`, `RenderFractionallySizedBox`, `RenderStack`, `RenderIndexedStack`, `RenderListBody`, `RenderPositioned`, `RenderFlow`, `RenderCustomSingleChildLayoutBox`, `RenderCustomMultiChildLayoutBox`, `RenderTable`) | ⚠ mostly exist + audited | ConstrainedBox/LimitedBox/AspectRatio/Baseline/Wrap/FractionallySizedBox/Stack/Flex **exist + oracle-verified faithful or fixed** 2026-06-30. `RenderIndexedStack`, `RenderListBody`, `RenderCustomSingleChildLayoutBox`, `RenderCustomMultiChildLayoutBox`, and `RenderTable` exist + oracle-verified 2026-07-01. `RenderFlow` exists with harness coverage. |
| Paint effects (`RenderClipRect/RRect/Path/Oval`, `RenderDecoratedBox`, `RenderOpacity` variants, `RenderTransform` family, `RenderFittedBox`, `RenderCustomPaint`, `RenderRepaintBoundary`, `RenderPhysicalModel`/`RenderPhysicalShape`, `RenderBackdropFilter`/`RenderShaderMask`, `RenderLeaderLayer`/`RenderFollowerLayer`) | ⚠ mostly exist | `RenderTransform`/`RenderFittedBox`/`RenderFractionalTranslation` audited+fixed 2026-06-30; clip/opacity/decorated_box exist (proxy-paint parity audit in progress). `RenderCustomPaint` exists with first harness slice 2026-07-01; repaint-listenable/semantics/cache hints remain deferred. `RenderPhysicalModel`/`RenderPhysicalShape` (Material elevation shadow+clip primitives) now exist 2026-07-01 — see `docs/research/widget-renderobject-map.md`'s closure note. `RenderBackdropFilter`/`RenderShaderMask` now exist 2026-07-01, extending `flui-rendering`'s paint pipeline with a new closure-scoped effect mechanism (`PaintCx::with_shader_mask`/`with_backdrop_filter`). **`ShaderMask`'s visual rendering gap is now CLOSED (2026-07-01)**: `flui-engine`'s `render_layer_recursive` gained a `Layer::ShaderMask` special case mirroring the already-shipped `BackdropFilter` one — captures the layer's children to an offscreen texture (reusing `Backend::render_shader_mask`'s proven six-step capture-then-mask-then-composite machinery), seeds the offscreen painter's transform with the ambient CTM composed against the bounds' own origin (not a DPR-only reset, which would silently mis-position any `ShaderMask` off the tree root — a GPU regression test nested under a translating ancestor catches exactly this), then masks and composites the result. See the closure note. `RenderLeaderLayer`/`RenderFollowerLayer` now exist 2026-07-01; Tier 2 (render-time position resolution, GPU-tested) landed same day — `CompositedTransformFollower` now positions correctly on screen relative to its target. **Resolved-transform-aware hit-testing is now CLOSED (2026-07-01) per `ADR-0015`** (`docs/adr/ADR-0015-render-follower-hit-test-channel.md`): a `RenderId → LayerId` correlation captured as a paint-phase byproduct, resolved post-paint via the same `flui_layer::resolve_follower_offset` the GPU path uses, and stashed in a `PipelineOwner` side table (`last_follower_offsets`/`last_hidden_follower_ids`) the hit-test walk reads generically — riding the existing transform-stack lifecycle, with `hit_test_transform`'s signature and every other implementor left untouched. A tap on a moved follower now hits at its resolved on-screen position; this is Flutter's own cache-from-last-composite contract (one-frame staleness matching `getLastTransform()`), not a live recompute. `RenderSemanticsAnnotations`/`RenderMergeSemantics`/`RenderExcludeSemantics` now exist 2026-07-01 per `ADR-0014` (`docs/adr/ADR-0014-semantics-assembly-integration.md`): the classic full-rebuild semantics assembly walk landed in `PipelineOwner::run_semantics` (a `SemanticsOwner` field + lifecycle, the boundary-vs-merge decision, `is_merging_semantics_of_descendants`/`excludes_semantics_subtree`), proven by a harness test that specifically covers a nested boundary-declaring descendant correctly collapsing under a `MergeSemantics` ancestor. **The render-object catalog is now 74/74 — zero verified-missing entries remain in `docs/research/widget-renderobject-map.md`.** Deferred per ADR-0014: the modern incremental `_RenderObjectSemantics` compiler, sibling-merge-groups/`RenderBlockSemantics`, cross-frame stable `SemanticsId` reuse, sliver semantics geometry, and the OS accessibility bridge (no AT-SPI/UIAccessibility/MSAA exists in `flui-platform`, and no consumer needs one yet — its own multi-session effort). |
| Slivers (`RenderViewport`, `RenderShrinkWrappingViewport`, `RenderSliverList/Grid/Padding/FillViewport/FillRemaining/ToBoxAdapter/Offstage/Opacity/PersistentHeader`) | ⚠ mostly exist; grid + shrink-wrap viewport + persistent-header blockers closed | Contained slivers audited+fixed 2026-06-30 (offstage correction, overscroll positioning). `RenderSliverGrid` and `RenderSliverGridLazy` now back eager `SliverGrid`/`GridView.count`/`GridView.extent` and lazy `GridView.builder`; lazy grid has a 1000-item scroll-bounded test. `RenderShrinkWrappingViewport` now backs `ShrinkWrappingViewport` plus `CustomScrollView`/`ListView`/`GridView` `shrink_wrap`. `RenderSliverScrollingPersistentHeader`/`RenderSliverPinnedPersistentHeader`/`RenderSliverFloatingPersistentHeader`/`RenderSliverFloatingPinnedPersistentHeader` now exist (no widget-layer `SliverPersistentHeader`/`SliverAppBar` yet — see `docs/research/widget-renderobject-map.md`'s closure note). **Follow-up defects surfaced while building the header family, both now FIXED 2026-07-01:** (1) `RenderViewport::attempt_layout`'s `overlap` field had a wrong sign vs. the oracle (`RenderShrinkWrappingViewport`'s sibling formula was already correct); two harness regression tests added. (2) No insertion path called `RenderObject::attach` for a Sliver child (`PipelineOwner::insert_child_render_object` was Box-protocol-only) — fixed via a new `insert_sliver_child_render_object` plus a protocol-generic `attach_inserted_node` call in `apply_deferred_mutation` (which also collaterally fixed the same gap for lazily-built Box children); proven red-then-green in `attach_detach_lifecycle.rs`. Both cited with file:line in `docs/research/widget-renderobject-map.md`. |
| Input / leaf (`RenderParagraph`, `RenderImage`, `RenderMouseRegion`, `RenderPointerListener`, `RenderEditable`) | ◐ partial | `RenderMouseRegion`, FLUI's `RenderListener`/Flutter `RenderPointerListener`, and the first `RenderEditable` visual-core slice exist + first oracle-verified harness/widget slices 2026-07-01. `RenderParagraph`/`RenderImage` exist; not yet parity-audited. Full text input still needs IME/composing/selection/platform work. |

**Exit:** widget→render-object checklist complete; per-RO layout + paint tests; intrinsic-size tests where applicable; 1000-item sliver scroll test green; `flui-rendering` coverage ≥ 80%. **Secondary-query gap CLOSED** (verified 2026-07-01 — `ADR-0010`/`ADR-0011`/`ADR-0012`, see the closure note above). **1000-item sliver scroll test CLOSED** (2026-07-01): audited the existing candidates (`harness_snapshot.rs`'s `snapshot_lazy_sliver_visible_band` never scrolls; `u3c_9b_bounded_child_count_after_scroll` covers only the first ~20% of the range; `flui-widgets/tests/lazy_grid.rs`'s 1000-item test jumps to one fixed deep offset) and found none actually scroll the full 1000-item range end-to-end. Added `u3c_9c_full_range_scroll_reaches_tail_with_bounded_children` in `crates/flui-rendering/tests/u3c_lazy_sliver_contract.rs`, which drives the scroll position across the entire 0..49,700px range in 200 steps and asserts both bounded attached-child count throughout and that logical indices genuinely reach the tail (min index 993/1000 at the final position) rather than staying stuck near the head. **`flui-rendering` coverage target substantially CLEARED as of 2026-07-01** (`cargo llvm-cov -p flui-rendering`): 83.73% regions / 79.98% functions / 81.14% lines, up from an initial 78.09%/72.58%/75.50% measurement the same day — all three metrics now at or effectively at the ≥80% target (function coverage rounds to 80%). Closed via targeted unit tests on genuinely-undertested files identified by a per-file coverage audit (not blind percentage-chasing): `parent_data/sliver_variants.rs`, `storage/state/{mod,geometry}.rs`, `view/render_view.rs`, `protocol/sliver_protocol.rs` (`SliverLayoutCtx` Direct-mode dispatch), `binding/mod.rs` (`RendererBinding` default methods + `debug_dump_*` helpers), `context/intrinsics.rs` (`BoxIntrinsicsCtx`/`BoxDryLayoutCtx`/`BoxDryBaselineCtx` child-query dispatch), `traits/render_object.rs` (`RenderObject<P>`'s ~20 default methods + `HitTestOutcome` ctors), and `context/layout.rs` (`LayoutContext`'s ~50 delegating methods across Box/Sliver cross-protocol bridges) all went from 2.6-67% to 65-100%, mostly 95-100%. One of these (`view/render_view.rs`) surfaced a genuine parity bug in the process: `RenderView::set_configuration` reordered `self.configuration.take()`/reassignment around the root-layer-rebuild call, so any device-pixel-ratio change after the first frame panicked instead of rebuilding the root layer — fixed to match `.flutter/.../view.dart:173-186`'s assign-before-rebuild ordering. Remaining gaps are a handful of pure trait-definition files (measurement artifacts, e.g. `protocol/capabilities.rs`) not worth chasing further per this pass's own reasoning.

---

## Business.1 — Widget catalog  *(entry: Core.2 exit — SATISFIED 2026-07-01, see Core.2 exit note above)*

> **Status correction (2026-07-01):** this phase's rows below were stale at
> "🛇 blocked" against the *entry* condition alone, but ~15-20 commits of
> real Business.1-scoped work already landed before Core.2's exit gate was
> even formally closed (GridView incl. `.builder`/lazy `RenderSliverGridLazy`,
> SliverGrid, CustomScrollView, ShrinkWrappingViewport, Spacer/SafeArea/
> Visibility, the SliverFillRemaining family, IndexedStack, ListBody, Table,
> MouseRegion, single-line `RenderEditable`, the implicit-animation family,
> Semantics widgets). The blanket "blocked" status did not reflect this.
> Corrected below to partial/in-progress with a concrete done-vs-missing
> breakdown per `crates/flui-widgets/src/` inventory and `docs/research/
> widget-renderobject-map.md` (updated 2026-07-01, the authoritative 74/74
> render-object catalog).

| # | Deliverable | Status | Notes |
|---|---|---|---|
| B1.1 | Full `flui-widgets` catalog beyond slice (layout, `RichText`, `Icon`, scrolling, input, `Navigator`/routing, implicit animations, `Hero`, `MediaQuery`, `LayoutBuilder`, `FutureBuilder`/`StreamBuilder`) | ◐ partial | **Done:** layout family (Align, Stack/Positioned/IndexedStack, Flex, Table, Wrap, Flow, sized/constrained/overflow boxes); scrolling (ListView, GridView incl. `.builder`/lazy, CustomScrollView, ScrollController/Physics/Scrollbar, RefreshIndicator, slivers); input (GestureDetector, Listener, MouseRegion, AbsorbPointer/IgnorePointer, single-line TextField/EditableText); implicit animations (AnimatedAlign/Container/Opacity/Padding/Size + Fade/Rotation/Scale transitions); `MediaQuery` (+ bonus `Theme`). **Missing:** `RichText` (only plain `Text` exists), `Icon` (zero widget — only an unrelated `CursorIcon` enum), `Navigator`/routing (zero implementation), `Hero` (zero occurrences), `LayoutBuilder` (existed pre-rewrite, commit `bb58a8fa`, wiped in the `flui-widgets`/`flui-objects` purge, never rebuilt), `FutureBuilder`/`StreamBuilder` (zero occurrences). |
| B1.2 | Re-enable `flui-assets` (A2) | 🛇 blocked | `flui-assets` crate directory exists but is excluded from `[workspace.members]`; re-enablement itself not yet investigated in this pass — A2's own row (line ~119) is the tracking point. |
| B1.3 | Non-trivial sample app built entirely from `flui-widgets` | 🛇 blocked | Genuinely gated on the B1.1 gaps above (at minimum `Navigator`, likely `Icon`/`RichText` for a realistic app), not on Core.2 anymore. |
| B1.4 | `Hero` + `GlobalKey` reparenting end-to-end | 🛇 blocked | `Hero` has zero occurrences in the codebase; this is unstarted, not blocked by an external precondition. |
| B1.5 | `flui-widgets` coverage ≥ 85% | ⚠ verify | Initial measurement 2026-07-01 (`cargo llvm-cov -p flui-widgets --no-fail-fast`, needed due to one order-dependent flaky test — confirmed pre-existing `flui-app` singleton-state flake per AGENTS.md's Testing Quirks, passes cleanly at `--test-threads 1`): 69.35% regions / 70.16% functions / 72.64% lines. Closed via a per-file audit (not blind percentage-chasing), one file/commit at a time — full per-file breakdown in git log for this branch. Most closures were pure coverage gaps (render-object wiring already correct), verified via each render object's own public getters or derived `Debug` output; recent notable ones: `custom_scroll_view.rs`/`list_view.rs` (build-branch dispatch — shrink_wrap/lazy/horizontal-axis combinations only ever exercised by the default path), `stack/stack.rs` (`IndexedStack` wiring, distinct from `Stack`), `gesture_arena_scope.rs` (proved the shared-arena identity via a real `TapGestureRecognizer` member, since `GestureArenaMember` is sealed), `custom_single_child_layout.rs` (delegate identity verified via the two existing concrete `SingleChildLayoutDelegate` impls), `text_field.rs` (58%→97% — `focus_first_text_node_in_root_scope`, the tap-to-focus heuristic, and `field_border_decoration` were both private free functions with zero direct coverage anywhere in the suite). Three real bugs found and fixed along the way: `RenderView::set_configuration` (see Core.2 exit note); `OverflowBox`/`SizedOverflowBox` silently dropping `alignment` changes on rebuild — `RenderConstrainedOverflowBox`/`RenderSizedOverflowBox` never exposed the `set_alignment` their shared `AligningShiftedBox` component already had, so a widget rebuilt with a new alignment kept its first-mount alignment forever; fixed by adding the missing setters and wiring them into both widgets' `update_render_object`, with a red-then-green regression test for each (confirmed by temporarily reverting the fix); and a `flui-rendering` virtualizer panic (`ExtentTree::seek_sorted` underflowed `hi - lo` when a lazily-built band's total extent was exactly zero — an edge case the `seek_sorted` proptest's generator range, `0.1..50.0`, structurally could never hit) — fixed by clamping `hi` to `hi.max(lo)`, with a red-then-green regression test in `sumtree.rs` proving the fix (`b980c0c1`). One stale memory note corrected: `SliverGridDelegate` is not behind `experimental-delegates` (only `CustomClipper` still is). Current: **83.61% regions / 83.39% functions / 85.38% lines** — line coverage has crossed the ≥85% target; region/function still closing in. |

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
