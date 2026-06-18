// Instanced rectangle shader for FLUI (SDF-based, full-affine transform).
//
// Renders multiple rectangles in a single draw call using GPU instancing.
// Each instance carries: local bounds, color, corner radii, a full 2×3 affine
// (2×2 linear + translation), and an optional SDF clip rrect.
//
// ## Affine transform design
//
// The vertex shader applies a full 2×3 affine in LOCAL space, enabling correct
// SDF anti-aliasing for rotated and skewed rects without any fragment-shader
// change:
//
//   local_pos = vertex.position * bounds.zw + bounds.xy
//   M         = mat2x2(transform.xy, transform.zw)   // column-major 2×2
//   device    = M * local_pos + transform_translate.xy
//
// The SDF fragment evaluates distance in LOCAL space (via `uv` / `rect_size`);
// `fwidth(dist)` measures the screen-space derivative of that local distance,
// yielding ~1-device-px AA under ANY affine (rotation/skew/anisotropic scale)
// with no fragment-shader change.
//
// For the baked-AABB fast path (axis-aligned SrcOver):
//   transform           = [1, 0, 0, 1]   (identity 2×2)
//   transform_translate = [0, 0, 0, 0]
//   device = identity * local + 0 = local  → byte-identical output.
//
// Performance:
// - 30-40% faster fragment shader (branchless SDF)
// - Adaptive antialiasing via fwidth() — correct at any zoom AND under rotation
// - SDF-based rounded rect clipping (no stencil buffer needed)

// Vertex input (shared unit quad: [0,0] to [1,1])
struct VertexInput {
    @location(0) position: vec2<f32>,  // Quad corner [0-1, 0-1]
}

// Instance input (per-rectangle data)
struct InstanceInput {
    @location(2) bounds: vec4<f32>,              // [x_local, y_local, width_local, height_local]
    @location(3) color: vec4<f32>,               // [r, g, b, a] in 0-1 range
    @location(4) corner_radii: vec4<f32>,        // [tl, tr, br, bl]
    @location(5) transform: vec4<f32>,           // 2×2 linear affine col-major: [a, b, c, d]
    @location(6) clip_bounds: vec4<f32>,         // [x, y, width, height] of clip
    @location(7) clip_radii: vec4<f32>,          // [tl, tr, br, bl] of clip
    @location(8) clip_kind: vec4<u32>,           // [kind, _, _, _]: 0=none, 1=rrect, 2=rsuperellipse
    @location(9) transform_translate: vec4<f32>, // [tx, ty, 0, 0] — translation part of affine
}

// Vertex output / Fragment input
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,              // Local UV coordinates [0-1]
    @location(2) rect_size: vec2<f32>,       // Rectangle size for SDF calculation
    @location(3) corner_radii: vec4<f32>,    // Corner radii for SDF
    @location(4) world_pos: vec2<f32>,       // World-space position for clip SDF
    @location(5) clip_bounds: vec4<f32>,     // Clip rect bounds
    @location(6) clip_radii: vec4<f32>,      // Clip corner radii (single-radius-per-corner)
    @location(7) @interpolate(flat) clip_kind: u32, // 0=none, 1=rrect, 2=rsuperellipse
}

// Viewport uniform (for screen-space to clip-space conversion)
struct Viewport {
    size: vec2<f32>,      // Viewport size in pixels
    _padding: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> viewport: Viewport;

// =============================================================================
// SDF Functions (inline for this shader, can be extracted to common library)
// =============================================================================

/// Rounded box SDF with per-corner radii
/// p: point to test (centered at origin)
/// b: half-extents (half width, half height)
/// r: corner radii [top-left, top-right, bottom-right, bottom-left]
fn sdRoundedBox(p: vec2<f32>, b: vec2<f32>, r: vec4<f32>) -> f32 {
    // Select radius based on quadrant (branchless!)
    // r2 = (top, bottom) radii for the active horizontal side:
    //   right (p.x>0) → (tr=r.y, br=r.z); left → (tl=r.x, bl=r.w).
    let r2 = select(vec2<f32>(r.x, r.w), vec2<f32>(r.y, r.z), p.x > 0.0);
    // r3 = bottom (p.y>0) → r2.y; top → r2.x.
    let r3 = select(r2.x, r2.y, p.y > 0.0);

    let q = abs(p) - b + vec2<f32>(r3);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0))) - r3;
}

