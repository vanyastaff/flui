// mode.wgsl
//
// Per-pixel ColorFilter::Mode: blends a solid filter color (SRC) over each
// layer pixel (DST) using one of the 28 Porter-Duff / W3C blend modes.
//
// ## Correctness contract
//
// The layer texture is premultiplied RGBA.  This shader:
//
//   1. Samples the source texel `t` (premultiplied RGBA = DST).
//   2. Un-premultiplies: dst_straight = t.rgb / t.a  (guard: if t.a == 0 → vec3(0)).
//   3. Retrieves the filter color `u.color` as straight sRGB RGBA (SRC).
//   4. Computes blend(src=u.color, dst=dst_straight, mode=u.blend_mode) in straight
//      sRGB space — the W3C composite formula outputs a premultiplied result for
//      advanced modes; Modulate and Porter-Duff compute premultiplied output directly.
//   5. Clamps to [0, 1] and emits the premultiplied result with REPLACE blend.
//
// This matches `flui_types::Color::blend(self=filter_color, dst=pixel_color, mode)`
// (the CPU oracle), where `self` (SRC) is the filter color and `dst` is the layer pixel.
//
// ## Blend mode encoding
//
// `u.blend_mode` must match `blend_mode_to_u32` in `mode/pipeline.rs` exactly —
// matching `blend_mode_to_u32` in `mode/pipeline.rs` (keep the two in sync).
// NOTE: this is NOT exactly enum-declaration order — id 14 is INTENTIONALLY
// UNUSED (a deliberate gap so the separable-advanced range starts cleanly at 15).
// No `BlendMode` maps to 14; the `else` dispatch branch below covers 14-25 but
// only 15-25 are ever produced.
//
//   0  = Clear          7  = SrcOut        (14 = unused)      21 = HardLight
//   1  = Src            8  = DstOut         15 = Screen       22 = SoftLight
//   2  = Dst            9  = SrcATop        16 = Overlay      23 = Difference
//   3  = SrcOver        10 = DstATop        17 = Darken       24 = Exclusion
//   4  = DstOver        11 = Xor            18 = Lighten      25 = Multiply
//   5  = SrcIn          12 = Plus           19 = ColorDodge   26 = Hue
//   6  = DstIn          13 = Modulate       20 = ColorBurn    27 = Saturation
//                                                             28 = Color
//                                                             29 = Luminosity
//
// ## Bind-group layout (group 0)
//
// | Binding | Stage | Type                  | Content                      |
// |---------|-------|-----------------------|------------------------------|
// | 0       | FS    | Uniform buffer        | `ModeUniforms`               |
// | 1       | FS    | 2D float texture      | Source layer (premultiplied) |
// | 2       | FS    | Non-filtering sampler | Nearest + ClampToEdge        |
//
// ## Vertex stage
//
// No vertex buffer.  The VS synthesises 6 vertices from @builtin(vertex_index)
// covering the full viewport ([0,0]→[1,1] NDC quad, two triangles).

// ─── Uniforms ─────────────────────────────────────────────────────────────────

