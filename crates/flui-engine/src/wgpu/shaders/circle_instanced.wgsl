// Instanced circle / ellipse shader for FLUI (SDF-based, full-affine transform).
//
// Renders circles and ellipses in a single draw call via GPU instancing.
// Each instance carries: a local center + radius (unit circle for the affine path),
// color, a full 2×3 affine (2×2 linear + translation), enabling correct SDF AA
// for rotated and scaled circles/ellipses without any fragment-shader change.
//
// ## Affine transform design
//
// The vertex shader applies a full 2×3 affine to an ORIGIN-CENTERED unit circle:
//
//   local_pos  = unit_pos * radius                  // unit_pos ∈ [-1,1]², origin-centered
//   M          = mat2x2(transform.xy, transform.zw) // column-major 2×2
//   device     = M * local_pos + transform_translate.xy
//
// The device CENTER lives in `transform_translate` (added after M), so M never
// scales it. center_radius.z is the radius; center_radius.xy is unused.
//
// For the affine path (circles/ellipses under rotation/shear):
//   center_radius = [0, 0, 1, 0]  (unit circle, radius folded into M)
//   transform     = M_world * diag(rx, ry)
//   transform_translate = M_world * center_local + t_world
//
// The SDF fragment evaluates `length(unit_pos) - 1.0` (0 at the unit-circle
// edge). `fwidth(dist)` measures the screen-space derivative of that local
// distance, yielding ~1-device-px AA under ANY affine (rotation/shear/
// anisotropic scale).
//
// ## Baked fast path (axis-aligned SrcOver circles)
//
//   center_radius       = [0, 0, radius, 0]   (xy unused; center is in translate)
//   transform           = diag(sx, sy)        (per-axis canvas scale)
//   transform_translate = [cx_dev, cy_dev, 0, 0]   (device-space center)
//   → device = diag(sx,sy) * (unit * radius) + center_dev
//   → scaled radius extent about the device center (matches the pre-affine path,
//     which added center directly and scaled only the radius displacement).
//
// ## AA fringe quad expansion
//
// The quad is expanded ~1.5 device-px outward so the outer AA fringe rasterizes.
// Margin is computed in LOCAL units = 1.5 / column_length (mirroring rect_instanced).
// The SDF distance is still measured to the true unit circle regardless of expansion.
//
// ## AA quality fix
//
// Previous shader used `edge_softness = 0.02` (radius-RELATIVE): a 200px circle
// got 4px AA; a 5px circle got 0.1px. The new `fwidth`-based model gives exactly
// ~1 device-px AA at ANY radius and any affine — the same model as rect_instanced.

// Vertex input (shared unit quad: [0,0] to [1,1])
struct VertexInput {
    @location(0) position: vec2<f32>,  // Quad corner [0 to 1]
}

// Instance input (per-circle / per-ellipse data)
struct InstanceInput {
    @location(2) center_radius: vec4<f32>,      // [cx, cy, radius, _padding]
    @location(3) color: vec4<f32>,              // [r, g, b, a] in 0-1 range
    @location(4) transform: vec4<f32>,          // 2×2 linear affine col-major: [a, b, c, d]
    @location(5) transform_translate: vec4<f32>,// [tx, ty, 0, 0] — translation part
}

// Vertex output / Fragment input
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    // Unit-circle coordinate passed from the vertex shader to the fragment.
    // For the unit-circle local shape: `unit_pos = (local_pos - center) / radius`.
    // The SDF evaluates `length(unit_pos) - 1.0`; fwidth gives correct screen-space
    // AA width regardless of the affine applied in the vertex shader.
    @location(1) unit_pos: vec2<f32>,
}

// Viewport uniform (for screen-space to clip-space conversion)
struct Viewport {
    size: vec2<f32>,      // Viewport size in pixels
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

