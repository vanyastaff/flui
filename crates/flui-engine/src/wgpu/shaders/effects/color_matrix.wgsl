// color_matrix.wgsl
//
// Per-pixel 5×4 color-matrix filter applied to a premultiplied layer offscreen.
//
// ## Correctness contract
//
// The layer texture is premultiplied RGBA.  The color-matrix spec (CSS Filter
// Effects §10.2, `ColorMatrix::apply` in flui-types) operates on **straight**
// (un-premultiplied) RGBA.  This shader therefore:
//
//   1. Samples the source texel `t` (premultiplied RGBA).
//   2. Un-premultiplies: straight = t.rgb / t.a  (guard: if t.a == 0 → vec3(0)).
//   3. Applies the 5×4 color matrix on straight RGBA:
//        out.r = dot(M_row0, vec4(r,g,b,a)) + offset.r
//        out.g = dot(M_row1, vec4(r,g,b,a)) + offset.g
//        out.b = dot(M_row2, vec4(r,g,b,a)) + offset.b
//        out.a = dot(M_row3, vec4(r,g,b,a)) + offset.a
//      (u.m is uploaded as the columns of M so that `u.m * straight` computes
//       the row dot-products via WGSL's column-major mat*vec.)
//   4. Clamps every output channel to [0, 1].
//   5. Re-premultiplies: out.rgb *= out.a
//   6. Emits with BlendState::REPLACE (no fixed-function blending).
//
// This sequence is bit-identical to `ColorMatrix::apply` on the straight color.
//
// ## Bind-group layout (group 0)
//
// | Binding | Stage | Type                  | Content                      |
// |---------|-------|-----------------------|------------------------------|
// | 0       | FS    | Uniform buffer        | `ColorMatrixUniforms`        |
// | 1       | FS    | 2D float texture      | Source layer (premultiplied) |
// | 2       | FS    | Non-filtering sampler | Nearest + ClampToEdge        |
//
// ## Vertex stage
//
// No vertex buffer.  The VS synthesises 6 vertices from @builtin(vertex_index)
// covering the full viewport ([0,0]→[1,1] NDC quad, two triangles).

// ─── Uniforms ────────────────────────────────────────────────────────────────

// WGSL uniform-block layout (§13.4.1 / WebGPU §3.13.3):
//   mat4x4<f32>  →  64 bytes (align 16)
//   vec4<f32>    →  16 bytes (align 16)
// Total: 80 bytes (multiple of 16 ✓).
//
// Rust side: `ColorMatrixUniform { m: [[f32;4];4], offset: [f32;4] }` = 80 bytes.
// `m[i]` is COLUMN i of the weight matrix M (see `ColorMatrixUniform::from_values`).
// Because WGSL `mat4x4<f32>` is column-major, the Rust `[[f32;4];4]` blocks are
// read as columns.  Packing column i of M into m[i] means WGSL's `u.m` equals M,
// and `u.m * straight` computes the correct row dot-products.
struct ColorMatrixUniforms {
    /// Columns 0..3 of the 4×4 weight matrix M.
    /// WGSL reads each [f32;4] block as a column, so u.m == M.
    m: mat4x4<f32>,
    /// Additive bias for each output channel (R/G/B/A).
    offset: vec4<f32>,
}

@group(0) @binding(0)
var<uniform> u: ColorMatrixUniforms;

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
@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertexOutput {
    // Triangle 0: verts 0,1,2  Triangle 1: verts 3,4,5
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
    // Step 1 — sample the premultiplied source texel.
    let t: vec4<f32> = textureSample(src_texture, src_sampler, in.uv);

    // Step 2 — un-premultiply (divide-by-zero guard: if α=0 the RGB is black).
    let straight_rgb: vec3<f32> = select(vec3<f32>(0.0), t.rgb / t.a, t.a > 0.0);
    let straight: vec4<f32> = vec4<f32>(straight_rgb, t.a);

    // Step 3 — apply the 5×4 matrix on straight RGBA.
    //   u.m is mat4x4<f32> whose column i was packed with column i of M on the
    //   Rust side (see ColorMatrixUniform::from_values).  WGSL column-major
    //   mat*vec: (u.m * straight)[i] = Σ_j u.m[j][i] * straight[j]
    //                                = Σ_j M[i,j] * straight[j]   ✓ (row dot-product)
    let out_straight: vec4<f32> = u.m * straight + u.offset;

    // Step 4 — clamp every output channel to [0, 1].
    let clamped: vec4<f32> = clamp(out_straight, vec4<f32>(0.0), vec4<f32>(1.0));

    // Step 5 — re-premultiply: rgb *= a.
    let out_rgb: vec3<f32> = clamped.rgb * clamped.a;

    // Step 6 — emit premultiplied RGBA (BlendState::REPLACE, no HW blending).
    return vec4<f32>(out_rgb, clamped.a);
}
