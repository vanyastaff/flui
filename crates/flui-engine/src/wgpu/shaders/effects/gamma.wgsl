// gamma.wgsl
//
// Per-pixel sRGB gamma transfer function applied to a premultiplied layer
// offscreen.  Only RGB channels are transformed; alpha is always passed through
// unchanged.
//
// ## Correctness contract
//
// The layer texture is premultiplied RGBA.  The gamma transfer operates on
// **straight** (un-premultiplied) RGB — this shader therefore:
//
//   1. Samples the source texel `t` (premultiplied RGBA).
//   2. Un-premultiplies: straight.rgb = t.rgb / t.a  (guard: if t.a == 0 → vec3(0)).
//   3. Applies the IEC 61966-2-1 piecewise transfer to each RGB channel:
//        direction == 0 (SrgbToLinear): linear = srgb_to_linear(c)
//        direction == 1 (LinearToSrgb): srgb   = linear_to_srgb(c)
//      Alpha is LEFT UNCHANGED.
//   4. Clamps every output channel to [0, 1].
//   5. Re-premultiplies: out.rgb *= out.a
//   6. Emits with BlendState::REPLACE (no fixed-function blending).
//
// This sequence is bit-identical to `flui_types::styling::color::srgb_to_linear` /
// `linear_to_srgb` applied to each straight RGB channel (the CPU oracle).
//
// ## Direction encoding
//
// `u.direction`:
//   0u → SrgbToLinear (GammaDirection::SrgbToLinear in Rust)
//   1u → LinearToSrgb (GammaDirection::LinearToSrgb in Rust)
// Must match `gamma_direction_to_u32` in `gamma/pipeline.rs`.
//
// ## Bind-group layout (group 0)
//
// | Binding | Stage | Type                  | Content                      |
// |---------|-------|-----------------------|------------------------------|
// | 0       | FS    | Uniform buffer        | `GammaUniforms`              |
// | 1       | FS    | 2D float texture      | Source layer (premultiplied) |
// | 2       | FS    | Non-filtering sampler | Nearest + ClampToEdge        |
//
// ## Vertex stage
//
// No vertex buffer.  The VS synthesises 6 vertices from @builtin(vertex_index)
// covering the full viewport ([0,0]→[1,1] NDC quad, two triangles).

// ─── Uniforms ─────────────────────────────────────────────────────────────────

// WGSL uniform-block layout:
//   u32  direction → 4 bytes (align 4)
//   u32  _pad[3]   → 12 bytes padding (total block = 16 bytes, align 16 ✓)
//
// Rust side: `GammaUniform { direction: u32, _pad: [u32; 3] }` = 16 bytes.
struct GammaUniforms {
    /// Transfer direction: 0 = SrgbToLinear, 1 = LinearToSrgb.
    direction: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

@group(0) @binding(0)
var<uniform> u: GammaUniforms;

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
    let xs = array<f32,6>(-1.0,  1.0, -1.0,  1.0,  1.0, -1.0);
    let ys = array<f32,6>(-1.0, -1.0,  1.0, -1.0,  1.0,  1.0);
    let us = array<f32,6>( 0.0,  1.0,  0.0,  1.0,  1.0,  0.0);
    let vs = array<f32,6>( 1.0,  1.0,  0.0,  1.0,  0.0,  0.0);

    var out: VertexOutput;
    out.position = vec4<f32>(xs[vi], ys[vi], 0.0, 1.0);
    out.uv       = vec2<f32>(us[vi], vs[vi]);
    return out;
}

// ─── Transfer functions ───────────────────────────────────────────────────────
//
// Mirrors `flui_types::styling::color::srgb_to_linear` and `linear_to_srgb`
// exactly (IEC 61966-2-1 piecewise).  One WGSL impl per direction, no branch
// table: the `u.direction` select is done once in `fs_main` via `select(...)`.

/// IEC 61966-2-1 sRGB → linear electro-optical transfer (gamma-decode).
fn srgb_to_linear(c: f32) -> f32 {
    return select(
        pow((c + 0.055) / 1.055, 2.4),
        c / 12.92,
        c <= 0.04045
    );
}

/// IEC 61966-2-1 linear → sRGB opto-electronic transfer (gamma-encode).
fn linear_to_srgb(c: f32) -> f32 {
    return select(
        1.055 * pow(c, 1.0 / 2.4) - 0.055,
        12.92 * c,
        c <= 0.003130800000
    );
}

// ─── Fragment stage ───────────────────────────────────────────────────────────

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Step 1 — sample the premultiplied source texel.
    let t: vec4<f32> = textureSample(src_texture, src_sampler, in.uv);

    // Step 2 — un-premultiply (divide-by-zero guard: if α=0 → straight RGB = 0).
    let straight_rgb: vec3<f32> = select(vec3<f32>(0.0), t.rgb / t.a, t.a > 0.0);

    // Step 3 — apply the transfer function per channel; alpha passes through.
    // `var` (not `let`): WGSL `let` requires an initializer; the value is assigned
    // in both branches below (definitely-assigned before the read at step 4).
    var transferred_rgb: vec3<f32>;
    if u.direction == 0u {
        // SrgbToLinear
        transferred_rgb = vec3<f32>(
            srgb_to_linear(straight_rgb.x),
            srgb_to_linear(straight_rgb.y),
            srgb_to_linear(straight_rgb.z),
        );
    } else {
        // LinearToSrgb
        transferred_rgb = vec3<f32>(
            linear_to_srgb(straight_rgb.x),
            linear_to_srgb(straight_rgb.y),
            linear_to_srgb(straight_rgb.z),
        );
    }

    // Step 4 — clamp RGB to [0, 1].  Alpha carries through from `t.a`.
    let clamped_rgb: vec3<f32> = clamp(transferred_rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    let out_a: f32 = t.a;

    // Step 5 — re-premultiply: rgb *= a.
    let out_rgb: vec3<f32> = clamped_rgb * out_a;

    // Step 6 — emit premultiplied RGBA (BlendState::REPLACE, no HW blending).
    return vec4<f32>(out_rgb, out_a);
}
