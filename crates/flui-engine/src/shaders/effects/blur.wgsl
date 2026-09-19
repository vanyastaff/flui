// blur.wgsl
//
// Separable Gaussian blur — one fragment shader parameterised by `direction`
// covers both the horizontal (U-axis) and vertical (V-axis) sub-passes.
//
// ## Correctness contracts (PINNED #2)
//
// **PINNED #2 — Premultiplied-direct, sRGB-encoded, NO unpremultiply.**
// The source texture is premultiplied RGBA in sRGB-encoded space (as rendered by
// the engine). Gaussian filtering in sRGB space with premultiplied values matches
// Impeller `gaussian_blur_filter_contents.cc:935` (`apply_unpremultiply=false`).
// This avoids the "dark halo" artefact that unpremultiply-first blur produces
// around translucent edges.
//
// **Exact √3-sigma kernel (Impeller match).**
// The kernel half-radius is `ceil(sigma × √3)`, matching Impeller's
// `kKernelRadiusPerSigma = √3` (`sigma.h:24`).  This is a deliberately tight
// kernel; running-sum renormalisation (below) compensates for the truncation.
//
// **Running-sum renormalisation.**
// Weights are accumulated into `tally` and the final `acc` is divided by `tally`
// rather than the theoretical integral. This exactly renormalises the truncated
// kernel: the result is correct even when the kernel is small relative to sigma.
//
// **Decal semantics.**
// wgpu has no `AddressMode::Decal`.  The H pass implements decal in-shader:
// samples outside `content_rect_uv` → `vec4(0.0)`.  The V pass decals at the
// texture edge `[0,1]` (reads the H-pass halo).
//
// **Anisotropic.**
// `sigma` is the sigma for *this* sub-pass: `sigma_x` for the H pass, `sigma_y`
// for the V pass.  The CPU driver (`apply_blur`) selects the sigma per direction.
//
// ## Bind-group layout (group 0)
//
// | Binding | Stage | Type                     | Content                          |
// |---------|-------|--------------------------|----------------------------------|
// | 0       | FS    | Uniform buffer           | `BlurUniforms` (32 bytes)        |
// | 1       | FS    | 2D float texture         | Source (premultiplied RGBA)      |
// | 2       | FS    | Linear-filtering sampler | Bilinear + ClampToEdge           |
//
// ## Vertex stage
//
// No vertex buffer. The VS synthesises 6 vertices from @builtin(vertex_index)
// covering the full viewport ([0,0]→[1,1] NDC quad, two CCW triangles).
// Mirrors morphology.wgsl exactly.
//
// ## Uniform layout (`BlurUniforms`, 32 bytes)
//
// | Byte offset | Size | Field            | Semantics                         |
// |-------------|------|------------------|-----------------------------------|
// | 0           | 8    | texture_size     | Source texture size in pixels     |
// | 8           | 4    | sigma            | Gaussian σ for this sub-pass      |
// | 12          | 4    | direction        | 0.0 = horizontal, 1.0 = vertical  |
// | 16          | 16   | content_rect_uv  | [min_u, min_v, max_u, max_v]      |
//
// Total = 32 bytes ✓

// ─── Uniforms ────────────────────────────────────────────────────────────────

// WGSL uniform-block layout (§13.4.1 / WebGPU §3.13.3):
//   vec2<f32>  →  8 bytes  (align 8)
//   f32        →  4 bytes  (align 4)
//   f32        →  4 bytes  (align 4)   — texture_size + sigma + direction = 16 bytes at offset 0
//   vec4<f32>  →  16 bytes (align 16)  — content_rect_uv at offset 16
// Total = 32 bytes ✓
struct BlurUniforms {
    /// Source texture size in pixels — used to convert kernel offsets to UV.
    texture_size: vec2<f32>,
    /// Gaussian sigma for this sub-pass (sigma_x for H, sigma_y for V).
    sigma: f32,
    /// Pass direction: 0.0 = horizontal (U axis), 1.0 = vertical (V axis).
    direction: f32,
    /// Content rectangle in UV space `[min_u, min_v, max_u, max_v]`.
    ///
    /// H pass: decal at the content rect — samples outside → `vec4(0.0)`.
    /// V pass: `[0.0, 0.0, 1.0, 1.0]` — decal at texture edge to include H halo.
    content_rect_uv: vec4<f32>,
}

