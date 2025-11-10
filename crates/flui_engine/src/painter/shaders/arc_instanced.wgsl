// Instanced arc shader for FLUI
//
// Renders multiple arcs (partial circles) in a single draw call using GPU instancing.
// Each instance contains: center, radius, angles (start, sweep), color, and transform.
//
// Performance: 100 arcs = 1 draw call (vs 100 without instancing)

// Vertex input (shared unit quad: [-1,-1] to [1,1])
struct VertexInput {
    @location(0) position: vec2<f32>,  // Quad corner [-1 to 1]
}

// Instance input (per-arc data)
struct InstanceInput {
    @location(2) center_radius: vec4<f32>,  // [x, y, radius, _padding]
    @location(3) angles: vec4<f32>,         // [start_angle, sweep_angle, _padding, _padding]
    @location(4) color: vec4<f32>,          // [r, g, b, a] in 0-1 range
    @location(5) transform: vec4<f32>,      // [scale_x, scale_y, translate_x, translate_y]
}

// Vertex output / Fragment input
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) local_pos: vec2<f32>,      // Position relative to arc center [-1 to 1]
    @location(2) radius: f32,               // Arc radius (for anti-aliasing)
    @location(3) start_angle: f32,          // Start angle in radians
    @location(4) sweep_angle: f32,          // Sweep angle in radians
}

// Viewport uniform (for screen-space to clip-space conversion)
struct Viewport {
    size: vec2<f32>,      // Viewport size in pixels
    _padding: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> viewport: Viewport;

@vertex
fn vs_main(
    vertex: VertexInput,
    instance: InstanceInput,
) -> VertexOutput {
    var out: VertexOutput;

    let center = instance.center_radius.xy;
    let radius = instance.center_radius.z;

    // Transform unit quad [-1,1] to arc bounding box
    // Apply instance transform for elliptical arcs (scale_x, scale_y)
    let local_pos = vertex.position * vec2<f32>(
        radius * instance.transform.x,
        radius * instance.transform.y
    );

    let world_pos = center + local_pos + instance.transform.zw; // Add translation

    // Convert to clip space [-1, 1]
    let clip_x = (world_pos.x / viewport.size.x) * 2.0 - 1.0;
    let clip_y = 1.0 - (world_pos.y / viewport.size.y) * 2.0; // Flip Y for screen coords

    out.position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    out.color = instance.color;
    out.local_pos = vertex.position; // [-1 to 1] range
    out.radius = radius;
    out.start_angle = instance.angles.x;
    out.sweep_angle = instance.angles.y;

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Calculate distance from arc center
    let dist = length(in.local_pos);

    // Arc edge at distance = 1.0 (unit circle in local space)
    // Smooth anti-aliasing using smoothstep
    let edge_softness = 0.02; // 2% of radius for smooth edge
    var alpha = 1.0 - smoothstep(1.0 - edge_softness, 1.0 + edge_softness, dist);

    // Discard pixels outside circle radius
    if (alpha < 0.01) {
        discard;
    }

    // Calculate angle of current pixel
    // atan2(y, x) returns angle from -π to π
    // We need to convert local_pos to angle
    let pixel_angle = atan2(in.local_pos.y, in.local_pos.x);

    // Normalize angles to [0, 2π] range
    let start = in.start_angle;
    let sweep = in.sweep_angle;

    // Handle angle wrapping
    // Convert pixel angle to [0, 2π] range
    var normalized_pixel = pixel_angle;
    if (normalized_pixel < 0.0) {
        normalized_pixel = normalized_pixel + 6.28318530718; // + 2π
    }

    // Convert start angle to [0, 2π] range
    var normalized_start = start;
    if (normalized_start < 0.0) {
        normalized_start = normalized_start + 6.28318530718;
    }

    // Calculate end angle
    var end_angle = normalized_start + sweep;

    // Check if pixel is within arc sweep
    var in_arc = false;

    if (sweep > 0.0) {
        // Clockwise sweep
        if (end_angle > 6.28318530718) {
            // Arc wraps around 2π
            in_arc = (normalized_pixel >= normalized_start) || (normalized_pixel <= (end_angle - 6.28318530718));
        } else {
            in_arc = (normalized_pixel >= normalized_start) && (normalized_pixel <= end_angle);
        }
    } else {
        // Counter-clockwise sweep
        end_angle = normalized_start + sweep;
        if (end_angle < 0.0) {
            // Arc wraps around 0
            in_arc = (normalized_pixel <= normalized_start) || (normalized_pixel >= (end_angle + 6.28318530718));
        } else {
            in_arc = (normalized_pixel <= normalized_start) && (normalized_pixel >= end_angle);
        }
    }

    // Discard pixels outside arc sweep with smooth edges
    if (!in_arc) {
        discard;
    }

    // Apply smooth edge anti-aliasing to arc boundaries
    let angle_softness = 0.05; // ~3 degrees
    var edge_alpha = 1.0;

    // Distance to start edge
    var dist_to_start = abs(normalized_pixel - normalized_start);
    if (dist_to_start > 3.14159265359) {
        dist_to_start = 6.28318530718 - dist_to_start; // Handle wrap-around
    }

    // Distance to end edge
    var dist_to_end = abs(normalized_pixel - end_angle);
    if (dist_to_end > 3.14159265359) {
        dist_to_end = 6.28318530718 - dist_to_end; // Handle wrap-around
    }

    // Smooth edges at arc start and end
    let min_edge_dist = min(dist_to_start, dist_to_end);
    if (min_edge_dist < angle_softness) {
        edge_alpha = min_edge_dist / angle_softness;
    }

    // Multiply alpha with color alpha and edge alpha
    return vec4<f32>(in.color.rgb, in.color.a * alpha * edge_alpha);
}