    // Map unit quad [0,0]–[1,1] to the unit-circle local space [-1,1]²,
    // EXPANDED outward by a ~1.5 device-px AA fringe.
    //
    // The unit quad maps to [-1, 1] × [-1, 1] in the un-expanded case:
    //   unit_pos = vertex.position * 2.0 - 1.0
    //
    // The SDF anti-aliases the edge over a ~1 device-px band that straddles the
    // geometric boundary (inside AND outside). Expanding the quad gives fringe
    // pixels fragments for the outer half of the AA band — essential for diagonal
    // and rotated edges. The SDF distance is still measured to the true unit
    // circle regardless of expansion.
    //
    // Margin in LOCAL units = 1.5 / (radius * column_length), so the fringe is
    // ~1.5 device-px at any zoom/rotation/scale. Column lengths of the 2×2 give
    // the local→device scale per axis (|M * e_x| = |col0|, |M * e_y| = |col1|).
    // The `radius` factor converts from unit-circle local space to world-local space.
    let fringe_device = 1.5;
    let col0_len = length(instance.transform.xy); // |x-column| → device scale of local-x
    let col1_len = length(instance.transform.zw); // |y-column| → device scale of local-y
    // margin_unit: how much to expand the unit-circle quad in unit-circle coordinates.
    // fringe_device / (radius * col_len) = fringe_dev_px / dev_px_per_unit.
    let r_safe = max(radius, 1e-6);
    let margin_unit = vec2<f32>(
        fringe_device / max(col0_len * r_safe, 1e-6),
        fringe_device / max(col1_len * r_safe, 1e-6),
    );
    // Expand the [-1,1] quad to [-(1+e), 1+e].
    let base_unit = vertex.position * 2.0 - 1.0; // [-1, 1]
    let exp_unit  = base_unit * (1.0 + margin_unit); // [-(1+e), 1+e]

    // Local-space position: the expanded unit coord scaled by radius, ORIGIN-
    // centered. The device CENTER is carried entirely in `transform_translate`
    // (added AFTER M below), so it is never scaled by M. Putting the center
    // inside `local` here would let M scale it — the baked-path double-scale bug
    // (a circle on a scaled/HiDPI CTM rendered its center at scale × position).
    let local_pos = exp_unit * r_safe;

    // Apply the full 2×3 affine: device = M * local + t.
    //
    // transform = [a, b, c, d] is the 2×2 linear part column-major:
    //   x-column = (a, b), y-column = (c, d).
    // So: M * p = (a*p.x + c*p.y,  b*p.x + d*p.y).
    //
    // Baked fast path: M = diag(sx, sy), local = unit*radius, t = center_dev
    //   → device = diag(sx,sy)*(unit*r) + center_dev — scaled radius extent about
    //     the device center (matches the pre-affine circle path exactly).
    // Affine path: M = M_world*diag(rx,ry), radius = 1, t = M_world*center + t_world
    //   → device = oriented ellipse about the device center.
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
    // Pass the EXPANDED unit coord to the fragment so the SDF evaluates
    // `length(unit_pos) - 1.0` relative to the true unit-circle edge,
    // even on the expanded fringe (exp_unit ranges slightly outside [-1,1] there).
    out.unit_pos = exp_unit;

    return out;
}

// =============================================================================
// Fragment Shader (SDF-based with screen-space fwidth AA)
// =============================================================================

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Signed distance to the unit-circle edge:
    //   d < 0 → inside the circle
    //   d = 0 → exactly on the edge
    //   d > 0 → outside the circle
    //
    // `length(unit_pos) - 1.0` is the exact SDF of the unit circle in the
    // coordinate space passed from the vertex shader. Because the vertex shader
    // maps local→device via the full affine M (which is M_world * diag(rx,ry)),
    // `fwidth(d)` measures the screen-space gradient of this distance, giving
    // ~1 device-px AA at ANY radius, rotation, or anisotropic scale — correct
    // for circles, rotated circles, and oriented ellipses.
    let d = length(in.unit_pos) - 1.0;

    // Adaptive AA: half-pixel-width in local SDF units, derived from fwidth.
    // smoothstep maps [-aa, aa] → [1, 0]: fragments inside are fully opaque,
    // outside fully transparent, and the boundary band is anti-aliased.
    let aa = fwidth(d) * 0.5;
    let alpha = 1.0 - smoothstep(-aa, aa, d);

    // Discard fully transparent pixels to reduce overdraw on expanded fringe.
    if (alpha < 0.001) {
        discard;
    }

    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}
