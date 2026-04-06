// Fill shader for tessellated vector paths
//
// Non-instanced: each vertex carries its own position and color.
// Used for lyon-tessellated path geometry.
//
// Vertex layout matches PathVertex in vertex.rs:
//   @location(0) position: vec2<f32>
//   @location(1) color:    vec4<f32>

// Vertex input
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
}

// Vertex output / Fragment input
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

// Viewport uniform
struct Viewport {
    size: vec2<f32>,
    _padding: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> viewport: Viewport;

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;

    // Convert screen-space position to clip space [-1, 1]
    let clip_x = (input.position.x / viewport.size.x) * 2.0 - 1.0;
    let clip_y = 1.0 - (input.position.y / viewport.size.y) * 2.0;

    output.clip_position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    output.color = input.color;

    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return input.color;
}
