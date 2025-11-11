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
// Complete Usage Example (Rust side)
// =============================================================================
//
// ```rust
// pub struct DualKawaseBlur {
//     downsample_pipeline: RenderPipeline,
//     downsample_bind_group_layout: BindGroupLayout,
//     upsample_pipeline: RenderPipeline,
//     upsample_bind_group_layout: BindGroupLayout,
//     mip_textures: Vec<Texture>,
//     sampler: Sampler,
// }
//
// impl DualKawaseBlur {
//     pub fn new(device: &Device, max_iterations: u32) -> Self {
//         // Create mip chain (each level half the size)
//         let mut mip_textures = Vec::new();
//         let mut size = initial_size;
//
//         for _ in 0..=max_iterations {
//             mip_textures.push(device.create_texture(&TextureDescriptor {
//                 size: Extent3d { width: size.width, height: size.height, depth: 1 },
//                 format: TextureFormat::Rgba8Unorm,
//                 usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
//                 // ...
//             }));
//             size.width /= 2;
//             size.height /= 2;
//         }
//
//         // Linear sampler for smooth blending
//         let sampler = device.create_sampler(&SamplerDescriptor {
//             mag_filter: FilterMode::Linear,
//             min_filter: FilterMode::Linear,
//             // ...
//         });
//
//         Self { /* ... */ }
//     }
//
//     pub fn apply(&mut self, encoder: &mut CommandEncoder,
//                  input: &Texture, iterations: u32) -> &Texture {
//         // 1. Copy input to mip[0]
//         encoder.copy_texture_to_texture(input, &self.mip_textures[0], /* ... */);
//
//         // 2. Downsample chain (blur and shrink)
//         for i in 0..iterations {
//             let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
//                 color_attachments: &[RenderPassColorAttachment {
//                     view: &self.mip_textures[i + 1].create_view(&Default::default()),
//                     // ...
//                 }],
//                 // ...
//             });
//
//             pass.set_pipeline(&self.downsample_pipeline);
//             pass.set_bind_group(0, &self.create_bind_group(i), &[]);
//             pass.draw(0..6, 0..1); // Fullscreen quad
//         }
//
//         // 3. Upsample chain (blend and grow)
//         for i in (0..iterations).rev() {
//             let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
//                 color_attachments: &[RenderPassColorAttachment {
//                     view: &self.mip_textures[i].create_view(&Default::default()),
//                     load: LoadOp::Load,  // Blend with existing content
//                     // ...
//                 }],
//                 // ...
//             });
//
//             pass.set_pipeline(&self.upsample_pipeline);
//             pass.set_bind_group(0, &self.create_bind_group(i + 1), &[]);
//             pass.draw(0..6, 0..1);
//         }
//
//         // 4. Return blurred result
//         &self.mip_textures[0]
//     }
// }
//
// // High-level API usage:
// let blur = DualKawaseBlur::new(&device, 4);
//
// // Glass panel effect
// let blurred_background = blur.apply(&mut encoder, &background_texture, 3);
// painter.texture(panel_bounds, blurred_background);
// painter.rect(panel_bounds, Color::rgba(255, 255, 255, 0.1)); // Tint overlay
//
// // Bloom effect
// let bright_pass = extract_bright_pixels(&scene);
// let bloom = blur.apply(&mut encoder, &bright_pass, 4);
// blend_additive(&scene, &bloom);
// ```
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
