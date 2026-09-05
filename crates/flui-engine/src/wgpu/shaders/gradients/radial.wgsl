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
    @location(4) radius_pad: vec2<f32>,     // [radius, padding]
    @location(5) corner_radii: vec4<f32>,   // [tl, tr, br, bl]
    @location(6) stop_count: u32,
    @location(7) stop_offset: u32,          // Offset into gradient stops buffer
    @location(8) clip_bounds: vec4<f32>,    // Device-space [x, y, w, h]
    @location(9) clip_radii: vec4<f32>,     // [tl, tr, br, bl]
    @location(10) clip_kind: vec4<u32>,     // [kind, _, _, _]
    @location(11) clip_device_to_local: vec4<f32>, // [a, b, c, d], columns first
    @location(12) clip_local_origin: vec4<f32>,    // [tx, ty, 0, 0]
}

// Gradient stop (same as linear)
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
    @location(2) radius: f32,
    @location(3) rect_size: vec2<f32>,
    @location(4) corner_radii: vec4<f32>,
    @location(5) @interpolate(flat) stop_count: u32,
    @location(6) @interpolate(flat) stop_offset: u32,
    // Device-space position, carried only for the clip SDF. Linear
    // already had one at location 0; these two did not.
    @location(7) world_pos: vec2<f32>,
    @location(8) clip_bounds: vec4<f32>,
    @location(9) clip_radii: vec4<f32>,
    @location(10) @interpolate(flat) clip_kind: u32,
    @location(11) clip_device_to_local: vec4<f32>,
    @location(12) clip_local_origin: vec4<f32>,
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

// =============================================================================
// Gradient Interpolation (same as linear)
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
    out.radius = instance.radius_pad.x;
    out.rect_size = instance.bounds.zw;
    out.corner_radii = instance.corner_radii;
    out.stop_count = instance.stop_count;
    out.stop_offset = instance.stop_offset;
    out.world_pos = world_pos;
    out.clip_bounds = instance.clip_bounds;
    out.clip_radii = instance.clip_radii;
    // Bit 2 carries the clip layer's Clip mode; `clipAlpha` unpacks it.
    out.clip_kind = instance.clip_kind.x | (instance.clip_kind.z << 2u);
    out.clip_device_to_local = instance.clip_device_to_local;
    out.clip_local_origin = instance.clip_local_origin;

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

    // Interpolate color from storage buffer
    var color = interpolateGradient(t, in.stop_count, in.stop_offset);

    // Apply corner clipping (derivatives dpdx/dpdy must be called from uniform control flow)
    let alpha = sdfToAlpha(dist);
    // Clip coverage — see `clipAlpha` in `common/clip.wgsl`.
    let clip_alpha = clipAlpha(
        in.world_pos,
        in.clip_bounds,
        in.clip_radii,
        in.clip_kind,
        in.clip_device_to_local,
        in.clip_local_origin,
    );
    color = vec4<f32>(color.rgb, color.a * alpha * clip_alpha);

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