// WGSL uniform-block layout:
//   vec4<f32>  color      → 16 bytes (align 16)
//   u32        blend_mode →  4 bytes
//   u32        _pad[3]    → 12 bytes padding
// Total: 32 bytes (multiple of 16 ✓).
//
// Rust side: `ModeUniform { color: [f32;4], blend_mode: u32, _pad: [u32;3] }` = 32 bytes.
struct ModeUniforms {
    /// Filter color in straight sRGB [r, g, b, a] (SRC for the blend).
    color: vec4<f32>,
    /// Blend mode index — must match `blend_mode_to_u32` in Rust.
    blend_mode: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

@group(0) @binding(0)
var<uniform> u: ModeUniforms;

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

// ─── Blend helpers ─────────────────────────────────────────────────────────────
//
// The 6 shared leaf helpers (hard_light, lum, clip_color, set_lum, sat,
// set_sat) are defined in `blend_helpers.wgsl` and resolved by naga_oil at
// pipeline-init time via `compose_wgsl_shader` in `src/wgpu/mod.rs`.
//
// All helpers mirror the corresponding private functions in
// `flui_types::styling::color`.  SRC = filter color, DST = layer pixel.
// Both operate on straight sRGB [0,1] values.

#import blend_helpers::{hard_light, lum, clip_color, set_lum, sat, set_sat}

/// W3C separable blend function B(cb, cs), one channel.
/// cb = backdrop (DST straight), cs = source (SRC straight).
/// Matches `separable_blend` in `flui_types::styling::color`.
fn separable_blend(mode: u32, cb: f32, cs: f32) -> f32 {
    // Screen: cb + cs - cb*cs
    if mode == 15u { return cb + cs - cb * cs; }
    // Overlay: hard_light(cs, cb) — arguments swapped vs HardLight
    if mode == 16u { return hard_light(cs, cb); }
    // Darken
    if mode == 17u { return min(cb, cs); }
    // Lighten
    if mode == 18u { return max(cb, cs); }
    // ColorDodge
    if mode == 19u {
        if cb <= 0.0 { return 0.0; }
        if cs >= 1.0 { return 1.0; }
        return min(cb / (1.0 - cs), 1.0);
    }
    // ColorBurn
    if mode == 20u {
        if cb >= 1.0 { return 1.0; }
        if cs <= 0.0 { return 0.0; }
        return 1.0 - min((1.0 - cb) / cs, 1.0);
    }
    // HardLight
    if mode == 21u { return hard_light(cb, cs); }
    // SoftLight
    if mode == 22u {
        if cs <= 0.5 {
            return cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb);
        } else {
            var d: f32;
            if cb <= 0.25 {
                d = ((16.0 * cb - 12.0) * cb + 4.0) * cb;
            } else {
                d = sqrt(cb);
            }
            return cb + (2.0 * cs - 1.0) * (d - cb);
        }
    }
    // Difference
    if mode == 23u { return abs(cb - cs); }
    // Exclusion
    if mode == 24u { return cb + cs - 2.0 * cb * cs; }
    // Multiply
    if mode == 25u { return cb * cs; }
    // Fallback (should not be reached for advanced modes)
    return cs;
}

/// W3C non-separable blend (Hue/Saturation/Color/Luminosity).
/// cb = backdrop (DST straight), cs = source (SRC straight).
/// mode: 26=Hue, 27=Saturation, 28=Color, 29=Luminosity.
fn nonseparable_blend(mode: u32, cb: vec3<f32>, cs: vec3<f32>) -> vec3<f32> {
    if mode == 26u { return set_lum(set_sat(cs, sat(cb)), lum(cb)); } // Hue
    if mode == 27u { return set_lum(set_sat(cb, sat(cs)), lum(cb)); } // Saturation
    if mode == 28u { return set_lum(cs, lum(cb)); }                   // Color
    // Luminosity (mode == 29u)
    return set_lum(cb, lum(cs));
}

// ─── Porter-Duff factor lookup ─────────────────────────────────────────────────
//
// Returns (Fa, Fb) for Porter-Duff modes.  The blended premultiplied channel is
// `Fa * src_pm + Fb * dst_pm` (and same for alpha).
// Mirrors `porter_duff_factors` in `flui_types::styling::color`.
// Returns (0,0) for non-Porter-Duff — callers must check `mode` first.
fn porter_duff_factors(mode: u32, sa: f32, da: f32) -> vec2<f32> {
    if mode ==  0u { return vec2<f32>(0.0, 0.0); }              // Clear
    if mode ==  1u { return vec2<f32>(1.0, 0.0); }              // Src
    if mode ==  2u { return vec2<f32>(0.0, 1.0); }              // Dst
    if mode ==  3u { return vec2<f32>(1.0, 1.0 - sa); }         // SrcOver
    if mode ==  4u { return vec2<f32>(1.0 - da, 1.0); }         // DstOver
    if mode ==  5u { return vec2<f32>(da, 0.0); }               // SrcIn
    if mode ==  6u { return vec2<f32>(0.0, sa); }               // DstIn
    if mode ==  7u { return vec2<f32>(1.0 - da, 0.0); }         // SrcOut
    if mode ==  8u { return vec2<f32>(0.0, 1.0 - sa); }         // DstOut
    if mode ==  9u { return vec2<f32>(da, 1.0 - sa); }          // SrcATop
    if mode == 10u { return vec2<f32>(1.0 - da, sa); }          // DstATop
    if mode == 11u { return vec2<f32>(1.0 - da, 1.0 - sa); }    // Xor
    if mode == 12u { return vec2<f32>(1.0, 1.0); }              // Plus
    return vec2<f32>(0.0, 0.0); // not a Porter-Duff mode
}

