// Instanced texture shader for FLUI
//
// Renders multiple textured quads in a single draw call using GPU instancing.
// Each instance contains: destination rect, source UV, tint color, and transform.
//
// Performance: 100 images = 1 draw call (vs 100 without instancing)
// Supports: texture atlases, rotation, color tinting

// Vertex input (shared unit quad: [0,0] to [1,1])
struct VertexInput {
    @location(0) position: vec2<f32>,  // Quad corner [0 to 1]
}

// Instance input (per-image data)
struct InstanceInput {
    @location(2) dst_rect: vec4<f32>,      // [x, y, width, height] in screen space
    @location(3) src_uv: vec4<f32>,        // [u_min, v_min, u_max, v_max] in 0-1 range
    @location(4) tint: vec4<f32>,          // [r, g, b, a] in 0-1 range
    @location(5) transform: vec4<f32>,     // [cos(angle), sin(angle), tx, ty]
}

// Vertex output / Fragment input
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,            // Texture coordinates
    @location(1) tint: vec4<f32>,          // Tint color
}

// Viewport uniform (for screen-space to clip-space conversion)
struct Viewport {
    size: vec2<f32>,      // Viewport size in pixels
    _padding: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> viewport: Viewport;

@group(1) @binding(0)
var texture_sampler: sampler;

@group(1) @binding(1)
var texture_view: texture_2d<f32>;

@vertex
fn vs_main(
    vertex: VertexInput,
    instance: InstanceInput,
) -> VertexOutput {
    var out: VertexOutput;

    // Extract destination rectangle components
    let dst_x = instance.dst_rect.x;
    let dst_y = instance.dst_rect.y;
    let dst_width = instance.dst_rect.z;
    let dst_height = instance.dst_rect.w;

    // Transform unit quad [0,1] to destination rectangle
    var local_pos = vertex.position * vec2<f32>(dst_width, dst_height);

    // Apply rotation if present
    let cos_angle = instance.transform.x;
    let sin_angle = instance.transform.y;

    // Rotate around center of destination rect
    if (abs(cos_angle - 1.0) > 0.001 || abs(sin_angle) > 0.001) {
        // Translate to origin (center of rect)
        let center = vec2<f32>(dst_width * 0.5, dst_height * 0.5);
        var centered = local_pos - center;

        // Apply rotation matrix
        let rotated = vec2<f32>(
            centered.x * cos_angle - centered.y * sin_angle,
            centered.x * sin_angle + centered.y * cos_angle
        );

        // Translate back
        local_pos = rotated + center;
    }

    // Apply position and additional translation
    let world_pos = vec2<f32>(dst_x, dst_y) + local_pos + instance.transform.zw;

    // Convert to clip space [-1, 1]
    let clip_x = (world_pos.x / viewport.size.x) * 2.0 - 1.0;
    let clip_y = 1.0 - (world_pos.y / viewport.size.y) * 2.0; // Flip Y for screen coords

    out.position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);

    // Calculate UV coordinates from source UV and vertex position
    let u_min = instance.src_uv.x;
    let v_min = instance.src_uv.y;
    let u_max = instance.src_uv.z;
    let v_max = instance.src_uv.w;

    out.uv = vec2<f32>(
        mix(u_min, u_max, vertex.position.x),
        mix(v_min, v_max, vertex.position.y)
    );

    out.tint = instance.tint;

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Sample texture
    var tex_color = textureSample(texture_view, texture_sampler, in.uv);

    // Apply tint (multiply)
    tex_color = tex_color * in.tint;

    // Alpha test (discard fully transparent pixels for better performance)
    if (tex_color.a < 0.01) {
        discard;
    }

    return tex_color;
}
