// Solid Color Mask Shader
// Applies a uniform solid color as a mask to the child texture

// Vertex shader inputs
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) tex_coords: vec2<f32>,
}

// Vertex shader outputs / Fragment shader inputs
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
}

// Uniform data
struct Uniforms {
    // Solid mask color (RGBA)
    mask_color: vec4<f32>,
}

// Bindings
@group(0) @binding(0) var child_texture: texture_2d<f32>;
@group(0) @binding(1) var child_sampler: sampler;
@group(0) @binding(2) var<uniform> uniforms: Uniforms;

// Vertex shader - simple passthrough
@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(in.position, 0.0, 1.0);
    out.tex_coords = in.tex_coords;
    return out;
}

// Fragment shader - apply solid color mask
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Sample child texture
    let child_color = textureSample(child_texture, child_sampler, in.tex_coords);

    // Apply solid mask: multiply child alpha by mask alpha
    let mask_alpha = uniforms.mask_color.a;
    let masked_color = vec4<f32>(
        child_color.rgb,
        child_color.a * mask_alpha
    );

    return masked_color;
}