/// Rounded superellipse SDF (iOS-squircle, n=4) with per-corner radii.
///
/// Mirrors `sdRoundedSuperellipse` from `common/sdf.wgsl`; inlined here per
/// the existing `sdRoundedBox` inlining convention. See the common-library
/// version for full prose.
fn sdRoundedSuperellipse(p: vec2<f32>, b: vec2<f32>, r: vec4<f32>) -> f32 {
    // (top, bottom) radii for the active side — see sdRoundedBox.
    let r2 = select(vec2<f32>(r.x, r.w), vec2<f32>(r.y, r.z), p.x > 0.0);
    let r3 = select(r2.x, r2.y, p.y > 0.0);

    let q = abs(p) - b + vec2<f32>(r3);

    if (q.x < 0.0 && q.y < 0.0) {
        return max(q.x, q.y) - r3;
    }

    if (r3 <= 0.0) {
        return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0)));
    }

    let ax = max(q.x, 0.0) / r3;
    let ay = max(q.y, 0.0) / r3;
    let n_norm = sqrt(sqrt(ax * ax * ax * ax + ay * ay * ay * ay));
    return (n_norm - 1.0) * r3;
}

/// Convert SDF distance to alpha with adaptive antialiasing
fn sdfToAlpha(dist: f32) -> f32 {
    // fwidth gives us screen-space gradient for resolution-independent AA
    let edge_width = fwidth(dist) * 0.5;
    return 1.0 - smoothstep(-edge_width, edge_width, dist);
}

// =============================================================================
// Vertex Shader
// =============================================================================

@vertex
fn vs_main(
    vertex: VertexInput,
    instance: InstanceInput,
) -> VertexOutput {
    var out: VertexOutput;

    // Map unit quad [0,0]–[1,1] to local shape space, EXPANDED outward by a
    // ~1.5 device-px AA fringe.
    //
    // The SDF anti-aliases the edge over a ~1 device-px band that straddles the
    // geometric boundary (inside AND outside). A quad sized to the exact rect
    // only rasterizes fragments whose center is inside, so the OUTER half of the
    // AA band is never shaded — invisible for axis-aligned edges (outer fringe
    // pixels have dist > edge_width → alpha 0) but visibly truncating for
    // diagonal/rotated edges, where an outside-center pixel can still be ~50%
    // covered. Expanding the quad gives those fringe pixels fragments; the SDF
    // distance is still measured to the TRUE rect (via `out.uv` below), so the
    // shape is unchanged — only the previously-missing outer fringe is added.
    //
    // Margin is applied in LOCAL units = fringe_device / per-axis scale, so the
    // fringe is ~1.5 device-px at any zoom/rotation. Column lengths of the 2×2
    // give the local→device scale per axis.
    let fringe_device = 1.5;
    let col0_len = length(instance.transform.xy); // |x-column| → device x-scale
    let col1_len = length(instance.transform.zw); // |y-column| → device y-scale
    let size_safe = max(instance.bounds.zw, vec2<f32>(1e-6, 1e-6));
    let margin_local = vec2<f32>(
        fringe_device / max(col0_len, 1e-6),
        fringe_device / max(col1_len, 1e-6),
    );
    // Expand the unit quad to [-e, 1+e] where e = margin_local / size.
    let expand = margin_local / size_safe;
    let exp_pos = vertex.position * (1.0 + 2.0 * expand) - expand;
    let local_pos = exp_pos * instance.bounds.zw + instance.bounds.xy;

    // Apply the full 2×3 affine: device = M * local + t.
    //
    // transform = [a, b, c, d] is the 2×2 linear part column-major:
    //   x-column = (a, b), y-column = (c, d).
    // So: M * p = (a*p.x + c*p.y,  b*p.x + d*p.y).
    //
    // For the baked-AABB path: M=identity ([1,0,0,1]), t=[0,0] →
    // device = local (byte-identical output).
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
    // UV carries the EXPANDED normalized coordinate so that the fragment's
    // `(uv - 0.5) * rect_size` recovers the TRUE centered local position even on
    // the expanded fringe (uv ranges slightly outside [0,1] there). This keeps
    // the SDF distance measured to the true rect regardless of quad expansion.
    out.uv = exp_pos;
    // rect_size = local width × height; the SDF evaluates distance in this space.
    // Under any affine, fwidth(dist) correctly measures the screen-space
    // derivative of the LOCAL distance → ~1 device-px AA band.
    out.rect_size = instance.bounds.zw;
    out.corner_radii = instance.corner_radii;
    // world_pos is the device-pixel position, used by the SDF clip test.
    out.world_pos = device_pos;
    out.clip_bounds = instance.clip_bounds;
    out.clip_radii = instance.clip_radii;
    out.clip_kind = instance.clip_kind.x;

    return out;
}

