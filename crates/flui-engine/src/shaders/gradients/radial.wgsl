// Radial Gradient Shader for FLUI
//
// Renders radial (circular) gradients with up to 8 color stops.
// Supports custom center point and radius for spotlight effects.
//
// Common use cases:
// - Avatar backgrounds (circular fade)
// - Button hover effects (radial highlight from center)
// - Spotlight/vignette effects
// - Loading spinners with gradients

// Vertex input (unit quad)
struct VertexInput {
    @location(0) position: vec2<f32>,
}

// Instance input
struct InstanceInput {
    @location(2) bounds: vec4<f32>,         // [x, y, width, height]
    @location(3) center: vec2<f32>,         // Center point (local coords)
    @location(4) radius: f32,               // Gradient radius
    @location(5) corner_radii: vec4<f32>,   // [tl, tr, br, bl]
    @location(6) stop_count: u32,
}

// Gradient stop (same as linear)
struct GradientStop {
    color: vec4<f32>,
    position: f32,
    _padding: vec3<f32>,
}

// Vertex output
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) local_pos: vec2<f32>,
    @location(1) center: vec2<f32>,
    @location(2) radius: f32,
    @location(3) rect_size: vec2<f32>,
    @location(4) corner_radii: vec4<f32>,
    @location(5) @interpolate(flat) stop_count: u32,
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
// Gradient Interpolation (same as linear)
// =============================================================================

fn interpolateGradient(t: f32, stop_count: u32) -> vec4<f32> {
    let t_clamped = clamp(t, 0.0, 1.0);

    if (stop_count == 0u) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    if (stop_count == 1u) {
        return gradient_stops[0].color;
    }

    var prev_stop = gradient_stops[0];
    var next_stop = gradient_stops[1];

    if (t_clamped <= prev_stop.position) {
        return prev_stop.color;
    }

    for (var i = 1u; i < stop_count; i++) {
        next_stop = gradient_stops[i];

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
    out.radius = instance.radius;
    out.rect_size = instance.bounds.zw;
    out.corner_radii = instance.corner_radii;
    out.stop_count = instance.stop_count;

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

    // Compute radial distance from center
    let radial_dist = length(in.local_pos - in.center);

    // Normalize to [0, 1] based on radius
    var t: f32;
    if (in.radius > 0.0001) {
        t = radial_dist / in.radius;
    } else {
        t = 0.0;
    }

    // Interpolate color
    var color = interpolateGradient(t, in.stop_count);

    // Apply corner clipping
    if (dist > -1.0) {
        let alpha = sdfToAlpha(dist);
        color = vec4<f32>(color.rgb, color.a * alpha);
    }

    return color;
}

// =============================================================================
// Usage Example
// =============================================================================
//
// ```rust
// // Spotlight effect from center
// let stops = vec![
//     GradientStop { color: Color::WHITE, position: 0.0 },
//     GradientStop { color: Color::TRANSPARENT, position: 1.0 },
// ];
//
// painter.radial_gradient_rect(
//     bounds,
//     center: bounds.center(),
//     radius: bounds.width * 0.5,
//     stops,
// );
//
// // Offset spotlight (hover effect)
// painter.radial_gradient_rect(
//     bounds,
//     center: mouse_pos,  // Follow cursor
//     radius: 100.0,
//     stops: vec![
//         GradientStop { color: Color::rgba(255, 255, 255, 0.3), position: 0.0 },
//         GradientStop { color: Color::TRANSPARENT, position: 1.0 },
//     ],
// );
// ```
