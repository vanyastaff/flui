// Instanced rectangle shader for FLUI
//
// Renders multiple rectangles in a single draw call using GPU instancing.
// Each instance contains: bounds, color, corner radii, and transform.
//
// Performance: 100 rectangles = 1 draw call (vs 100 without instancing)

// Vertex input (shared unit quad: [0,0] to [1,1])
struct VertexInput {
    @location(0) position: vec2<f32>,  // Quad corner [0-1, 0-1]
}

// Instance input (per-rectangle data)
struct InstanceInput {
    @location(2) bounds: vec4<f32>,         // [x, y, width, height]
    @location(3) color: vec4<f32>,          // [r, g, b, a] in 0-1 range
    @location(4) corner_radii: vec4<f32>,   // [tl, tr, br, bl]
    @location(5) transform: vec4<f32>,      // [scale_x, scale_y, translate_x, translate_y]
}

// Vertex output / Fragment input
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,              // Local UV coordinates [0-1]
    @location(2) rect_size: vec2<f32>,       // Rectangle size for radius calculation
    @location(3) corner_radii: vec4<f32>,    // Corner radii
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

    // Transform unit quad [0-1] to rectangle bounds
    let local_pos = vertex.position * instance.bounds.zw; // Scale by width/height
    let world_pos = local_pos + instance.bounds.xy;        // Translate to position

    // Apply instance transform (for rotations, scaling, etc.)
    let transformed_x = world_pos.x * instance.transform.x + instance.transform.z;
    let transformed_y = world_pos.y * instance.transform.y + instance.transform.w;

    // Convert to clip space [-1, 1]
    let clip_x = (transformed_x / viewport.size.x) * 2.0 - 1.0;
    let clip_y = 1.0 - (transformed_y / viewport.size.y) * 2.0; // Flip Y for screen coords

    out.position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    out.color = instance.color;
    out.uv = vertex.position;  // UV coordinates [0-1]
    out.rect_size = instance.bounds.zw;
    out.corner_radii = instance.corner_radii;

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Calculate distance from edges for rounded corners
    let size = in.rect_size;
    let uv = in.uv;
    let radii = in.corner_radii;

    // Determine which corner we're in and get its radius
    var radius: f32;
    var local_uv: vec2<f32>;

    if (uv.x < 0.5 && uv.y < 0.5) {
        // Top-left corner
        radius = radii.x;
        local_uv = uv * size;
    } else if (uv.x >= 0.5 && uv.y < 0.5) {
        // Top-right corner
        radius = radii.y;
        local_uv = vec2<f32>((1.0 - uv.x) * size.x, uv.y * size.y);
    } else if (uv.x >= 0.5 && uv.y >= 0.5) {
        // Bottom-right corner
        radius = radii.z;
        local_uv = (vec2<f32>(1.0) - uv) * size;
    } else {
        // Bottom-left corner
        radius = radii.w;
        local_uv = vec2<f32>(uv.x * size.x, (1.0 - uv.y) * size.y);
    }

    // If we have a radius, calculate distance from corner circle
    if (radius > 0.0) {
        let corner_center = vec2<f32>(radius, radius);
        let dist = length(local_uv - corner_center);

        // Smooth anti-aliasing at corner edge
        let edge_dist = radius - dist;
        let alpha = smoothstep(0.0, 1.0, edge_dist);

        // Multiply alpha with color alpha
        return vec4<f32>(in.color.rgb, in.color.a * alpha);
    }

    // No rounding, return color as-is
    return in.color;
}
