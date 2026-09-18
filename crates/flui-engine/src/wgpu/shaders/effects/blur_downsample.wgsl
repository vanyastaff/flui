// Dual Kawase Blur - Downsample Pass
//
// Fast, high-quality blur for UI effects (glass, backdrop, etc.)
// Part 1 of 2-pass algorithm (downsample → upsample)
//
// Algorithm: Dual Kawase (Masaki Kawase, 2003)
// Used in: KDE Plasma, Unity, many mobile games
//
// Performance:
// - 5-10x faster than naive Gaussian blur
// - Logarithmic scaling (doubling blur = +2 passes)
// - Quality close to Gaussian for UI
//
// Typical usage:
// - Glass/frosted glass effects
// - Backdrop blur (iOS-style)
// - Bloom post-processing
// - Depth of field

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
struct BlurParams {
    texture_size: vec2<f32>,  // Input texture size
    offset: f32,              // Sample offset multiplier
    _padding: f32,
}

@group(0) @binding(0)
var<uniform> params: BlurParams;

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
// Fragment Shader (Downsample)
// =============================================================================

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // Pixel size in UV coordinates
    let texel_size = 1.0 / params.texture_size;
    let offset = texel_size * params.offset;

    // 5-tap pattern (center + 4 diagonal corners)
    // This gives excellent blur quality with minimal samples
    //
    // Pattern:
    //    X   X
    //      C      (C = center, X = sample)
    //    X   X

    // Center sample (weighted 4x for importance)
    var color = textureSample(input_texture, input_sampler, input.uv) * 4.0;

    // 4 diagonal samples (chess pattern)
    color += textureSample(input_texture, input_sampler, input.uv + vec2<f32>(-offset.x, -offset.y));
    color += textureSample(input_texture, input_sampler, input.uv + vec2<f32>(offset.x, -offset.y));
    color += textureSample(input_texture, input_sampler, input.uv + vec2<f32>(-offset.x, offset.y));
    color += textureSample(input_texture, input_sampler, input.uv + vec2<f32>(offset.x, offset.y));

    // Average (4 samples + 4x center = 8 total weight)
    return color / 8.0;
}

// =============================================================================
// Usage (Rust side)
// =============================================================================
//
// This is the downsample half of the Dual Kawase blur used by backdrop
// filters, driven by `wgpu::offscreen::blur::render_blur`
// (`crates/flui-engine/src/wgpu/offscreen/blur.rs`), which owns the pipeline
// and the mip chain. An embedder reaches it by pushing a backdrop blur onto a
// layer (`SceneBuilder::push_backdrop_blur`), not by driving this shader.
//
// Typical iteration counts, as the blur radius grows: 1 ≈ 4 px, 2 ≈ 8 px,
// 3 ≈ 16 px, 4 ≈ 32 px. The separable Gaussian path
// (`wgpu::blur::apply_blur`) is the other filter in this directory and has its
// own shaders.
//
// =============================================================================
// Performance Characteristics
// =============================================================================
//
// Compared to naive Gaussian:
// - Gaussian: O(R²) per pixel (R = blur radius)
// - Kawase: O(log R) passes, constant samples per pass
//
// Example: 32px blur radius
// - Naive Gaussian: ~1024 samples per pixel (32²)
// - Dual Kawase: 4 iterations × 5 samples = 20 samples total (51x faster!)
//
// Memory: N mip levels (each half size) = 33% extra memory
// - 1920x1080 → 960x540 → 480x270 → 240x135 → 120x68
// - Total: ~1.33x original texture size
//
// Quality: Very close to Gaussian for UI (perceptually identical)
