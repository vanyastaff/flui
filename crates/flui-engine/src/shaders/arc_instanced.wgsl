// Instanced arc shader for FLUI
//
// Renders multiple arcs (partial circles) in a single draw call using GPU instancing.
//
// Instance layout matches ArcInstance in vertex.rs:
//   @location(2) center:      vec2<f32>  (center x, y)
//   @location(3) radius:      f32
//   @location(4) start_angle: f32
//   @location(5) sweep_angle: f32
//   @location(6) color:       vec4<f32>  (RGBA)
//   (padding not exposed to shader)

const TWO_PI: f32 = 6.28318530718;

// Vertex input (shared unit quad: [0,0] to [1,1])
struct VertexInput {
    @location(0) position: vec2<f32>,
}

// Instance input — matches ArcInstance::desc()
struct InstanceInput {
    @location(2) center: vec2<f32>,
    @location(3) radius: f32,
    @location(4) start_angle: f32,
    @location(5) sweep_angle: f32,
    @location(6) color: vec4<f32>,
}

// Vertex output / Fragment input
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) local_pos: vec2<f32>,
    @location(2) start_angle: f32,
    @location(3) sweep_angle: f32,
}

// Viewport uniform
struct Viewport {
    size: vec2<f32>,
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

    // Convert unit quad [0,1] to [-1,1] range
    let normalized_pos = vertex.position * 2.0 - 1.0;

    // Scale by radius to create bounding box around arc
    let world_pos = instance.center + normalized_pos * instance.radius;

    // Convert to clip space [-1, 1]
    let clip_x = (world_pos.x / viewport.size.x) * 2.0 - 1.0;
    let clip_y = 1.0 - (world_pos.y / viewport.size.y) * 2.0;

    out.position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    out.color = instance.color;
    out.local_pos = normalized_pos;
    out.start_angle = instance.start_angle;
    out.sweep_angle = instance.sweep_angle;

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Distance from center in local space (unit circle)
    let dist = length(in.local_pos);

    // Circle edge SDF with adaptive AA
    let edge_width = fwidth(dist) * 0.5;
    var alpha = 1.0 - smoothstep(1.0 - edge_width, 1.0 + edge_width, dist);

    if (alpha < 0.01) {
        discard;
    }

    // Calculate angle of current pixel
    let pixel_angle = atan2(in.local_pos.y, in.local_pos.x);

    // Normalize pixel angle to [0, 2pi)
    var norm_pixel = pixel_angle;
    if (norm_pixel < 0.0) {
        norm_pixel = norm_pixel + TWO_PI;
    }

    // Normalize start angle to [0, 2pi)
    var norm_start = in.start_angle;
    if (norm_start < 0.0) {
        norm_start = norm_start + TWO_PI;
    }

    let end_angle = norm_start + in.sweep_angle;

    // Check if pixel is within arc sweep
    var in_arc = false;

    if (in.sweep_angle > 0.0) {
        if (end_angle > TWO_PI) {
            in_arc = (norm_pixel >= norm_start) || (norm_pixel <= (end_angle - TWO_PI));
        } else {
            in_arc = (norm_pixel >= norm_start) && (norm_pixel <= end_angle);
        }
    } else {
        let neg_end = norm_start + in.sweep_angle;
        if (neg_end < 0.0) {
            in_arc = (norm_pixel <= norm_start) || (norm_pixel >= (neg_end + TWO_PI));
        } else {
            in_arc = (norm_pixel <= norm_start) && (norm_pixel >= neg_end);
        }
    }

    if (!in_arc) {
        discard;
    }

    // Smooth edges at arc boundaries
    let angle_softness = 0.05;
    var dist_to_start = abs(norm_pixel - norm_start);
    if (dist_to_start > 3.14159265359) {
        dist_to_start = TWO_PI - dist_to_start;
    }
    var dist_to_end = abs(norm_pixel - end_angle);
    if (dist_to_end > 3.14159265359) {
        dist_to_end = TWO_PI - dist_to_end;
    }
    let min_edge_dist = min(dist_to_start, dist_to_end);
    var edge_alpha = 1.0;
    if (min_edge_dist < angle_softness) {
        edge_alpha = min_edge_dist / angle_softness;
    }

    return vec4<f32>(in.color.rgb, in.color.a * alpha * edge_alpha);
}
