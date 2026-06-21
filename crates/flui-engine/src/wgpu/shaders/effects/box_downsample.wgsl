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
//
// ## Bucketed pool support (crop_uv)
//
// When the 2× texture is acquired from a pool bucket that is LARGER than the
// exact supersample dimensions (rounded up to the nearest 64px), the shader
// must only sample the content region. `crop_uv` is `(supersample_w/bucket_w,
// supersample_h/bucket_h)`. The quad always covers NDC [-1,1], but UVs are
// scaled to [0, crop_uv] so only the actual content is read. When the bucket
// exactly equals the supersample (no padding), crop_uv = (1, 1) and the shader
// is identical to the original.

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv:       vec2<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0)       uv:       vec2<f32>,
}

// Crop-UV uniform: scale applied to vertex UV so that only the content region
// of a pooled bucket texture is sampled. (1.0, 1.0) = no padding, full texture.
struct CropUv {
    uv: vec2<f32>,
    _pad: vec2<f32>,
}

@group(0) @binding(0)
var source_texture: texture_2d<f32>;

@group(0) @binding(1)
var linear_sampler: sampler;

@group(0) @binding(2)
var<uniform> crop: CropUv;

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.position = vec4<f32>(in.position, 0.0, 1.0);
    // Scale UV into the content region of the (possibly oversized) bucket texture.
    out.uv = in.uv * crop.uv;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Source dimensions in texels (the full bucket, not the content region).
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
