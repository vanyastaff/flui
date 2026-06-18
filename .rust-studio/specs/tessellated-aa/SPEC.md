# SPEC — Engine-wide anti-aliasing for tessellated geometry (flui-engine)

Status: **APPROVED-PENDING** (design settled via design-panel + 2 adversarial reshapes + user scope decisions). Successor effort to Phase B (advanced dst-read blend, #251–257).

## Problem

SDF instanced primitives (axis-aligned rect/rrect/circle/arc, SrcOver) already AA in-shader
via `fwidth`/`sdfToAlpha` (`shaders/common/sdf.wgsl:145`). Everything else routes to lyon
tessellation and renders with **no AA at `sample_count: 1`** → aliased edges. This is a documented
engine-wide "Phase-A quality limit" (`batches/shapes.rs:71,93,197,322`), not a Phase-B defect.

The aliased-tessellated set: (a) rotated/skewed/transformed basic shapes (rect/rrect/circle/ellipse),
(b) non-SrcOver basic shapes (Porter-Duff + advanced blend), (c) arbitrary paths / polygons / béziers /
complex strokes.

## Rejected approaches (with reasons — do not revisit without new facts)

- **Surface MSAA** — regresses the just-shipped Phase B (single-sample surface backdrop-copy in
  `advanced_blend/mod.rs`, single-sample pooled offscreen in `opacity_layer.rs`, REPLACE composite);
  4× memory/bandwidth; redundant for SDF primitives that already AA via `fwidth`.
- **Analytic feather-strip for fills** — lyon_tessellation 1.0.20 `FillVertex` has **no `normal()`**
  (only `position()/sources()/as_endpoint_id()/interpolated_attributes()`), so the fringe would be a
  contour stroke = premul double-blend; plus concave/self-intersect over-coverage. Wrong tool.
- **Stencil-then-cover** — solves *GPU* winding resolution for raw paths; flui already CPU-triangulates
  via lyon `FillTessellator` (watertight, winding-correct NonZero/EvenOdd, non-overlapping). The stencil
  pass re-solves an already-solved problem; its own AA still came from an isolated MSAA offscreen. Both
  adversarial reviewers independently converged on "supersample the existing triangulation instead".
  Greenfield depth-stencil + pool rewrite for no benefit in a CPU-tessellation architecture.

## Chosen architecture (two halves)

### Half 1 — SDF-affine reroute (transformed basic shapes, SrcOver)

Currently the instanced fast path is gated on `is_axis_aligned() && blend==SrcOver`
(`batches/shapes.rs:75`) because the instance `transform` is `[scale_x, scale_y, tx, ty]` —
scale+translate only, **no rotation/skew** (`rect_instanced.wgsl:26`). Extend the instance to a full
2×3 affine; the vertex shader applies it; the fragment evaluates the SDF in **local space** via the
inverse transform. `fwidth` stays correct (it measures screen-space derivative of the transformed
distance → ~1-device-px AA even under skew/anisotropy). Reuse existing `sdRoundedBox`,
`sdRoundedSuperellipse`, `sdEllipse`, `sdOrientedBox`. Reroute rotated/skewed SrcOver
rect/rrect/circle/ellipse/arc from tessellation → instanced affine-SDF. **Exact analytic AA, cheap,
covers the dominant real-world case (transformed cards/buttons), zero Phase-B coupling.**

### Half 2 — SSAA offscreen tile (arbitrary paths + non-SrcOver basic shapes)

For geometry with no closed-form SDF (custom paths, polygons, béziers, strokes) and for non-SrcOver
basic shapes: render lyon's **existing** triangulation into an **isolated NxN supersampled** pooled
offscreen (just a larger `{w,h}` texture — the pool already keys on `{w,h,format}`; **no multisampled
texture, no stencil, no pool rewrite**), box-filter downsample (reuse the PR-6 `blit.wgsl` infra into a
downsample variant), producing a resolved **premultiplied color tile**. Composite that tile via the
**existing** `DrawItem::OffscreenTexture` (SrcOver/Porter-Duff) or `DrawItem::AdvancedShape`
(advanced dst-read) seam at the correct Z. **Supersampling is unconditionally correct on all
topologies** (convex/concave/self-intersecting/holes). Surface stays `sample_count: 1` → Phase-B-safe.

## Routing decision tree (no producer left permanently aliased)

| shape × transform × blend | route |
|---|---|
| basic shape, axis-aligned, SrcOver | existing instanced SDF (unchanged, byte-identical) |
| basic shape, rotated/skewed, SrcOver | **NEW** instanced affine-SDF (half 1) |
| basic shape, any transform, non-SrcOver (Porter-Duff/advanced) | **NEW** SSAA tile (half 2) → OffscreenTexture/AdvancedShape |
| arbitrary path/polygon/stroke, SrcOver | **NEW** SSAA tile (half 2) → OffscreenTexture |
| arbitrary path/polygon/stroke, non-SrcOver | **NEW** SSAA tile (half 2) → OffscreenTexture(blend)/AdvancedShape |

Document any genuine exception (e.g. `Plus`/`Modulate`) explicitly; never silent.

## The AA oracle (gating blocker — define BEFORE relying on it)

A green render test that only checks "AA present" is vacuous (Phase-B lesson:
adversarial-soundness-catches-vacuous-gates). Define two real oracles in PR-1 and **calibrate them
against the existing SDF primitives** (which are known-correct AA) before using them to gate new code:

1. **CPU analytic area-coverage** — for a known edge (45° line, circle arc) compute exact fractional
   pixel coverage; assert rendered alpha within tolerance.
2. **Tessellated-vs-SDF edge-consistency** — render the same circle/rect via the SDF path AND via the
   new path; assert boundary pixels match within tolerance. This *is* the literal goal.

Plus: interior byte-identity, monotonic coverage ramp across the boundary band, topology correctness
(pentagram NonZero=solid vs EvenOdd=hole, both AA'd), anti-MVP "all tessellated producers emit ≥1
partial-alpha boundary pixel".

## PR sequence (each independently `just ci`-green; axis-aligned SrcOver byte-identical throughout)

1. **PR-1** — AA oracle harness + calibration vs existing SDF; **instanced rect/rrect → full affine +
   local-space SDF**; reroute rotated/skewed SrcOver rect/rrect. GPU-readback gated by oracle.
2. **PR-2** — **instanced circle/ellipse/arc → affine**; reroute rotated SrcOver circle/ellipse/arc.
3. **PR-3** — **SSAA infra** (pool NxN request + downsample blit + render-tess-to-SSAA-tile → premul
   tile → `DrawItem::OffscreenTexture`); route SrcOver arbitrary paths/polygons. Edge-consistency +
   pentagram NonZero/EvenOdd topology gates.
4. **PR-4** — route **non-SrcOver basic shapes + Porter-Duff/advanced arbitrary paths** through the
   SSAA tile (compose via OffscreenTexture blend / AdvancedShape). Anti-MVP all-producers guard;
   document exceptions.
5. **PR-5 (conditional)** — batching multiple paths into shared tiles; thin-shape (<1px) handling —
   only if readback/profiling shows need.

## Maintainer conditions (all must hold to claim done)

1. Surface stays `sample_count: 1`; **no `resolve_target: Some` on the surface, no `DepthStencilState`
   anywhere** (SSAA uses normal larger textures, not MSAA/stencil). CI grep guard.
2. Existing axis-aligned SrcOver byte-identical (instanced path unchanged for that case; local GPU golden).
3. AA oracle defined + calibrated against existing SDF in PR-1 before gating any new behavior.
4. Tessellated-vs-SDF edge-consistency passes (rotated rect ~ axis rect; circle-via-path ~ circle-via-SDF).
5. Topology: pentagram NonZero (solid) vs EvenOdd (hole) both correctly AA'd.
6. No real producer permanently aliased (anti-MVP test); explicit documented exceptions only.
7. Phase B untouched except additively (`advanced_blend/`, `opacity_layer.rs` core, `replay.rs`); Phase
   B GPU-readback suite stays green.
8. `fwidth` correctness under non-uniform scale verified (coverage-ramp at 1× and 8×).
9. Every touched non-test file < 1500 LOC; split modules as needed (tessellator.rs already ~1520 incl tests).

## Test / verification

GPU-readback local DX12 only (`--features enable-wgpu-tests -- --test-threads 1`), not CI — naming the
GPU, red→green (assertions fail without the change). `just ci` green per PR. Phase B suite green.
Optional `examples/` AA demo (rotated shapes + a star path) for visual confirmation beyond unit pixels.

## Review discipline (per PR)

`rust-builder` (test-first) → `rust-reviewer` + **`ce-kieran`** (soundness, mandatory on this core) →
**I run the GPU-readback serial on DX12 myself** (the only pixel gate; not in CI). Capture the durable
decision to the Obsidian vault via `/remember` after the architecture lands.

---

## Progress (as of 2026-06-18)

- **PR-1 MERGED** (#258, `6fd8ae0b`): rect/rrect affine-SDF reroute + AA oracle harness (O1–O7).
  Caught/fixed in review: missing outer AA fringe (quad-expansion fix), pre-existing `sdRoundedBox`
  corner-radius scramble (non-uniform radii), doc-CI.
- **PR-2 MERGED** (#259, `2d3d82ac`): circle/oval affine + `fwidth` AA-model fix (replaced
  radius-relative `edge_softness=0.02`). Caught/fixed: BLOCKER double-scale of the circle center
  (HiDPI/DPR>1 broke) — center now in `transform_translate`, never inside the local vector; C4 guards it.
- **PR-2b MERGED** (#260, `b9f2bf28`): arc affine + `fwidth` radial AA + screen-space angular
  half-plane SDF. Caught/fixed: doc-CI break (stale `[ArcInstance::new]` links), `use_center=false`
  shape regression (chord segments now tessellate), elliptical-arc collapse (now folds `diag(rx,ry)`).

**Half 1 (SDF-reroute for all closed-form shapes) is COMPLETE.** Every rect/rrect/circle/oval/arc
SrcOver shape now anti-aliases correctly under any affine, at any radius/scale/rotation.

**Deferred quality follow-up (own PR, engine-wide):** switch `sdfToAlpha` from `fwidth` (L1/Manhattan,
over-widens diagonal-edge AA by up to √2) to `length(vec2(dpdx,dpdy))` (L2) across ALL 5 SDF shader
copies (rect_instanced, circle_instanced, arc_instanced, common/sdf, gradients). Changes existing
axis-aligned output by ≤1/255 at corners — a deliberate quality improvement, not byte-identical.

## PR-3 implementation map (SSAA offscreen for arbitrary paths) — READY TO BUILD

Scouted; integration points confirmed:

- **Route**: `painter.rs:866 draw_path` → `batches/paths.rs:181 DrawBatcher::draw_path` →
  `batches/mod.rs:153 add_tessellated_with_key`. Add an SSAA diversion **parallel to the AdvancedShape
  diversion** (`mod.rs:165-207`): when `paint.style==Fill && blend==SrcOver` AND the geometry is an
  **arbitrary path/polygon** (NOT a closed-form shape already SDF-routed), seal the prior segment
  (`finish_current_segment`), render the path's `DrawSegment` to a 2× pooled offscreen, box-downsample
  to a 1× premultiplied tile, and push `DrawItem::OffscreenTexture(PendingOffscreenTexture{texture,bounds})`.
- **Reuse `DrawItem::OffscreenTexture`** (`command_ir.rs:62,295`) verbatim — no new variant. It composites
  via `replay.rs:286` → `flush_texture_batch_premultiplied` (`replay.rs:1434`). Z-order is implicit in
  `draw_order` position (R1 arm order), so seal+push is Z-correct (same as AdvancedShape).
- **Offscreen render**: reuse `opacity_layer.rs:60 render_segment_to_offscreen` to draw the segment into
  a `pool.acquire(vp_w*N, vp_h*N, surface_format)` 2× texture (existing usage flags suffice — no new axis).
- **Downsample**: NEW `shaders/effects/box_downsample.wgsl` (~20 LOC, 4-tap linear box filter 2×→1×) +
  a small pipeline; `blit.wgsl` is 1:1 nearest, not reusable as-is. Output a 1× premultiplied tile.
- **Does NOT need the surface** (unlike advanced blend) → runs at record time in `DrawBatcher` (it has
  the resources pool), no renderer-layer driver needed.
- **Anti-MVP**: gate strictly on arbitrary-path SrcOver; assert no closed-form shape is mis-routed here
  (they stay SDF). A grep/structural guard that the SSAA arm is reached only by paths/polygons.
- **Tests** (local DX12): P1 polygon fill boundary vs analytic oracle; P2 self-intersecting/holed path
  (pentagram NonZero vs EvenOdd) — supersampling must be correct on all topologies; P3 scale-invariance
  (1× vs 8×); P4 anti-MVP all-arbitrary-paths-AA; reuse the `aa_oracle_tests.rs` harness.
- **Surprises**: `OffscreenTexture` carries only `{texture, bounds}` (Z implicit) → path-tile rides it
  verbatim. SSAA 2× tile is a distinct pool key (no reuse with 1× tiles) — acceptable; note in benches.

## PR-4 (after PR-3): non-SrcOver + Porter-Duff/advanced arbitrary paths
Route non-SrcOver basic shapes + Porter-Duff/advanced arbitrary paths through the SSAA tile, composing
via `OffscreenTexture`(blend) / `AdvancedShape`. Final anti-MVP guard: no real producer permanently
aliased; document explicit exceptions (e.g. Plus/Modulate).
