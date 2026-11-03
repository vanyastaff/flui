// resample.wgsl
//
// Resamples a premultiplied RGBA source through an affine map: the output
// pixel whose centre is `p` (in output texels) shows the source at
// `map · p`, in source texels, read through the bound sampler (nearest or
// bilinear). A point outside `decal` reads transparent black; between
// `decal` and the texture edge the sampler clamps.
//
// ## Bind-group layout (group 0)
//
// | Binding | Stage | Type               | Content                        |
// |---------|-------|--------------------|--------------------------------|
// | 0       | FS    | Uniform buffer     | `ResampleUniforms` (64 bytes)  |
// | 1       | FS    | 2D float texture   | Source (premultiplied RGBA)    |
// | 2       | FS    | Filtering sampler  | Nearest or linear, clamped     |
//
// ## Uniform layout (`ResampleUniforms`, 64 bytes)
//
// | Byte offset | Size | Field       | Semantics                             |
// |-------------|------|-------------|---------------------------------------|
// | 0           | 16   | map_x       | `[xx, xy, x0, 0]`: source x of `p`    |
// | 16          | 16   | map_y       | `[yx, yy, y0, 0]`: source y of `p`    |
// | 32          | 16   | decal       | Source texels read, `[l, t, r, b]`    |
// | 48          | 8    | source_size | Source texture size in texels         |
// | 56          | 8    | _pad        | alignment padding                     |

struct ResampleUniforms {
    /// Source x of output point `p` is `dot(map_x.xy, p) + map_x.z`.
    map_x: vec4<f32>,
    /// Source y of output point `p` is `dot(map_y.xy, p) + map_y.z`.
    map_y: vec4<f32>,
    /// The source texels a sample may read, `[left, top, right, bottom]`:
    /// a point outside them reads transparent black.
    decal: vec4<f32>,
    /// Source texture size in texels, to turn a texel position into UV.
    source_size: vec2<f32>,
    /// Alignment padding — not read by the shader.
    _pad: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> u: ResampleUniforms;

@group(0) @binding(1)
var src_texture: texture_2d<f32>;

@group(0) @binding(2)
var src_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
}

// Full-viewport quad: 6 vertices from vertex_index, two CCW triangles.
@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertexOutput {
    let xs = array<f32,6>(-1.0,  1.0, -1.0,  1.0,  1.0, -1.0);
    let ys = array<f32,6>(-1.0, -1.0,  1.0, -1.0,  1.0,  1.0);
    var out: VertexOutput;
    out.position = vec4<f32>(xs[vi], ys[vi], 0.0, 1.0);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // The fragment position is the output pixel's centre, in output texels.
    let p = in.position.xy;
    let source = vec2<f32>(
        dot(u.map_x.xy, p) + u.map_x.z,
        dot(u.map_y.xy, p) + u.map_y.z,
    );
    // Sampled before the decal test, so the sample stays in uniform control
    // flow.
    let texel = textureSample(src_texture, src_sampler, source / u.source_size);
    let inside = all(source >= u.decal.xy) && all(source <= u.decal.zw);
    return select(vec4<f32>(0.0), texel, inside);
}
