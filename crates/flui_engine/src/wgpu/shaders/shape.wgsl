// Vertex shader for tessellated shape rendering
//
// Handles paths, polygons, strokes, and any tessellated geometry.
// Converts pixel coordinates to clip space using viewport uniform.

// Viewport uniform (for screen-space to clip-space conversion)
struct Viewport {
    size: vec2<f32>,      // Viewport size in pixels
    _padding: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> viewport: Viewport;

struct VertexInput {
    @location(0) position: vec2<f32>,  // Position in pixel coordinates
    @location(1) color: vec4<f32>,     // RGBA color [0-1]
    @location(2) uv: vec2<f32>,        // UV coordinates (unused for now)
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;

    // Convert pixel coordinates to clip space [-1, 1] using viewport uniform
    let clip_x = (input.position.x / viewport.size.x) * 2.0 - 1.0;
    let clip_y = 1.0 - (input.position.y / viewport.size.y) * 2.0; // Flip Y for screen coords

    output.clip_position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    output.color = input.color;

    return output;
}

// Fragment shader

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return input.color;
}
