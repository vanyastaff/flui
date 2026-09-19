// Fullscreen blit — copies an intermediate texture 1:1 onto the swapchain surface.
//
// Used by the COPY_SRC-less present path (intermediate-active mode): when the
// swapchain surface does not support COPY_SRC, the frame is rendered into a
// pooled intermediate texture that *does* support COPY_SRC, then this shader
// blits the intermediate into the real surface view with no blend (Replace/Copy).
//
// Guarantees:
// - Nearest-neighbour sampling + 1:1 UV → pixel-identical to a direct render.
// - No blend equation on the output attachment (`ColorTargetState::blend = None`)
//   so every texel of the surface is replaced, never composited.

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@group(0) @binding(0)
var intermediate_texture: texture_2d<f32>;

@group(0) @binding(1)
var nearest_sampler: sampler;

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.position = vec4<f32>(input.position, 0.0, 1.0);
    output.uv = input.uv;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // Nearest-neighbour sample: 1:1 blit, same texel for every screen pixel.
    return textureSample(intermediate_texture, nearest_sampler, input.uv);
}
