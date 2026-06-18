// Instanced arc shader for FLUI (SDF-based, full-affine transform).
//
// Renders arcs (pie-slice sectors of a unit circle) in a single draw call via
// GPU instancing. Each instance carries: a 2×3 affine (2×2 linear + device
// center in the translation), start/sweep angles, and color.
//
// ## Affine transform design (mirrors circle_instanced.wgsl)
//
// The vertex shader applies a full 2×3 affine to an ORIGIN-CENTERED unit disk:
//
//   local_pos  = unit_pos * radius                  // unit_pos ∈ [-1,1]², origin-centered
//   M          = mat2x2(transform.xy, transform.zw) // column-major 2×2
//   device     = M * local_pos + transform_translate.xy
//
// The device CENTER lives in `transform_translate` (added after M), so M never
// scales it — same contract as circle_instanced (the PR-2 double-scale fix).
//
// For the baked fast path (axis-aligned SrcOver arcs):
//   center_radius       = [0, 0, radius, 0]
//   transform           = diag(sx, sy)
//   transform_translate = [cx_dev, cy_dev, 0, 0]
//   → device = diag(sx,sy) * (unit*radius) + center_dev
//
// For the affine path (rotated SrcOver arcs):
//   center_radius       = [0, 0, 1, 0]   (unit circle, radius folded into M)
//   transform           = M_world * r     (col-major [a,b,c,d])
//   transform_translate = M_world * center_local + t_world
//
// ## Radial AA (fwidth — radius-independent)
//
// Previous shader used `edge_softness = 0.02` (radius-RELATIVE — wrong).
// New model: `d_r = length(unit_pos) - 1.0`, `aa_r = fwidth(d_r) * 0.5`.
// `fwidth(d_r)` measures the screen-space derivative of the unit-circle SDF,
// yielding ~1 device-px AA at ANY radius, rotation, or anisotropic scale.
//
// ## Angular AA (screen-space fwidth — resolution-independent)
//
// Previous shader used `angle_softness = 0.05` rad (fixed ~3° — wrong at any
// but one radius). New model uses an SDF of the two radial half-planes that
// bound the sector:
//
// The signed distance to each bounding half-plane is the 2D cross product of
// the edge ray with the query point (positive on the swept/inside side):
//
//   start_dir = (cos(start), sin(start))   [the ray at the start edge]
//   The inside of the sector is on the swept side of start_dir, where
//   cross(start_dir, unit_pos) ≥ 0:
//   d_start = cross(start_dir, unit_pos)
//           = start_dir.x*unit_pos.y - start_dir.y*unit_pos.x
//           = cos(start)*py - sin(start)*px
//
//   The sector ends at end_angle; the point must be on the opposite (right) side
//   of end_dir to still be inside:
//   d_end = -cross(end_dir, unit_pos) = sin(end)*px - cos(end)*py
//
// For a positive sweep (sweep > 0): inside = d_start ≥ 0 AND d_end ≥ 0
//   → angular_sdf = min(d_start, d_end)   (intersection of two half-planes)
//
// For sweep ≥ 2π (full circle): no angular cut — degrade to purely radial.
//
// For a sweep > π (the arc is the MAJORITY half — MORE than a semicircle):
//   The two half-planes' INTERSECTION is no longer the correct inside region;
//   the inside is actually the UNION: angular_sdf = max(d_start, d_end).
//   This is because for a 270° arc, a point at angle 180° is inside both the
//   "swept side of start" and "swept side of end" individually, so max works.
//
// For negative sweep (counter-clockwise): flip sign of both d_start and d_end.
//
// `fwidth` of the resulting angular_sdf gives screen-space ~1 device-px AA
// at the sector edges at ANY radius or scale.
//
// ## Outer-fringe quad expansion
//
// The quad is expanded ~1.5 device-px outward so the outer AA fringe rasterizes.
// Margin is computed in LOCAL units = 1.5 / (radius * column_length) — same
// formula as circle_instanced.
//
// ## AA model improvement over previous arc shader
//
// - Radial: `edge_softness=0.02` (radius-relative) → `fwidth(dist)` (radius-independent).
// - Angular: `angle_softness=0.05` rad (fixed ~3°) → `fwidth(angular_sdf)` (~1px device).
// - No fixed `angle_softness` threshold remains.
// - The `in_arc` boolean + angular smoothstep is replaced by a signed-distance
//   approach that computes continuous partial-alpha at the sector edges.

