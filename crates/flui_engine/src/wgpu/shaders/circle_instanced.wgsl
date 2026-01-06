// Instanced circle shader for FLUI
//
// Renders multiple circles in a single draw call using GPU instancing.
// Each instance contains: center, radius, color, and transform (for ellipses).
//
// Performance: 100 circles = 1 draw call (vs 100 without instancing)

// Vertex input (shared unit quad: [0,0] to [1,1])
struct VertexInput {
    @location(0) position: vec2<f32>,  // Quad corner [0 to 1]
}

// Instance input (per-circle data)
struct InstanceInput {
    @location(2) center_radius: vec4<f32>,  // [x, y, radius, _padding]
    @location(3) color: vec4<f32>,          // [r, g, b, a] in 0-1 range
    @location(4) transform: vec4<f32>,      // [scale_x, scale_y, translate_x, translate_y]
}

// Vertex output / Fragment input
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) local_pos: vec2<f32>,      // Position relative to circle center [-1 to 1]
    @location(2) radius: f32,               // Circle radius (for anti-aliasing)
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

    // Convert unit quad [0,1] to [-1,1] range for local position
    let normalized_pos = vertex.position * 2.0 - 1.0;

    // Transform to circle bounding box in world space
    // Apply instance transform for ellipses (scale_x, scale_y)
    let scaled_pos = normalized_pos * vec2<f32>(
        radius * instance.transform.x,
        radius * instance.transform.y
    );

    let world_pos = center + scaled_pos + instance.transform.zw; // Add translation

    // Convert to clip space [-1, 1]
    let clip_x = (world_pos.x / viewport.size.x) * 2.0 - 1.0;
    let clip_y = 1.0 - (world_pos.y / viewport.size.y) * 2.0; // Flip Y for screen coords

    out.position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    out.color = instance.color;
    out.local_pos = normalized_pos; // [-1 to 1] range for fragment shader
    out.radius = radius;

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Calculate distance from circle center
    let dist = length(in.local_pos);

    // Circle edge at distance = 1.0 (unit circle in local space)
    // Smooth anti-aliasing using smoothstep
    let edge_softness = 0.02; // 2% of radius for smooth edge
    let alpha = 1.0 - smoothstep(1.0 - edge_softness, 1.0 + edge_softness, dist);

    // Discard pixels outside circle (optional, but improves overdraw)
    if (alpha < 0.01) {
        discard;
    }

    // Multiply alpha with color alpha
    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}
