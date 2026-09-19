// Fill shader for solid colors and gradients
//
// This shader handles basic 2D rendering with vertex colors.
// It supports both solid fills and per-vertex color interpolation.

// Uniform buffer containing view-projection matrix and viewport info
struct Uniforms {
    view_proj: mat4x4<f32>,
    viewport_size: vec4<f32>,
    time: f32,
    _padding: vec3<f32>,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

// Vertex shader input
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) uv: vec2<f32>,
}

// Vertex shader output / Fragment shader input
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
}

// Vertex shader
@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;

    // Transform position to clip space
    output.clip_position = uniforms.view_proj * vec4<f32>(input.position, 0.0, 1.0);

    // Pass through color and UV
    output.color = input.color;
    output.uv = input.uv;

    return output;
}

// Fragment shader
@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // Simply output the interpolated vertex color
    // This gives us smooth gradients from vertex colors
    return input.color;
}
