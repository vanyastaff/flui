// Sweep Gradient Mask Shader
// Applies an angular (sweep/conic) gradient as a mask to the child texture.
// Uses atan2 to compute the angle from a center point, then maps
// the angle range [start_angle, end_angle] to a color interpolation factor.

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
    // Start angle in radians
    start_angle: f32,
    // End angle in radians
    end_angle: f32,
    // Color at start angle
    start_color: vec4<f32>,
    // Color at end angle
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

// Fragment shader - apply sweep gradient mask
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Sample child texture
    let child_color = textureSample(child_texture, child_sampler, in.tex_coords);

    // Calculate angle from center using atan2
    let offset = in.tex_coords - uniforms.center;
    let angle = atan2(offset.y, offset.x);

    // Map angle to [0, 1] interpolation factor within the sweep range
    let angle_range = uniforms.end_angle - uniforms.start_angle;
    var t: f32;
    if (abs(angle_range) > 0.0001) {
        t = (angle - uniforms.start_angle) / angle_range;
    } else {
        // Degenerate gradient (zero range), use start color
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
