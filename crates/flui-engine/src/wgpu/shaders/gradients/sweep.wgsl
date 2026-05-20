// Sweep (Conic/Angular) Gradient Shader for FLUI
//
// Renders angular gradients around a center point with up to 8 color stops.
// Uses atan2 to compute the angle, then maps to color stops.
//
// Common use cases:
// - Color wheel / hue picker
// - Pie chart backgrounds
// - Circular progress indicators
// - Conic gradient decorations

// Vertex input (unit quad)
struct VertexInput {
    @location(0) position: vec2<f32>,
}

// Instance input
struct InstanceInput {
    @location(2) bounds: vec4<f32>,         // [x, y, width, height]
    @location(3) center: vec2<f32>,         // Center point (local coords)
    @location(4) angles: vec2<f32>,         // [start_angle, end_angle] in radians
    @location(5) corner_radii: vec4<f32>,   // [tl, tr, br, bl]
    @location(6) stop_count: u32,
    @location(7) stop_offset: u32,          // Offset into gradient stops buffer
}

// Gradient stop (same layout as linear/radial)
struct GradientStop {
    color: vec4<f32>,
    position: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
}

// Vertex output
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) local_pos: vec2<f32>,
    @location(1) center: vec2<f32>,
    @location(2) angles: vec2<f32>,
    @location(3) rect_size: vec2<f32>,
    @location(4) corner_radii: vec4<f32>,
    @location(5) @interpolate(flat) stop_count: u32,
    @location(6) @interpolate(flat) stop_offset: u32,
}

// Uniforms
struct Viewport {
    size: vec2<f32>,
    _padding: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> viewport: Viewport;

@group(1) @binding(0)
var<storage, read> gradient_stops: array<GradientStop>;

// =============================================================================
// SDF Functions
// =============================================================================

fn sdRoundedBox(p: vec2<f32>, b: vec2<f32>, r: vec4<f32>) -> f32 {
    let r2 = select(r.zw, r.xy, p.x > 0.0);
    let r3 = select(r2.y, r2.x, p.y > 0.0);
    let q = abs(p) - b + vec2<f32>(r3);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0))) - r3;
}

fn sdfToAlpha(dist: f32) -> f32 {
    let edge_width = fwidth(dist) * 0.5;
    return 1.0 - smoothstep(-edge_width, edge_width, dist);
}

// =============================================================================
// Gradient Interpolation (same as linear/radial)
// =============================================================================

fn interpolateGradient(t: f32, stop_count: u32, stop_offset: u32) -> vec4<f32> {
    let t_clamped = clamp(t, 0.0, 1.0);

    if (stop_count == 0u) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    if (stop_count == 1u) {
        return gradient_stops[stop_offset].color;
    }

    var prev_stop = gradient_stops[stop_offset];
    var next_stop = gradient_stops[stop_offset + 1u];

    if (t_clamped <= prev_stop.position) {
        return prev_stop.color;
    }

    for (var i = 1u; i < stop_count; i++) {
        next_stop = gradient_stops[stop_offset + i];

        if (t_clamped <= next_stop.position) {
            let range = next_stop.position - prev_stop.position;
            if (range > 0.0) {
                let local_t = (t_clamped - prev_stop.position) / range;
                return mix(prev_stop.color, next_stop.color, local_t);
            } else {
                return next_stop.color;
            }
        }

        prev_stop = next_stop;
    }

    return next_stop.color;
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

    let local_pos = vertex.position * instance.bounds.zw;
    let world_pos = local_pos + instance.bounds.xy;

    let clip_x = (world_pos.x / viewport.size.x) * 2.0 - 1.0;
    let clip_y = 1.0 - (world_pos.y / viewport.size.y) * 2.0;

    out.clip_position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    out.local_pos = local_pos;
    out.center = instance.center;
    out.angles = instance.angles;
    out.rect_size = instance.bounds.zw;
    out.corner_radii = instance.corner_radii;
    out.stop_count = instance.stop_count;
    out.stop_offset = instance.stop_offset;

    return out;
}

// =============================================================================
// Fragment Shader
// =============================================================================

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Check if inside rounded corners
    let centered_pos = (in.local_pos / in.rect_size - 0.5) * in.rect_size;
    let dist = sdRoundedBox(centered_pos, in.rect_size * 0.5, in.corner_radii);

    if (dist > 1.0) {
        discard;
    }

    // Compute angle from center using atan2
    let offset = in.local_pos - in.center;
    let angle = atan2(offset.y, offset.x);

    // Map angle to [0, 1] based on start/end angle range
    let start_angle = in.angles.x;
    let end_angle = in.angles.y;
    let angle_range = end_angle - start_angle;

    var t: f32;
    if (abs(angle_range) > 0.0001) {
        // Normalize the angle relative to the sweep range
        // Handle wrapping: shift angle so start_angle maps to 0
        var shifted = angle - start_angle;
        // Wrap into [0, 2*PI) range
        let two_pi = 6.28318530718;
        shifted = shifted - floor(shifted / two_pi) * two_pi;
        let norm_range = angle_range - floor(angle_range / two_pi) * two_pi;
        if (norm_range > 0.0001) {
            t = shifted / norm_range;
        } else {
            t = 0.0;
        }
    } else {
        t = 0.0;
    }

    t = clamp(t, 0.0, 1.0);

    // Interpolate color from gradient stops
    var color = interpolateGradient(t, in.stop_count, in.stop_offset);

    // Apply corner clipping (fwidth must be called from uniform control flow)
    let alpha = sdfToAlpha(dist);
    color = vec4<f32>(color.rgb, color.a * alpha);

    return color;
}