// ─── Fragment stage ───────────────────────────────────────────────────────────

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Step 1 — sample the premultiplied layer texel (DST).
    let t: vec4<f32> = textureSample(src_texture, src_sampler, in.uv);

    // Step 2 — un-premultiply DST (divide-by-zero guard).
    let dst_straight_rgb: vec3<f32> = select(vec3<f32>(0.0), t.rgb / t.a, t.a > 0.0);
    let dst_a: f32 = t.a;

    // SRC = filter color (already straight sRGB from uniform).
    let src_straight_rgb: vec3<f32> = u.color.rgb;
    let src_a: f32 = u.color.a;

    // Premultiplied SRC and DST channels for Modulate / Porter-Duff branches.
    let src_pm = src_straight_rgb * src_a;
    let dst_pm = dst_straight_rgb * dst_a;

    // Step 3+4 — compute blend.  Output is premultiplied for Modulate and Porter-Duff;
    // for advanced modes the W3C composite formula yields a premultiplied result directly.
    var out_pm: vec3<f32>;
    var out_a: f32;

    let mode = u.blend_mode;

    if mode == 13u {
        // Modulate: component-wise product of premultiplied colors.
        out_pm = src_pm * dst_pm;
        out_a  = src_a * dst_a;
    } else if mode <= 12u {
        // Porter-Duff (modes 0..12).
        let f = porter_duff_factors(mode, src_a, dst_a);
        out_pm = clamp(src_pm * f.x + dst_pm * f.y, vec3<f32>(0.0), vec3<f32>(1.0));
        out_a  = clamp(src_a  * f.x + dst_a  * f.y, 0.0, 1.0);
    } else if mode >= 26u {
        // Non-separable HSL modes (26-29): Hue/Saturation/Color/Luminosity.
        // W3C composite: co = αs*(1-αb)*Cs + αs*αb*B(Cb,Cs) + (1-αs)*αb*Cb
        let blended = nonseparable_blend(mode, dst_straight_rgb, src_straight_rgb);
        out_pm.x = src_a * (1.0 - dst_a) * src_straight_rgb.x
                 + src_a * dst_a * blended.x
                 + (1.0 - src_a) * dst_a * dst_straight_rgb.x;
        out_pm.y = src_a * (1.0 - dst_a) * src_straight_rgb.y
                 + src_a * dst_a * blended.y
                 + (1.0 - src_a) * dst_a * dst_straight_rgb.y;
        out_pm.z = src_a * (1.0 - dst_a) * src_straight_rgb.z
                 + src_a * dst_a * blended.z
                 + (1.0 - src_a) * dst_a * dst_straight_rgb.z;
        out_a = src_a + dst_a * (1.0 - src_a);
    } else {
        // Separable advanced modes (15..25). id 14 is unused (see header); no
        // mode maps to it, so this branch only ever runs for 15..25 in practice.
        // W3C composite: same formula as non-separable but B is per-channel.
        let b_r = separable_blend(mode, dst_straight_rgb.x, src_straight_rgb.x);
        let b_g = separable_blend(mode, dst_straight_rgb.y, src_straight_rgb.y);
        let b_b = separable_blend(mode, dst_straight_rgb.z, src_straight_rgb.z);
        out_pm.x = src_a * (1.0 - dst_a) * src_straight_rgb.x
                 + src_a * dst_a * b_r
                 + (1.0 - src_a) * dst_a * dst_straight_rgb.x;
        out_pm.y = src_a * (1.0 - dst_a) * src_straight_rgb.y
                 + src_a * dst_a * b_g
                 + (1.0 - src_a) * dst_a * dst_straight_rgb.y;
        out_pm.z = src_a * (1.0 - dst_a) * src_straight_rgb.z
                 + src_a * dst_a * b_b
                 + (1.0 - src_a) * dst_a * dst_straight_rgb.z;
        out_a = src_a + dst_a * (1.0 - src_a);
    }

    // Step 5 — clamp and emit premultiplied RGBA (BlendState::REPLACE).
    let clamped_pm = clamp(out_pm, vec3<f32>(0.0), vec3<f32>(1.0));
    let clamped_a  = clamp(out_a, 0.0, 1.0);
    return vec4<f32>(clamped_pm, clamped_a);
}
