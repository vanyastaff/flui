// Box-filter 2×→1× downsample for SSAA path anti-aliasing.
//
// Reads a 2× supersampled source texture (premultiplied RGBA) and averages the
// four texels that correspond to each output pixel.  Produces a premultiplied
// tile that can be composited directly via the premultiplied texture pipeline.
//
// Sampling layout (2× → 1×):
//   For an output pixel at UV (u, v), the four covered source texel centres are at:
//     (u ∓ 0.5/src_w, v ∓ 0.5/src_h)   — the 2×2 source texels under this output pixel
//
// One output pixel spans 2/src_dim in UV (the source is 2× the output size), so the
// four source-texel centres sit at ±(half the output-pixel UV span)/2 = ±0.5/src_dim
// from the output-pixel centre. With linear filtering, sampling exactly at a source
// texel centre returns that texel's value, so the four taps give the exact 2×2 box
// average (ordered-grid 2× SSAA). The code below uses `0.5/src_dim` — matching this.
//
// Averaging premultiplied values is correct: premultiplied colour is linear in
// coverage, so the mean of four coverage-weighted colours equals the
// coverage-weighted mean colour — no artefact at transparent edges.

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv:       vec2<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0)       uv:       vec2<f32>,
}

@group(0) @binding(0)
var source_texture: texture_2d<f32>;

@group(0) @binding(1)
var linear_sampler: sampler;

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.position = vec4<f32>(in.position, 0.0, 1.0);
    out.uv = in.uv;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Source dimensions in texels.
    let src_dim = vec2<f32>(textureDimensions(source_texture));

    // Half a sub-texel offset in UV space: sub-texel = 1/src_dim texels,
    // so the tap offset to the sub-texel centre = 0.5 * (1/src_dim) = 0.5/src_dim.
    let half_texel = 0.5 / src_dim;

    // Four-tap box filter: sample the four sub-texels covering this output pixel.
    let tl = textureSample(source_texture, linear_sampler, in.uv + vec2<f32>(-half_texel.x, -half_texel.y));
    let tr = textureSample(source_texture, linear_sampler, in.uv + vec2<f32>( half_texel.x, -half_texel.y));
    let bl = textureSample(source_texture, linear_sampler, in.uv + vec2<f32>(-half_texel.x,  half_texel.y));
    let br = textureSample(source_texture, linear_sampler, in.uv + vec2<f32>( half_texel.x,  half_texel.y));

    // Uniform 4-tap average.  Premultiplied averaging is linear-correct.
    return (tl + tr + bl + br) * 0.25;
}