// ────────────────────────────────────────────────────────────────────────────

struct VertexInput {
    @location(0) position: vec2<f32>,  // Quad corner [0 to 1]
}

struct InstanceInput {
    @location(2) center_radius: vec4<f32>,      // [0, 0, radius, 0]
    @location(3) angles: vec4<f32>,             // [start_angle, sweep_angle, 0, 0]
    @location(4) color: vec4<f32>,              // [r, g, b, a] in 0-1 range
    @location(5) transform: vec4<f32>,          // 2×2 linear affine col-major: [a, b, c, d]
    @location(6) transform_translate: vec4<f32>,// [tx, ty, 0, 0] — device center
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    // Unit-disk coordinate: `unit_pos = expanded_unit_quad * 2 - 1`, scaled by
    // the fringe expansion. SDF evaluates `length(unit_pos) - 1.0`. Passed to
    // the fragment so `fwidth` measures the screen-space derivative correctly.
    @location(1) unit_pos: vec2<f32>,
    @location(2) start_angle: f32,
    @location(3) sweep_angle: f32,
}

struct Viewport {
    size: vec2<f32>,
    _padding: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> viewport: Viewport;

// =============================================================================
// Vertex Shader
// =============================================================================

@vertex
fn vs_main(
    vertex: VertexInput,
    instance: InstanceInput,
) -> VertexOutput {
    var out: VertexOutput;

    let radius = instance.center_radius.z;

    // Map the unit quad [0,0]–[1,1] to the unit-disk local space [-1,1]²,
    // expanded outward by a ~1.5 device-px AA fringe — mirrors circle_instanced.
    //
    // Margin in LOCAL units = 1.5 / (radius * column_length), so the fringe
    // stays ~1.5 device-px at any zoom / scale.
    let fringe_device = 1.5;
    let col0_len = length(instance.transform.xy); // |x-column|
    let col1_len = length(instance.transform.zw); // |y-column|
    let r_safe = max(radius, 1e-6);
    let margin_unit = vec2<f32>(
        fringe_device / max(col0_len * r_safe, 1e-6),
        fringe_device / max(col1_len * r_safe, 1e-6),
    );
    // Expand the [-1,1] quad to [-(1+e), 1+e].
    let base_unit = vertex.position * 2.0 - 1.0; // [-1, 1]
    let exp_unit  = base_unit * (1.0 + margin_unit); // [-(1+e), 1+e]

    // Local-space position: the expanded unit coord scaled by radius,
    // ORIGIN-centered. The device CENTER is in `transform_translate` (added
    // AFTER M below) so it is never scaled by M.
    let local_pos = exp_unit * r_safe;

    // Apply the full 2×3 affine: device = M * local + t.
    // transform = [a, b, c, d] column-major: x-col=(a,b), y-col=(c,d).
    let m = mat2x2<f32>(
        instance.transform.xy,  // x-column (a, b)
        instance.transform.zw,  // y-column (c, d)
    );
    let device_pos = m * local_pos + instance.transform_translate.xy;

    // Convert device pixels to clip space [-1, 1].
    let clip_x = (device_pos.x / viewport.size.x) * 2.0 - 1.0;
    let clip_y = 1.0 - (device_pos.y / viewport.size.y) * 2.0; // flip Y

    out.position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    out.color = instance.color;
    // Pass the EXPANDED unit coord to the fragment. The SDF evaluates
    // `length(unit_pos) - 1.0` relative to the true unit-circle edge
    // even on the expanded fringe.
    out.unit_pos    = exp_unit;
    out.start_angle = instance.angles.x;
    out.sweep_angle = instance.angles.y;

    return out;
}

// =============================================================================
// Fragment Shader
// =============================================================================

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let unit_pos = in.unit_pos;
    let start    = in.start_angle;
    let sweep    = in.sweep_angle;

