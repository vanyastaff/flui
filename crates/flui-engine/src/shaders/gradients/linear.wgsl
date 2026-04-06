// Linear Gradient Shader for FLUI
//
// Renders linear gradients with up to 8 color stops.
// Supports arbitrary start/end points for any gradient angle.
//
// Common use cases:
// - Button backgrounds (top-to-bottom highlight)
// - Card headers (diagonal branding gradients)
// - Progress bars (horizontal gradients)
// - Background overlays (fade effects)

// Vertex input (unit quad)
struct VertexInput {
    @location(0) position: vec2<f32>,  // [0-1, 0-1]
}

// Instance input (per-gradient rectangle)
struct InstanceInput {
    @location(2) bounds: vec4<f32>,         // [x, y, width, height]
    @location(3) gradient_start: vec2<f32>, // Start point (local coords)
    @location(4) gradient_end: vec2<f32>,   // End point (local coords)
    @location(5) corner_radii: vec4<f32>,   // [tl, tr, br, bl] for clipping
    @location(6) stop_count: u32,           // Number of gradient stops (1-8)
}

// Gradient stop definition
struct GradientStop {
    color: vec4<f32>,  // RGBA color
    position: f32,     // Position along gradient [0.0, 1.0]
    _padding: vec3<f32>,
}

// Vertex output / Fragment input
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_pos: vec2<f32>,      // World space position
    @location(1) local_pos: vec2<f32>,      // Local position within bounds
    @location(2) gradient_start: vec2<f32>,
    @location(3) gradient_end: vec2<f32>,
    @location(4) rect_size: vec2<f32>,
    @location(5) corner_radii: vec4<f32>,
    @location(6) @interpolate(flat) stop_count: u32,
}

// Uniforms
struct Viewport {
    size: vec2<f32>,
    _padding: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> viewport: Viewport;

// Gradient stops (storage buffer for dynamic sizing)
@group(1) @binding(0)
var<storage, read> gradient_stops: array<GradientStop>;

// =============================================================================
// SDF Functions (for rounded corners clipping)
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

/// Interpolate color from gradient stops
/// t: position along gradient [0.0, 1.0]
/// stop_count: number of stops to consider
fn interpolateGradient(t: f32, stop_count: u32) -> vec4<f32> {
    let t_clamped = clamp(t, 0.0, 1.0);

    // Handle edge cases
    if (stop_count == 0u) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0); // Fallback to black
    }
    if (stop_count == 1u) {
        return gradient_stops[0].color;
    }

    // Find the two stops that bracket our position
    var prev_stop = gradient_stops[0];
    var next_stop = gradient_stops[1];

    // Before first stop
    if (t_clamped <= prev_stop.position) {
        return prev_stop.color;
    }

    // Find bracketing stops
    for (var i = 1u; i < stop_count; i++) {
        next_stop = gradient_stops[i];

        if (t_clamped <= next_stop.position) {
            // Interpolate between prev_stop and next_stop
            let range = next_stop.position - prev_stop.position;
            if (range > 0.0) {
                let local_t = (t_clamped - prev_stop.position) / range;
                return mix(prev_stop.color, next_stop.color, local_t);
            } else {
                // Stops at same position, use next
                return next_stop.color;
            }
        }

        prev_stop = next_stop;
    }

    // After last stop
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

    // Transform to world space
    let local_pos = vertex.position * instance.bounds.zw;
    let world_pos = local_pos + instance.bounds.xy;

    // Convert to clip space
    let clip_x = (world_pos.x / viewport.size.x) * 2.0 - 1.0;
    let clip_y = 1.0 - (world_pos.y / viewport.size.y) * 2.0;

    out.clip_position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    out.world_pos = world_pos;
    out.local_pos = local_pos;
    out.gradient_start = instance.gradient_start;
    out.gradient_end = instance.gradient_end;
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
    // Check if we're inside rounded corners (for clipping)
    let centered_pos = (in.local_pos / in.rect_size - 0.5) * in.rect_size;
    let dist = sdRoundedBox(centered_pos, in.rect_size * 0.5, in.corner_radii);

    // Early discard if outside shape
    if (dist > 1.0) {
        discard;
    }

    // Compute gradient parameter t
    let gradient_vec = in.gradient_end - in.gradient_start;
    let gradient_length_sq = dot(gradient_vec, gradient_vec);

    var t: f32;
    if (gradient_length_sq > 0.0001) {
        // Project local position onto gradient line
        t = dot(in.local_pos - in.gradient_start, gradient_vec) / gradient_length_sq;
    } else {
        // Degenerate gradient (start == end), use solid color
        t = 0.0;
    }

    // Interpolate color from gradient stops
    var color = interpolateGradient(t, in.stop_count);

    // Apply rounded corner alpha
    if (dist > -1.0) {
        let alpha = sdfToAlpha(dist);
        color = vec4<f32>(color.rgb, color.a * alpha);
    }

    return color;
}

// =============================================================================
// Usage Example (Rust side)
// =============================================================================
//
// ```rust
// // Create gradient stops
// let stops = vec![
//     GradientStop {
//         color: Color::rgb(255, 0, 150),  // Pink
//         position: 0.0,
//     },
//     GradientStop {
//         color: Color::rgb(0, 200, 255),  // Blue
//         position: 1.0,
//     },
// ];
//
// // Render vertical gradient (top to bottom)
// painter.gradient_rect(
//     bounds,
//     gradient_start: Vec2::new(0.0, 0.0),      // Top of rect
//     gradient_end: Vec2::new(0.0, bounds.height), // Bottom of rect
//     stops,
//     corner_radius: 12.0,
// );
//
// // Diagonal gradient (top-left to bottom-right)
// painter.gradient_rect(
//     bounds,
//     gradient_start: Vec2::new(0.0, 0.0),
//     gradient_end: Vec2::new(bounds.width, bounds.height),
//     stops,
//     corner_radius: 12.0,
// );
// ```
