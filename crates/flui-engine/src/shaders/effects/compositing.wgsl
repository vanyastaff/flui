// Compositing shader for SaveLayer/RestoreLayer offscreen target blending.
//
// Renders an offscreen texture as a textured quad back to the parent
// render target with configurable opacity.

struct Viewport {
    size: vec2<f32>,
    _padding: vec2<f32>,
}
@group(0) @binding(0) var<uniform> viewport: Viewport;

struct CompositeUniforms {
    // Destination bounds in pixels: x, y, width, height
    bounds: vec4<f32>,
    // Layer opacity multiplier
    opacity: f32,
    _padding: vec3<f32>,
}
@group(1) @binding(0) var<uniform> composite: CompositeUniforms;
@group(1) @binding(1) var t_source: texture_2d<f32>;
@group(1) @binding(2) var s_source: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@location(0) quad_pos: vec2<f32>) -> VertexOutput {
    var out: VertexOutput;
    // quad_pos is in [0,1] range from the unit quad
    let pixel_pos = composite.bounds.xy + quad_pos * composite.bounds.zw;
    // Convert to clip space
    out.position = vec4<f32>(
        pixel_pos.x / viewport.size.x * 2.0 - 1.0,
        1.0 - pixel_pos.y / viewport.size.y * 2.0,
        0.0,
        1.0
    );
    out.uv = quad_pos;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(t_source, s_source, in.uv);
    return vec4<f32>(color.rgb, color.a * composite.opacity);
}