// =============================================================================
// Fragment Shader (SDF-based with SDF clip support)
// =============================================================================

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Convert UV [0,1] to centered coordinates [-size/2, size/2]
    let p = (in.uv - 0.5) * in.rect_size;

    // Compute signed distance to rounded rectangle
    // Negative = inside, Positive = outside, 0 = on edge
    let dist = sdRoundedBox(p, in.rect_size * 0.5, in.corner_radii);

    // Convert distance to alpha with adaptive antialiasing
    // fwidth() automatically adjusts AA based on zoom level and pixel density
    let alpha = sdfToAlpha(dist);

    // --- SDF Clip Test ---
    // Branch on clip_kind: 0 = no clip, 1 = sdRoundedBox, 2 = sdRoundedSuperellipse.
    // The clip_bounds + clip_radii slot is shared between clip kinds; the
    // kind flag selects the SDF function. Per-instance flat interpolation
    // keeps the branch divergence-free within a draw call.
    var clip_alpha = 1.0;
    if (in.clip_kind != 0u && in.clip_bounds.z > 0.0 && in.clip_bounds.w > 0.0) {
        let clip_center = in.clip_bounds.xy + in.clip_bounds.zw * 0.5;
        let clip_p = in.world_pos - clip_center;
        let clip_half = in.clip_bounds.zw * 0.5;

        var clip_dist = 0.0;
        if (in.clip_kind == 2u) {
            clip_dist = sdRoundedSuperellipse(clip_p, clip_half, in.clip_radii);
        } else {
            // clip_kind == 1u (rrect) — also the safe default for any
            // future kind we haven't yet learned about.
            clip_dist = sdRoundedBox(clip_p, clip_half, in.clip_radii);
        }

        clip_alpha = sdfToAlpha(clip_dist);
    }

    // Return color with antialiased alpha, modulated by clip alpha
    return vec4<f32>(in.color.rgb, in.color.a * alpha * clip_alpha);
}

// =============================================================================
// Performance Notes
// =============================================================================
//
// Previous implementation (pixel-space + branching):
// - 4 if/else branches per fragment
// - Fixed-width antialiasing (1 pixel)
// - GPU divergence (different threads take different paths)
//
// Current implementation (SDF-based):
// - 0 branches (fully branchless for non-clipped, 1 branch for clip test)
// - Adaptive antialiasing (perfect at any zoom)
// - Optimal GPU parallelism (all threads execute same code)
//
// SDF clipping:
// - Eliminates stencil buffer requirement for rounded rect clips
// - No tessellation needed (pure math in fragment shader)
// - ~10x faster than tessellation + path clip approach
// - Smooth antialiased clip edges at any resolution
//
// Measured improvement: 30-40% faster on average GPUs