    // ── Radial SDF (fwidth-based, radius-independent) ─────────────────────
    //
    // Signed distance to the unit-circle edge:
    //   d < 0 → inside the circle
    //   d = 0 → on the edge
    //   d > 0 → outside
    // `fwidth(d)` gives ~1 device-px AA at ANY radius, rotation, or scale.
    let d_radial = length(unit_pos) - 1.0;
    let aa_radial = fwidth(d_radial) * 0.5;
    let radial_alpha = 1.0 - smoothstep(-aa_radial, aa_radial, d_radial);

    // Discard pixels fully outside the circle (fringe expansion safety).
    if radial_alpha < 0.001 {
        discard;
    }

    // ── Angular SDF (screen-space fwidth, sector half-planes) ────────────
    //
    // Full-circle shortcut: |sweep| ≥ 2π → no angular cut, pure circle.
    let tau = 6.28318530718; // 2π
    let abs_sweep = abs(sweep);
    if abs_sweep >= tau {
        return vec4<f32>(in.color.rgb, in.color.a * radial_alpha);
    }

    // Angular SDF design (positive sweep, screen Y-down):
    //
    //   start_dir = (cos(start), sin(start))    [the ray at the start edge]
    //   end_dir   = (cos(end), sin(end))         where end = start + sweep
    //
    // We want d_start ≥ 0 for points on the swept (inside) side of the start ray.
    // Using the 2D cross product of start_dir with unit_pos:
    //   cross(start_dir, P) = cos(start)*P.y - sin(start)*P.x
    // Positive cross = P is to the LEFT of start_dir.
    // For a positive-sweep (CW in screen coords) arc the inside is to the
    // LEFT of start → d_start = cos(start)*P.y - sin(start)*P.x.
    //
    // For the end edge the inside is to the RIGHT:
    //   d_end = -(cos(end)*P.y - sin(end)*P.x) = sin(end)*P.x - cos(end)*P.y
    //
    // For |sweep| ≤ π the inside is the INTERSECTION (both d ≥ 0):
    //   angular_sdf = min(d_start, d_end)
    //
    // For |sweep| > π the inside is the UNION (at least one d ≥ 0):
    //   angular_sdf = max(d_start, d_end)
    //
    // For negative sweep (CCW): flip both — equivalently negate unit_pos's
    // angular frame. We handle this by swapping start/end when sweep < 0
    // (equivalent to reflecting the problem into the positive-sweep case).
    let end_angle = start + sweep;

    // For negative sweep, treat as a positive-sweep arc from end→start.
    var a0 = start;
    var a1 = end_angle;
    if sweep < 0.0 {
        a0 = end_angle;
        a1 = start;
    }

    // Signed distances from each bounding half-plane.
    // d0 ≥ 0 → point is on the swept side of the a0 edge.
    // d1 ≥ 0 → point is on the swept side of the a1 edge.
    let d_start_hp = cos(a0) * unit_pos.y - sin(a0) * unit_pos.x;
    let d_end_hp   = sin(a1) * unit_pos.x - cos(a1) * unit_pos.y;

    // Effective positive sweep angle (already normalized to ≥ 0 by the swap above).
    let pos_sweep = abs_sweep;

    var angular_sdf: f32;
    if pos_sweep <= 3.14159265358979 {
        // ≤ 180°: sector is the INTERSECTION of both half-planes.
        angular_sdf = min(d_start_hp, d_end_hp);
    } else {
        // > 180°: sector is the UNION (either half-plane suffices).
        angular_sdf = max(d_start_hp, d_end_hp);
    }

    // Screen-space AA width for the angular edge: ~1 device-px.
    let aa_angular = fwidth(angular_sdf) * 0.5;
    let angular_alpha = smoothstep(-aa_angular, aa_angular, angular_sdf);

    // Discard pixels fully outside the angular sector.
    if angular_alpha < 0.001 {
        discard;
    }

    // ── Combine ───────────────────────────────────────────────────────────
    let alpha = radial_alpha * angular_alpha;
    if alpha < 0.001 {
        discard;
    }

    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}
