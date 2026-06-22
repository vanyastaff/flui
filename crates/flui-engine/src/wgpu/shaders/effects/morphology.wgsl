// morphology.wgsl
//
// Separable morphological filter — dilate (max) or erode (min) — over a
// premultiplied RGBA offscreen.
//
// ## Correctness contracts (PINNED)
//
// **PINNED #1 — Premultiplied-direct, NO unpremultiply.**
// Morphology max/min operates directly on premultiplied RGBA channel values.
// Unlike the color-matrix filter, morphology does NOT unpremultiply before the
// operation.  This matches Impeller `morphology_filter.frag` semantics: the
// correct result for a morphological filter is the per-channel maximum or
// minimum of the premultiplied texels in the kernel window.
//
// **Decal, not ClampToEdge.**
// wgpu has no `AddressMode::Decal`.  This shader implements decal semantics
// in-shader: any sample whose UV falls outside `u.content_rect_uv` is treated
// as `vec4(0)` (transparent black) for dilate, or `vec4(1)` (opaque white) for
// erode — i.e., the neutral element of the accumulation.  This ensures that the
// filter does not bleed content from beyond the content rectangle into the output.
//
// **One shader, two ops.**
// `u.op == 0.0` → dilate (init=`vec4(0)`, accumulate `max`).
// `u.op == 1.0` → erode  (init=`vec4(1)`, accumulate `min`).
// H/V passes are separate `apply_morphology` sub-calls on the CPU; the GPU sees
// a single direction per draw.
//
// ## Bind-group layout (group 0)
//
// | Binding | Stage | Type                  | Content                          |
// |---------|-------|-----------------------|----------------------------------|
// | 0       | FS    | Uniform buffer        | `MorphUniforms` (48 bytes)       |
// | 1       | FS    | 2D float texture      | Source (premultiplied RGBA)      |
// | 2       | FS    | Non-filtering sampler | Nearest + ClampToEdge            |
//
// ## Vertex stage
//
// No vertex buffer.  The VS synthesises 6 vertices from @builtin(vertex_index)
// covering the full viewport ([0,0]→[1,1] NDC quad, two CCW triangles).
// Mirrors color_matrix.wgsl exactly.
//
// ## Uniform layout (`MorphUniforms`, 48 bytes)
//
// | Byte offset | Size | Field            | Semantics                         |
// |-------------|------|------------------|-----------------------------------|
// | 0           | 8    | texture_size     | Source texture size in pixels     |
// | 8           | 4    | radius           | Kernel half-radius in pixels      |
// | 12          | 4    | direction        | 0.0 = horizontal, 1.0 = vertical  |
// | 16          | 16   | content_rect_uv  | [min_u, min_v, max_u, max_v]      |
// | 32          | 4    | op               | 0.0 = dilate, 1.0 = erode         |
// | 36          | 12   | _pad             | alignment padding                  |

// ─── Uniforms ────────────────────────────────────────────────────────────────

// WGSL uniform-block layout (§13.4.1 / WebGPU §3.13.3):
//   vec2<f32>  →  8 bytes  (align 8)
//   f32        →  4 bytes  (align 4)
//   f32        →  4 bytes  (align 4)    — texture_size + radius + direction = 16 bytes at offset 0
//   vec4<f32>  →  16 bytes (align 16)   — content_rect_uv at offset 16
//   f32        →  4 bytes  (align 4)
//   vec3<f32>  →  12 bytes (align 16 ← vec3 aligns to 16 in WGSL)
// ── WGSL vec3<f32> has align 16, so `_pad` starts at offset 36 with 12 bytes
// Total in Rust: texture_size(8) + radius(4) + direction(4) + content_rect_uv(16) + op(4) + _pad(12) = 48
//
// In WGSL the struct layout is:
//   offset  0: vec2<f32>  texture_size    (align 8, size 8)
//   offset  8: f32        radius          (align 4, size 4)
//   offset 12: f32        direction       (align 4, size 4)
//   offset 16: vec4<f32>  content_rect_uv (align 16, size 16)
//   offset 32: f32        op              (align 4, size 4)
//   offset 36: f32        _pad0           (align 4, size 4)
//   offset 40: f32        _pad1           (align 4, size 4)
//   offset 44: f32        _pad2           (align 4, size 4)
// Total = 48 bytes ✓
struct MorphUniforms {
    /// Source texture size in pixels — used to convert kernel offsets to UV.
    texture_size: vec2<f32>,
    /// Kernel half-radius in pixels: sample `[-ceil(radius) .. ceil(radius)]`.
    radius: f32,
    /// Pass direction: 0.0 = horizontal (U axis), 1.0 = vertical (V axis).
    direction: f32,
    /// Content rectangle in UV space `[min_u, min_v, max_u, max_v]`.
    ///
    /// Samples outside this rectangle are replaced by the neutral element
    /// (vec4(0) for dilate, vec4(1) for erode) — decal semantics.
    content_rect_uv: vec4<f32>,
    /// Operation selector: 0.0 = dilate (max), 1.0 = erode (min).
    op: f32,
    /// Alignment padding — not read by the shader.
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
}

@group(0) @binding(0)
var<uniform> u: MorphUniforms;

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
// Identical pattern to color_matrix.wgsl — no vertex buffer.
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
    // Texel step size in UV for one pixel in each axis.
    let texel_size: vec2<f32> = vec2<f32>(1.0) / u.texture_size;

    // Direction vector: (1, 0) for horizontal (U), (0, 1) for vertical (V).
    let dir: vec2<f32> = select(vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0), u.direction > 0.5);

    // Integer kernel half-radius (rounded up so fractional radii include the
    // partial-coverage texel — conservative morphology).
    let r: i32 = i32(ceil(u.radius));

    // Initialise the accumulator to the neutral element for the operation:
    //   dilate (op ≈ 0): neutral = vec4(0) — max with 0 is transparent black.
    //   erode  (op ≈ 1): neutral = vec4(1) — min with 1 is opaque white.
    var acc: vec4<f32> = select(vec4<f32>(0.0), vec4<f32>(1.0), u.op > 0.5);

    for (var i: i32 = -r; i <= r; i++) {
        let sample_uv: vec2<f32> = in.uv + dir * f32(i) * texel_size;

        // Decal guard: if the SAMPLE position is outside the decal rectangle
        // (`content_rect_uv`) in UV space, substitute TRANSPARENT BLACK instead of
        // sampling the texture.  This implements decal semantics (wgpu has no
        // AddressMode::Decal), matching Impeller `morphology_filter.frag`, whose
        // out-of-bounds sample is `vec4(0)` for BOTH ops (only the accumulator
        // INIT above is op-dependent).  Using `vec4(0)` is what makes erode shrink
        // at a decal boundary (`min(acc, 0) == 0`); an op-dependent neutral here
        // (the old `vec4(1)` for erode) was a no-op and silently disabled
        // edge-erosion — a parity bug vs Flutter/Impeller.
        let inside: bool =
            sample_uv.x >= u.content_rect_uv.x &&
            sample_uv.y >= u.content_rect_uv.y &&
            sample_uv.x <= u.content_rect_uv.z &&
            sample_uv.y <= u.content_rect_uv.w;

        // Sample the premultiplied source, or transparent black (decal) outside.
        let texel: vec4<f32> = select(
            vec4<f32>(0.0),
            textureSample(src_texture, src_sampler, sample_uv),
            inside,
        );

        // Accumulate: dilate = max, erode = min.
        acc = select(max(acc, texel), min(acc, texel), u.op > 0.5);
    }

    // Output is premultiplied RGBA — no repremultiplication needed (PINNED #1).
    return acc;
}
