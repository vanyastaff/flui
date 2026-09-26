# ADR-0087: One raster contract in `flui-layer` with wgpu and CPU backends; retained layer identity drives damage

- **Status:** Proposed
- **Date:** 2026-09-25
- **Amends (on acceptance):** [ADR-0006](ADR-0006-c-ir-record-replay-seam.md) §5 (the scene-level backend trait
  and the GPU-free lowering move to `flui-layer`; the Command IR stays in the engine),
  [ADR-0061](ADR-0061-damage-needs-layer-identity.md) (names the producer: retained boundary
  subtrees keyed by `RenderId` and a differ emitting `DamageRegion::Partial`),
  [ADR-0068](ADR-0068-frame-disposition-replaces-the-presented-bit.md) (`PresentDisposition`
  moves to `flui-layer`, unchanged), [ADR-0045](ADR-0045-raster-lane.md) §1 (the home of
  `RasterBackend`)
- **Supersedes (on acceptance):** the single-rasteriser stance in the introduction of
  [`crates/flui-engine/ARCHITECTURE.md`](../../crates/flui-engine/ARCHITECTURE.md) ("No other
  rasteriser … is planned, and nothing here exists to make one pluggable")
- **Related:** [ADR-0057](ADR-0057-coverage-correct-blending-is-capability-gated.md) (capability
  gates and named fallbacks), [ADR-0062](ADR-0062-the-paint-queue-is-the-cross-pass-record.md),
  [ADR-0063](ADR-0063-the-renderer-owns-its-surface-target.md),
  [ADR-0066](ADR-0066-display-list-command-representation.md),
  [ADR-0067](ADR-0067-engine-owned-glyph-atlas.md),
  [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md) (tier, kind and reach facts for the new
  crate), [ADR-0091](ADR-0091-one-owner-thread-isolated-realms-raster-thread.md) (one raster
  thread per `GpuContext`)
- **Refs:** decisions D5 and D6 in [`design/decisions.md`](../../design/decisions.md); the review
  in [`report-architecture.ru.md` §4.7](../research/2026-09-25-architecture-review/report-architecture.ru.md)

## Context

### There is one rasteriser, by stated policy

`crates/flui-engine/ARCHITECTURE.md:8-13` says wgpu "is the engine, not a backend behind one",
that no other rasteriser is planned, and that `RasterBackend` is "a GPU-free test seam for the
application frame loop, not a plugin point". The seam is real and already used that way:
`RasterBackend: Send` (`crates/flui-engine/src/raster.rs:100`) returns
`Result<PresentDisposition, EngineError>` (`raster.rs:111`), `PresentDisposition` is defined next
to it (`raster.rs:54`), and it has one production implementation (`impl RasterBackend for
crate::Renderer`, `raster.rs:179`) and eight test doubles, five in `flui-app`
(`crates/flui-app/src/app/raster_lane.rs:558`, `raster_test_support.rs:111`,
`runner/device_recovery.rs:412,718`, `runner/surface_lifecycle.rs:685`) and three in
`flui-engine` (`raster.rs:230`, `raster_owner.rs:1775`,
`tests/raster_backpressure_allocation.rs:95`), plus a bench and a compile-fail fixture.
`flui-app` imports all three names from the engine
(`crates/flui-app/src/app/raster_lane.rs:59`). `EngineError` is wgpu-shaped: 18 lines of
`crates/flui-engine/src/error.rs` mention wgpu.

The GPU-free half of lowering a `Scene` — traversal order, clip and opacity discipline, effect
decomposition — lives in the engine as crate-private traits: `CommandRenderer`
(`crates/flui-engine/src/command_renderer.rs:31`), `LayerStateStack`
(`crates/flui-engine/src/layer_state_stack.rs:41`) and `LayerVisitor`
(`crates/flui-engine/src/layer_walk.rs:94`). A second walker already exists and has diverged:
`HeadlessRenderer` (`crates/flui-engine/src/headless.rs:54`) documents that it only approximates
`BackdropFilter`, `ShaderMask` and `Follower` (`headless.rs:15-27`).

Every pixel test therefore needs a GPU adapter. The facade's `gpu-readback-tests` feature is off
by default because "the workspace `test` job has none" (`Cargo.toml:618-625`),
so golden images cannot run in the ordinary test job, and a machine with no usable adapter has
no renderer at all.

### Every frame is a full repaint

`DamageRegion` has one variant, `Full` (`crates/flui-layer/src/scene_snapshot.rs:16-21`), and the
raster lane always sends it (`crates/flui-app/src/app/raster_lane.rs:354`). ADR-0061 decided
that damage must come from comparing consecutive layer trees, which needs layers with an
identity that survives a frame, and stated that such pairing "does not exist": `LayerNode`
carried an `element_id` that was always `None`.

That half of ADR-0061's context is now stale. `LayerNode` carries `render_id: Option<RenderId>`
(`crates/flui-layer/src/tree/layer_tree.rs:38`), and the paint pass stamps it on the root and
on every repaint-boundary layer (`crates/flui-rendering/src/pipeline/owner/paint.rs:1186`,
`:1258`, `:1422`). What is still missing is the producer: nothing retains a boundary's subtree
across frames or compares it. The consumer is written: `DamageTracker` is crate-private in the
engine (`crates/flui-engine/src/damage.rs:22`) with `mark_dirty`, `mark_full_repaint` and
`damage_rect` (`damage.rs:38`, `:49`, `:62`), and `damage_scissor`
(`crates/flui-engine/benches/render_throughput.rs:282`) is the baseline a producer must beat.

## Decision

### 1. The raster contract lives in `flui-layer`

- `RasterBackend` (with its `Send` supertrait, ADR-0045 §1), `PresentDisposition` (unchanged,
  ADR-0068) and a new wgpu-free `RasterError` (`#[non_exhaustive]`) move to `flui-layer`.
  `render_scene` returns `Result<PresentDisposition, RasterError>`. `flui-engine` maps its
  `EngineError` into `RasterError` at the trait boundary and keeps `EngineError` for its own
  API.
- A `flui_layer::lower` module takes the GPU-free lowering: traversal order, the clip/opacity
  and save-layer discipline, effect decomposition into neutral steps, and the renderer and state
  stack traits that express them. It names no GPU type.
- The Command IR, `DrawBatcher`, `GpuReplay` and everything else in ADR-0006 §1-§4 and §6 stay in
  `flui-engine`. This is not the "separate IR crate" ADR-0006 rejected: the IR does not move, and
  the lowering that does move gains a second consumer (§2).
- `RasterOwner` stays in `flui-engine` through H0. Moving it out is a separate, optional
  decision, and is possible only once nothing in it names wgpu.

### 2. Two equal backends

- `flui-engine` (wgpu) and a new `flui-engine-cpu` both implement `flui_layer::RasterBackend` and
  consume `flui_layer::lower`. `flui-engine-cpu` sits in the render tier, kind internal,
  `publish = false` until the golden-image API is settled at B3, and must not reach `wgpu` in its
  normal dependency graph (tier R's reach fact, ADR-0081 §2: R's set forbids `wgpu`, and only
  `flui-engine` holds a `grant` for it).
- A conformance suite in `flui-layer` renders the same scenes through every backend. A behaviour
  one backend cannot provide is a named capability gap with a fallback, on the model of
  ADR-0057, never a silent approximation.
- `HeadlessRenderer` moves onto the shared lowering. Its three approximations become either
  conformance failures to fix or named gaps.
- The CPU backend serves golden images and GPU-less CI first and a no-adapter runtime fallback
  later (H2). Which CPU rasteriser it wraps is not decided here; the choice must be measured
  against the conformance suite, including effects and filters.

### 3. Retained layer identity is the damage producer

- A repaint boundary's layer subtree is retained across frames as an `Arc` subtree keyed by the
  boundary's `RenderId`, the stamp the paint pass already writes.
- A differ in `flui-layer` compares the new frame's boundary subtrees with the retained ones.
  Same `RenderId` and pointer-identical content (`Arc::ptr_eq`, ADR-0061's cheap comparison)
  contributes nothing. A changed, moved, added or removed subtree contributes the union of its
  old and new bounds in surface coordinates.
- The result is `DamageRegion::Partial(..)`, a new variant (`DamageRegion` is already
  `#[non_exhaustive]`). `Full` remains the fallback for the first frame, a resize or surface
  generation change, a root change, a subtree the differ cannot pair, and damage above a
  threshold.
- The raster lane stops constructing `Full` unconditionally. `DamageTracker` stays crate-private
  in the engine; multi-rect accumulation returns only together with a consumer that reads more
  than the bounding union, as ADR-0061's amendment requires.
- Damage has a real off switch: with it off, no subtree is retained, no differ runs and the cost
  per frame is the same as today.

### 4. The retained render target is conditional

wgpu does not expose swapchain buffer age (gfx-rs/wgpu#682, as cited by the review), so a partial repaint renders into a retained target
and blits it to the swapchain. The retained target is used only when damage is `Partial` and
below the threshold; a `Full` frame renders straight to the swapchain. The threshold, and the
bandwidth cost of the blit on tile-based mobile GPUs, are hypotheses to be measured before the
contract closes.

### 5. Timing

The contract closes before H3. Partial repaint is an exit criterion of B2. The work follows the
platform-contract and frame-transaction changes (ADR-0082, ADR-0083); ordering is in
[the migration plan](../plans/2026-09-25-architecture-migration-plan.md).

## Alternatives considered

- **Replace the wgpu engine with Vello.** Rejected: it would re-type the audited engine for a
  renderer with its own gaps, and does not give a GPU-less path.
- **A CPU mode inside `flui-engine` behind a feature.** Rejected: the crate links wgpu
  unconditionally, so the GPU-less CI build would still compile and link it, and a feature that
  switches rasterisers is a visibility toggle, not an additive feature.
- **Keep the headless GPU renderer as the golden path.** Rejected: it needs an adapter the test
  job does not have, and it already diverges from the windowed walk.
- **Derive damage from which render objects repainted.** Rejected by ADR-0061 and still wrong:
  the objects that always repaint cover the screen.
- **Always render through a retained target.** Rejected: it pays a full-surface blit on every
  `Full` frame, which is the common case during animation.
- **Make damage the first breaking change of H0.** Rejected on sequencing: it depends on stable
  layer identity and on the frame transaction owning the snapshot, and nothing in H0's exit needs
  it.

## Consequences

- `crates/flui-engine/ARCHITECTURE.md`'s introduction is rewritten, and a `## Mapping decisions`
  entry records the move of the contract and the lowering, in the change that performs it.
  `flui-layer`'s `ARCHITECTURE.md` gains the contract, the lowering and the differ.
- ADR-0061's decision stands and gains its producer; its "pairing does not exist" context is
  historical. ADR-0006 §5 stays true inside the engine (no device trait); the scene-level trait
  it did not forbid moves to `flui-layer`.
- **Breaks.** `flui_engine::{RasterBackend, PresentDisposition}` become
  `flui_layer::{RasterBackend, PresentDisposition}`; every implementation changes its error type
  to `RasterError` (one production impl, eight test doubles, the `raster_backpressure` bench and
  the `raster_backend_requires_send` compile-fail test). A match on `DamageRegion` must handle
  `Partial` or keep its wildcard arm.
- A new crate to maintain, gated by the conformance suite; the cost of every new layer or effect
  now includes its CPU lowering or a named gap.
- Retaining boundary subtrees holds the previous frame's layers alive one frame longer.

## Verification

None of these exist yet.

- `wgpu` is absent from the normal closures of `flui-layer` and `flui-engine-cpu`, stated by
  tier R's set in the ADR-0081 reach gate (§2), which only `flui-engine`'s grant excuses.
  `cargo tree -i` is not the
  check: it errors on an absent package.
- The `flui-layer` conformance suite passes on both backends, including `BackdropFilter`,
  `ShaderMask` and `Follower`, or lists each as a named gap.
- Differ unit tests: an unchanged boundary yields no damage; a changed boundary yields its
  bounds; a moved boundary yields the union of old and new bounds; an unpaired subtree yields
  `Full`.
- A readback test whose sample points distinguish correct partial damage from broken damage:
  move one boundary between frames and assert that its old position is cleared and pixels outside
  the damage are untouched. Damage computed from the new bounds only must fail it.
- A test that with damage switched off no subtree is retained and every frame sends `Full`.
- `damage_scissor` rerun against the producer, recorded against today's baseline.
