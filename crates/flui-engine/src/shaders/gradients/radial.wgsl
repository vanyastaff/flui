// Radial Gradient Shader for FLUI
//
// Renders radial (circular) gradients with dynamic color stops via storage buffer.
// Supports custom center point and radius for spotlight effects.
// Uses uniform buffer for per-gradient params (one draw call per gradient).

// Vertex input (unit quad)
struct VertexInput {
    @location(0) position: vec2<f32>,
}

// Vertex output
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) local_pos: vec2<f32>,
}

// Viewport uniform (shared, group 0)
struct Viewport {
    size: vec2<f32>,
    _padding: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> viewport: Viewport;

// Gradient uniform (per-gradient, group 1)
struct GradientUniforms {
    bounds: vec4<f32>,          // x, y, w, h
    center_radius: vec4<f32>,   // center.x, center.y, radius, 0.0
    corner_radii: vec4<f32>,    // tl, tr, br, bl
    stop_count: u32,
    _padding: vec3<u32>,
}

@group(1) @binding(0)
var<uniform> gradient: GradientUniforms;

// Gradient stop (storage buffer for dynamic sizing)
struct GradientStop {
    color: vec4<f32>,
    position: f32,
    _padding: vec3<f32>,
}

@group(1) @binding(1)
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
// Gradient Interpolation
// =============================================================================

fn interpolateGradient(t: f32, stop_count: u32) -> vec4<f32> {
    let t_clamped = clamp(t, 0.0, 1.0);

    if (stop_count == 0u) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    if (stop_count == 1u) {
        return gradient_stops[0].color;
    }

    if (t_clamped <= gradient_stops[0].position) {
        return gradient_stops[0].color;
    }

    var prev_stop = gradient_stops[0];
    for (var i = 1u; i < stop_count; i++) {
        let next_stop = gradient_stops[i];

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

    return gradient_stops[stop_count - 1u].color;
}

// =============================================================================
// Vertex Shader
// =============================================================================

@vertex
fn vs_main(vertex: VertexInput) -> VertexOutput {
    var out: VertexOutput;

    let local_pos = vertex.position * gradient.bounds.zw;
    let world_pos = local_pos + gradient.bounds.xy;

    let clip_x = (world_pos.x / viewport.size.x) * 2.0 - 1.0;
    let clip_y = 1.0 - (world_pos.y / viewport.size.y) * 2.0;

    out.clip_position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    out.local_pos = local_pos;

    return out;
}

// =============================================================================
// Fragment Shader
// =============================================================================

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let rect_size = gradient.bounds.zw;

    // Check if inside rounded corners
    let centered_pos = (in.local_pos / rect_size - 0.5) * rect_size;
    let dist = sdRoundedBox(centered_pos, rect_size * 0.5, gradient.corner_radii);

    if (dist > 1.0) {
        discard;
    }

    // Compute radial distance from center
    let center = gradient.center_radius.xy;
    let radius = gradient.center_radius.z;
    let radial_dist = length(in.local_pos - center);

    // Normalize to [0, 1] based on radius
    var t: f32;
    if (radius > 0.0001) {
        t = radial_dist / radius;
    } else {
        t = 0.0;
    }

    // Interpolate color
    var color = interpolateGradient(t, gradient.stop_count);

    // Apply corner clipping
    if (dist > -1.0) {
        let alpha = sdfToAlpha(dist);
        color = vec4<f32>(color.rgb, color.a * alpha);
    }

    return color;
}
