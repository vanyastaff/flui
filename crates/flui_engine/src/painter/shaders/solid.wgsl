// Solid color shader for basic shapes (rectangles, circles, lines)
//
// This shader renders solid-colored geometry with premultiplied alpha.
// Vertices are provided in screen-space coordinates and converted to NDC.

struct Uniforms {
    viewport_size: vec2<f32>,
    _padding: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;

    // Convert screen-space coordinates to NDC (Normalized Device Coordinates)
    // Screen: [0, viewport_size] -> NDC: [-1, 1]
    let ndc_x = (input.position.x / uniforms.viewport_size.x) * 2.0 - 1.0;
    let ndc_y = 1.0 - (input.position.y / uniforms.viewport_size.y) * 2.0;

    output.clip_position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    output.color = input.color;

    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // Return premultiplied alpha color
    return input.color;
}
