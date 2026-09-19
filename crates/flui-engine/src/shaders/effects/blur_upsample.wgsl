// Dual Kawase Blur - Upsample Pass
//
// Part 2 of 2-pass algorithm (downsample → upsample)
// Combines blurred mip levels back to original resolution
//
// This pass uses a tent filter (weighted 9-tap) for smooth upsampling

// Vertex input (fullscreen quad)
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
}

// Vertex output
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// Uniforms
struct BlurParams {
    texture_size: vec2<f32>,
    offset: f32,
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
// Fragment Shader (Upsample with tent filter)
// =============================================================================

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let texel_size = 1.0 / params.texture_size;
    let offset = texel_size * params.offset;

    // 9-tap tent filter for smooth upsampling
    //
    // Pattern with weights:
    //   1   2   1
    //   2   4   2    (center weighted 4x, diagonal 2x, cardinal 1x)
    //   1   2   1
    //
    // Total weight: 16

    // Center (4x weight)
    var color = textureSample(input_texture, input_sampler, input.uv) * 4.0;

    // 4 diagonal samples (2x weight each)
    color += textureSample(input_texture, input_sampler, input.uv + vec2<f32>(-offset.x, -offset.y)) * 2.0;
    color += textureSample(input_texture, input_sampler, input.uv + vec2<f32>(offset.x, -offset.y)) * 2.0;
    color += textureSample(input_texture, input_sampler, input.uv + vec2<f32>(-offset.x, offset.y)) * 2.0;
    color += textureSample(input_texture, input_sampler, input.uv + vec2<f32>(offset.x, offset.y)) * 2.0;

    // 4 cardinal samples (1x weight each)
    let offset2 = offset * 2.0;
    color += textureSample(input_texture, input_sampler, input.uv + vec2<f32>(-offset2.x, 0.0));
    color += textureSample(input_texture, input_sampler, input.uv + vec2<f32>(offset2.x, 0.0));
    color += textureSample(input_texture, input_sampler, input.uv + vec2<f32>(0.0, -offset2.y));
    color += textureSample(input_texture, input_sampler, input.uv + vec2<f32>(0.0, offset2.y));

    // Normalize by total weight (4 + 4*2 + 4*1 = 16)
    return color / 16.0;
}

// =============================================================================
// Usage (Rust side)
// =============================================================================
//
// This shader is the upsample half of the separable Gaussian filter, driven by
// `wgpu::blur::apply_blur` (`crates/flui-engine/src/wgpu/blur/mod.rs`), which
// owns the pipeline, the ping-pong textures, and the iteration count. It is
// reached from `WgpuPainter` through `ImageFilterPass::Blur`; an embedder
// applies a blur by pushing an `ImageFilter::Blur` onto a layer, not by driving
// this shader directly. The `offscreen/blur.rs` Dual Kawase path is a separate
// consumer of the same idea for backdrop filters.
//
// =============================================================================
// Advanced: Adaptive Blur (variable blur per region)
// =============================================================================
//
// You can modulate blur strength spatially using a mask texture:
//
// ```wgsl
// @group(1) @binding(0)
// var blur_mask: texture_2d<f32>;
//
// @fragment
// fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
//     let mask = textureSample(blur_mask, input_sampler, input.uv).r;
//     let sharp = textureSample(input_texture, input_sampler, input.uv);
//     let blurred = /* ... tent filter ... */;
//     return mix(sharp, blurred, mask);
// }
// ```
//
// Use cases:
// - Depth of field (blur based on depth)
// - Radial blur (blur increases with distance from center)
// - Selective focus (UI element sharp, background blurred)