@group(0) @binding(0)
var<uniform> u: BlurUniforms;

@group(0) @binding(1)
var src_texture: texture_2d<f32>;

@group(0) @binding(2)
var src_sampler: sampler;

// ─── Vertex stage ─────────────────────────────────────────────────────────────

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0)       uv:       vec2<f32>,
}

// Full-viewport quad: 6 vertices from vertex_index, two CCW triangles.
// Covers NDC [(-1,-1)..(1,1)] with UV [(0,0)..(1,1)].
// Identical pattern to morphology.wgsl — no vertex buffer.
@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertexOutput {
    let xs = array<f32,6>(-1.0,  1.0, -1.0,  1.0,  1.0, -1.0);
    let ys = array<f32,6>(-1.0, -1.0,  1.0, -1.0,  1.0,  1.0);
    let us = array<f32,6>( 0.0,  1.0,  0.0,  1.0,  1.0,  0.0);
    let vs = array<f32,6>( 1.0,  1.0,  0.0,  1.0,  0.0,  0.0);

    var out: VertexOutput;
    out.position = vec4<f32>(xs[vi], ys[vi], 0.0, 1.0);
    out.uv       = vec2<f32>(us[vi], vs[vi]);
    return out;
}

// ─── Fragment stage ───────────────────────────────────────────────────────────

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Degenerate case: sigma <= 0 → identity (kernel_radius = 0, only centre tap).
    if u.sigma <= 0.0 {
        let centre_uv: vec2<f32> = in.uv;
        let inside_centre: bool =
            centre_uv.x >= u.content_rect_uv.x &&
            centre_uv.y >= u.content_rect_uv.y &&
            centre_uv.x <= u.content_rect_uv.z &&
            centre_uv.y <= u.content_rect_uv.w;
        return select(
            vec4<f32>(0.0),
            textureSample(src_texture, src_sampler, centre_uv),
            inside_centre,
        );
    }

    // Texel step size in UV for one pixel.
    let texel_size: vec2<f32> = vec2<f32>(1.0) / u.texture_size;

    // Direction vector: (1, 0) for horizontal (U), (0, 1) for vertical (V).
    let dir: vec2<f32> = select(vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0), u.direction > 0.5);

    // Kernel half-radius in taps: ceil(sigma × √3).
    // √3 = 1.7320508 — Impeller's kKernelRadiusPerSigma (sigma.h:24).
    let r: i32 = i32(ceil(u.sigma * 1.7320508));

    // Accumulate weighted colour and running weight sum.
    // Running-sum normalisation: dividing by `tally` compensates for the
    // truncated tail (kernel sums to ~0.78 of the Gaussian integral at √3·sigma).
    var acc: vec4<f32> = vec4<f32>(0.0);
    var tally: f32 = 0.0;

    for (var i: i32 = -r; i <= r; i++) {
        let sample_uv: vec2<f32> = in.uv + dir * f32(i) * texel_size;

        // Decal guard: samples outside content_rect_uv → vec4(0.0).
        // H pass: content_rect_uv = actual content bounds in UV.
        // V pass: content_rect_uv = [0,1] (texture-edge decal — reads the H halo).
        let inside: bool =
            sample_uv.x >= u.content_rect_uv.x &&
            sample_uv.y >= u.content_rect_uv.y &&
            sample_uv.x <= u.content_rect_uv.z &&
            sample_uv.y <= u.content_rect_uv.w;

        let texel: vec4<f32> = select(
            vec4<f32>(0.0),
            textureSample(src_texture, src_sampler, sample_uv),
            inside,
        );

        // Gaussian weight: exp(-0.5 * i² / σ²).
        // WGSL: no method syntax — use free function `exp()`.
        let weight: f32 = exp(-0.5 * f32(i) * f32(i) / (u.sigma * u.sigma));

        acc   += texel * weight;
        tally += weight;
    }

    // Renormalise: divide accumulated colour by sum of weights.
    // Guard against near-zero tally (only reachable at extreme sigma → r=0).
    if tally > 0.0 {
        return acc / tally;
    }
    return acc;
}
