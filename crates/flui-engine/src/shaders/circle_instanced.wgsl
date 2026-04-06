// Instanced circle shader for FLUI
//
// Renders multiple circles/ellipses in a single draw call using GPU instancing.
// Each instance contains: center, radius, color, and transform.
//
// Instance layout matches CircleInstance in vertex.rs:
//   @location(2) center:    vec2<f32>  (center x, y)
//   @location(3) radius:    vec2<f32>  (rx, ry)
//   @location(4) color:     vec4<f32>  (RGBA)
//   @location(5) transform: vec4<f32>  (scale_x, scale_y, translate_x, translate_y)

// Vertex input (shared unit quad: [0,0] to [1,1])
struct VertexInput {
    @location(0) position: vec2<f32>,
}

// Instance input (per-circle data) — matches CircleInstance::desc()
struct InstanceInput {
    @location(2) center: vec2<f32>,
    @location(3) radius: vec2<f32>,
    @location(4) color: vec4<f32>,
    @location(5) transform: vec4<f32>,
}

// Vertex output / Fragment input
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) local_pos: vec2<f32>,
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

    // Convert unit quad [0,1] to [-1,1] range for local position
    let normalized_pos = vertex.position * 2.0 - 1.0;

    // Transform to ellipse bounding box in world space
    let scaled_pos = normalized_pos * instance.radius * instance.transform.xy;
    let world_pos = instance.center + scaled_pos + instance.transform.zw;

    // Convert to clip space [-1, 1]
    let clip_x = (world_pos.x / viewport.size.x) * 2.0 - 1.0;
    let clip_y = 1.0 - (world_pos.y / viewport.size.y) * 2.0;

    out.position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    out.color = instance.color;
    out.local_pos = normalized_pos;

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // SDF for unit circle: distance from origin
    let dist = length(in.local_pos);

    // Adaptive antialiasing via fwidth
    let edge_width = fwidth(dist) * 0.5;
    let alpha = 1.0 - smoothstep(1.0 - edge_width, 1.0 + edge_width, dist);

    if (alpha < 0.01) {
        discard;
    }

    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}
