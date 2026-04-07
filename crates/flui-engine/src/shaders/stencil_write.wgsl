// Stencil write shader for non-rectangular clipping.
//
// Renders clip geometry into the stencil buffer without producing any
// visible color output.  The stencil operation (increment or decrement)
// is configured on the pipeline, not in the shader.

struct Viewport {
    size: vec2<f32>,
    _padding: vec2<f32>,
}
@group(0) @binding(0) var<uniform> viewport: Viewport;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,  // unused but matches PathVertex layout
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let clip_x = in.position.x / viewport.size.x * 2.0 - 1.0;
    let clip_y = 1.0 - in.position.y / viewport.size.y * 2.0;
    out.position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    return out;
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0, 0.0, 0.0, 0.0);
}
