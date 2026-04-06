// Linear Gradient Mask Shader
// Applies a linear gradient as a mask to the child texture

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
    // Gradient start point (normalized 0.0-1.0)
    start: vec2<f32>,
    // Gradient end point (normalized 0.0-1.0)
    end: vec2<f32>,
    // Color at start
    start_color: vec4<f32>,
    // Color at end
    end_color: vec4<f32>,
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

// Fragment shader - apply linear gradient mask
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Sample child texture
    let child_color = textureSample(child_texture, child_sampler, in.tex_coords);

    // Calculate gradient interpolation factor (t)
    // Project current point onto gradient line
    let gradient_dir = uniforms.end - uniforms.start;
    let point_vec = in.tex_coords - uniforms.start;

    // t = dot(point_vec, gradient_dir) / dot(gradient_dir, gradient_dir)
    let gradient_length_sq = dot(gradient_dir, gradient_dir);
    var t: f32;
    if (gradient_length_sq > 0.0001) {
        t = dot(point_vec, gradient_dir) / gradient_length_sq;
    } else {
        // Degenerate gradient (start == end), use start color
        t = 0.0;
    }

    // Clamp t to [0, 1]
    t = clamp(t, 0.0, 1.0);

    // Interpolate between start and end colors
    let gradient_color = mix(uniforms.start_color, uniforms.end_color, t);

    // Apply gradient mask: multiply child alpha by gradient alpha
    let masked_color = vec4<f32>(
        child_color.rgb,
        child_color.a * gradient_color.a
    );

    return masked_color;
}
