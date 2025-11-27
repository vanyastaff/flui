// Radial Gradient Mask Shader
// Applies a radial gradient as a mask to the child texture

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
    // Gradient center point (normalized 0.0-1.0)
    center: vec2<f32>,
    // Gradient radius (normalized)
    radius: f32,
    // Padding for alignment
    _padding: f32,
    // Color at center
    center_color: vec4<f32>,
    // Color at edge
    edge_color: vec4<f32>,
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

// Fragment shader - apply radial gradient mask
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Sample child texture
    let child_color = textureSample(child_texture, child_sampler, in.tex_coords);

    // Calculate distance from center
    let offset = in.tex_coords - uniforms.center;
    let distance = length(offset);

    // Calculate gradient interpolation factor (t)
    // t = 0.0 at center, t = 1.0 at radius
    var t: f32;
    if (uniforms.radius > 0.0001) {
        t = distance / uniforms.radius;
    } else {
        // Degenerate gradient (radius == 0), use center color
        t = 0.0;
    }

    // Clamp t to [0, 1]
    t = clamp(t, 0.0, 1.0);

    // Interpolate between center and edge colors
    let gradient_color = mix(uniforms.center_color, uniforms.edge_color, t);

    // Apply gradient mask: multiply child alpha by gradient alpha
    let masked_color = vec4<f32>(
        child_color.rgb,
        child_color.a * gradient_color.a
    );

    return masked_color;
}
