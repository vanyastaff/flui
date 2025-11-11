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
// Usage Pattern (Rust side)
// =============================================================================
//
// ```rust
// pub struct DualKawaseBlur {
//     downsample_pipeline: RenderPipeline,
//     upsample_pipeline: RenderPipeline,
//     mip_textures: Vec<Texture>,
// }
//
// impl DualKawaseBlur {
//     /// Apply blur with N iterations
//     /// iterations: 1 = light blur, 4 = heavy blur
//     pub fn apply(&mut self, input: &Texture, iterations: u32) -> &Texture {
//         // Downsample chain (shrinking)
//         self.mip_textures[0] = input.clone();
//         for i in 1..=iterations {
//             let src = &self.mip_textures[i - 1];
//             let dst = &mut self.mip_textures[i];
//
//             // Each iteration halves resolution
//             render_pass.set_pipeline(&self.downsample_pipeline);
//             render_pass.set_bind_group(0, &src.bind_group, &[]);
//             render_pass.draw(0..6, 0..1); // Fullscreen quad
//         }
//
//         // Upsample chain (growing)
//         for i in (0..iterations).rev() {
//             // See blur_upsample.wgsl
//         }
//
//         &self.mip_textures[0]
//     }
// }
//
// // Typical blur levels:
// // iterations=1: radius ~ 4px  (light blur for glass effect)
// // iterations=2: radius ~ 8px  (medium blur for backdrops)
// // iterations=3: radius ~ 16px (heavy blur for focus effects)
// // iterations=4: radius ~ 32px (extreme blur for backgrounds)
// ```
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
