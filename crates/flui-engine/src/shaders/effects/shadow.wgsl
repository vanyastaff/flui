// Shadow shader for FLUI
//
// Renders box shadows using analytical Gaussian approximation.
// Single-pass, no blur required.
//
// Instance layout matches ShadowInstance in batchers/effects.rs:
//   @location(2) bounds:      vec4<f32>  (x, y, w, h)
//   @location(3) color:       vec4<f32>  (RGBA)
//   @location(4) offset_data: vec4<f32>  (offset_x, offset_y, blur_radius, spread)
//
// Note: offset (vec2), blur_radius (f32), and spread (f32) are packed
// into a single vec4 at location 4 for alignment.

const PI: f32 = 3.14159265359;
const SQRT_2: f32 = 1.41421356237;

// Vertex input (shared unit quad)
struct VertexInput {
    @location(0) position: vec2<f32>,
}

// Instance input — matches ShadowInstance layout
struct InstanceInput {
    @location(2) bounds: vec4<f32>,
    @location(3) color: vec4<f32>,
    @location(4) offset_data: vec4<f32>,
}

// Vertex output
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) local_pos: vec2<f32>,
    @location(1) bounds: vec4<f32>,
    @location(2) color: vec4<f32>,
    @location(3) offset_data: vec4<f32>,
}

// Viewport uniform
struct Viewport {
    size: vec2<f32>,
    _padding: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> viewport: Viewport;

// =============================================================================
// Error Function (erf) Approximation
// =============================================================================

fn erf(x: f32) -> f32 {
    let s = sign(x);
    let a = abs(x);
    let t = 1.0 / (1.0 + 0.3275911 * a);
    let poly = t * (0.254829592 +
                    t * (-0.284496736 +
                    t * (1.421413741 +
                    t * (-1.453152027 +
                    t * 1.061405429))));
    return s * (1.0 - poly * exp(-a * a));
}

// =============================================================================
// Shadow Functions
// =============================================================================

fn roundedRectShadow(p: vec2<f32>, rect_half: vec2<f32>, sigma: f32) -> f32 {
    let dist_x = abs(p.x) - rect_half.x;
    let dist_y = abs(p.y) - rect_half.y;
    let dist = max(dist_x, dist_y);

    if (dist > 3.0 * sigma) {
        return 0.0;
    }
    if (dist < -3.0 * sigma) {
        return 1.0;
    }

    return 0.5 - 0.5 * erf(dist / (sigma * SQRT_2));
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

    let blur_radius = instance.offset_data.z;
    let spread = instance.offset_data.w;
    let offset = instance.offset_data.xy;

    // Expand bounds by blur + spread to cover shadow extent
    let expand = blur_radius * 3.0 + spread;
    let expanded_bounds = vec4<f32>(
        instance.bounds.x - expand + offset.x,
        instance.bounds.y - expand + offset.y,
        instance.bounds.z + expand * 2.0,
        instance.bounds.w + expand * 2.0,
    );

    // Transform unit quad to expanded shadow bounds
    let local_pos = vertex.position * expanded_bounds.zw;
    let world_pos = local_pos + expanded_bounds.xy;

    // Convert to clip space
    let clip_x = (world_pos.x / viewport.size.x) * 2.0 - 1.0;
    let clip_y = 1.0 - (world_pos.y / viewport.size.y) * 2.0;

    out.clip_position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    out.local_pos = world_pos;
    out.bounds = instance.bounds;
    out.color = instance.color;
    out.offset_data = instance.offset_data;

    return out;
}

// =============================================================================
// Fragment Shader
// =============================================================================

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let offset = in.offset_data.xy;
    let blur_radius = in.offset_data.z;
    let spread = in.offset_data.w;

    // Compute sigma from blur radius
    let sigma = max(blur_radius * 0.5, 0.001);

    // Center of the shadow rectangle (with offset applied)
    let rect_center = vec2<f32>(
        in.bounds.x + in.bounds.z * 0.5 + offset.x,
        in.bounds.y + in.bounds.w * 0.5 + offset.y,
    );

    // Half-extents of the shadow rectangle (with spread)
    let rect_half = vec2<f32>(
        in.bounds.z * 0.5 + spread,
        in.bounds.w * 0.5 + spread,
    );

    // Position relative to shadow center
    let p = in.local_pos - rect_center;

    // Compute shadow intensity
    let shadow_alpha = roundedRectShadow(p, rect_half, sigma);

    return vec4<f32>(in.color.rgb, in.color.a * shadow_alpha);
}
