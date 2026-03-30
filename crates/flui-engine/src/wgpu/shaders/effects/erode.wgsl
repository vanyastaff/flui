// Morphological Erode (Min) Filter
//
// For each pixel, takes the MINIMUM value of all pixels within the kernel radius.
// Effect: Shrinks bright/opaque areas, makes shapes smaller.
//
// Separable: run horizontal pass (direction=0), then vertical pass (direction=1)
// for O(N) instead of O(N²) per pixel.
//
// Usage:
// - Shrinking shapes / thinning text
// - Removing small bright artifacts
// - Combined with dilate for morphological open/close operations

// Vertex input (fullscreen quad)
struct VertexInput {
    @location(0) position: vec2<f32>,  // [-1, 1]
    @location(1) uv: vec2<f32>,        // [0, 1]
}

// Vertex output
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// Uniforms
struct MorphParams {
    texture_size: vec2<f32>,  // Input texture size in pixels
    radius: f32,              // Kernel radius in pixels
    direction: f32,           // 0.0 = horizontal, 1.0 = vertical
}

@group(0) @binding(0)
var<uniform> params: MorphParams;

@group(0) @binding(1)
var input_texture: texture_2d<f32>;

@group(0) @binding(2)
var input_sampler: sampler;

// =============================================================================
// Vertex Shader (passthrough)
// =============================================================================

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.position = vec4<f32>(input.position, 0.0, 1.0);
    output.uv = input.uv;
    return output;
}

// =============================================================================
// Fragment Shader (Erode = Min filter)
// =============================================================================

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let texel_size = 1.0 / params.texture_size;

    // Direction vector: horizontal (1,0) or vertical (0,1)
    let dir = select(vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0), params.direction > 0.5);

    let r = i32(ceil(params.radius));

    // Initialize to maximum possible value (erode takes min)
    var min_color = vec4<f32>(1.0);

    for (var i = -r; i <= r; i++) {
        let offset = dir * f32(i) * texel_size;
        let sample_color = textureSample(input_texture, input_sampler, input.uv + offset);
        min_color = min(min_color, sample_color);
    }

    return min_color;
}
