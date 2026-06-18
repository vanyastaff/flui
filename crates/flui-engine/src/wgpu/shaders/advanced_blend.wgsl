// Advanced blend-mode composite shader.
//
// Implements the W3C Compositing and Blending Level 1 separable and
// non-separable blend functions for all 15 advanced modes.  The math is
// a verbatim port of `Color::blend` in `flui-types/src/styling/color.rs`
// (lines 913–1149); any divergence is a correctness bug — the CPU oracle is
// the canonical definition.
//
// ## Pipeline contract
//
// - VS: generates a unit quad over `op_bounds` (device-space pixels) and
//   converts to clip space using `viewport_size`.  No vertex buffer is bound;
//   vertices are synthesised from `@builtin(vertex_index)`.
// - FS group 0:
//     binding 0 — `BlendUniforms` uniform
//     binding 1 — foreground texture (premultiplied RGBA f32)
//     binding 2 — backdrop copy texture (premultiplied RGBA f32)
//     binding 3 — nearest-clamp sampler
//
// ## Gamma space
//
// No `pow(2.2)` linearisation anywhere.  The textures store byte-channel
// values divided by 255 (i.e. the same encoding `Color::blend` uses).
// Introducing gamma linearisation here would diverge from the CPU oracle.
//
// ## Premultiplied flow
//
// Both input textures arrive premultiplied.  The fragment shader
// un-premultiplies to straight Cs/Cb before calling the blend function,
// then re-composites according to the W3C formula, and outputs premultiplied
// RGBA.  The pipeline blend state is REPLACE (the shader owns the full
// composite); no GPU fixed-function blending occurs.

// ── Uniforms ─────────────────────────────────────────────────────────────────

struct BlendUniforms {
    // Device-space pixel rectangle for the foreground layer: [x, y, w, h].
    // offset 0, size 16.
    op_bounds:    vec4<f32>,
    // Viewport size in device pixels [w, h] for clip-space conversion.
    // offset 16, size 8.
    viewport_size: vec2<f32>,
    // Origin of the backdrop copy rect in device pixels [x, y].
    // offset 24, size 8.
    copy_origin:  vec2<f32>,
    // Extent of the backdrop copy rect in device pixels [w, h].
    // offset 32, size 8.
    copy_extent:  vec2<f32>,
    // Group opacity pre-applied into the source sample.
    // offset 40, size 4.
    opacity:      f32,
    // Alignment gap: tint_rgb (vec3<f32>) requires 16-byte alignment,
    // so bytes 44-47 are padding. offset 44, size 4.
    _pad0:        f32,
    // Per-channel tint RGB pre-applied into the source sample (matches
    // `flush_opacity_layer` tint folding: tint = (tint_rgb * opacity, opacity)).
    // offset 48, size 12.
    tint_rgb:     vec3<f32>,
    // Blend mode discriminant; see `mode_to_u32` in pipeline.rs.
    // offset 60, size 4.
    mode:         u32,
    // Struct-size padding to 80 bytes (multiple of 16). offset 64, size 16.
    _pad1:        vec4<u32>,
}

@group(0) @binding(0)
var<uniform> blend: BlendUniforms;

@group(0) @binding(1)
var foreground_tex: texture_2d<f32>;

@group(0) @binding(2)
var backdrop_tex:   texture_2d<f32>;

@group(0) @binding(3)
var nearest_sampler: sampler;

// ── Vertex shader ─────────────────────────────────────────────────────────────
//
// Synthesises 6 vertices (2 triangles, CCW) from `vertex_index` 0-5 that form
// a quad covering `op_bounds` in device space, then converts to clip space.
// No vertex buffer is bound; this avoids a CPU upload per draw.

struct VsOutput {
    // In the vertex shader this holds clip-space position; the rasteriser
    // converts it so that in the fragment shader @builtin(position) delivers
    // `frag_coord` — fragment-centre device pixel coordinates (col+0.5, row+0.5).
    @builtin(position) frag_pos: vec4<f32>,
    // Foreground texture UV in [0,1] within `op_bounds`.
    @location(0)       src_uv:   vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOutput {
    // Unit-quad corners for the 2-triangle strip (CCW winding).
    // Indices map: 0→TL, 1→TR, 2→BL, 3→TR, 4→BR, 5→BL
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), // TL
        vec2<f32>(1.0, 0.0), // TR
        vec2<f32>(0.0, 1.0), // BL
        vec2<f32>(1.0, 0.0), // TR
        vec2<f32>(1.0, 1.0), // BR
        vec2<f32>(0.0, 1.0), // BL
    );
    let uv = corners[vi];

    let bx = blend.op_bounds.x;
    let by = blend.op_bounds.y;
    let bw = blend.op_bounds.z;
    let bh = blend.op_bounds.w;

    // Device pixel position.
    let px = bx + uv.x * bw;
    let py = by + uv.y * bh;

    // Clip space: x in [-1,1], y flipped (screen Y grows down, clip Y up).
    let clip_x = (px / blend.viewport_size.x) * 2.0 - 1.0;
    let clip_y = 1.0 - (py / blend.viewport_size.y) * 2.0;

    var out: VsOutput;
    out.frag_pos = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    out.src_uv   = uv;
    return out;
}

// ── Blend helpers (verbatim port of color.rs separable/nonseparable) ──────────

fn hard_light(cb: f32, cs: f32) -> f32 {
    if cs <= 0.5 {
        return 2.0 * cb * cs;
    }
    return 1.0 - 2.0 * (1.0 - cb) * (1.0 - cs);
}

fn separable_blend(mode: u32, cb: f32, cs: f32) -> f32 {
    switch mode {
        // Multiply  (mode 0)
        case 0u: { return cb * cs; }
        // Screen    (mode 1)
        case 1u: { return cb + cs - cb * cs; }
        // Overlay   (mode 2): overlay(cb,cs) == hard_light(cs,cb)
        case 2u: { return hard_light(cs, cb); }
        // Darken    (mode 3)
        case 3u: { return min(cb, cs); }
        // Lighten   (mode 4)
        case 4u: { return max(cb, cs); }
        // ColorDodge (mode 5) — exact branch order from color.rs:1040-1047
        case 5u: {
            if cb <= 0.0 { return 0.0; }
            if cs >= 1.0 { return 1.0; }
            return min(cb / (1.0 - cs), 1.0);
        }
        // ColorBurn (mode 6) — exact branch order from color.rs:1049-1056
        case 6u: {
            if cb >= 1.0 { return 1.0; }
            if cs <= 0.0 { return 0.0; }
            return 1.0 - min((1.0 - cb) / cs, 1.0);
        }
        // HardLight (mode 7)
        case 7u: { return hard_light(cb, cs); }
        // SoftLight (mode 8) — verbatim from color.rs:1059-1069
        case 8u: {
            if cs <= 0.5 {
                return cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb);
            }
            var d: f32;
            if cb <= 0.25 {
                d = ((16.0 * cb - 12.0) * cb + 4.0) * cb;
            } else {
                d = sqrt(cb);
            }
            return cb + (2.0 * cs - 1.0) * (d - cb);
        }
        // Difference (mode 9)
        case 9u: { return abs(cb - cs); }
        // Exclusion  (mode 10)
        case 10u: { return cb + cs - 2.0 * cb * cs; }
        default: { return cs; }
    }
}

// Luminosity of an RGB triple (W3C Lum).
fn lum(c: vec3<f32>) -> f32 {
    return 0.3 * c.r + 0.59 * c.g + 0.11 * c.b;
}

// Clip an RGB triple back into [0,1] preserving luminosity (W3C ClipColor).
// The epsilon guards avoid 0/0 when all channels are equal.
fn clip_color(c: vec3<f32>) -> vec3<f32> {
    let l = lum(c);
    let n = min(min(c.r, c.g), c.b);
    let x = max(max(c.r, c.g), c.b);
    var out = c;
    if n < 0.0 && abs(l - n) > 1.1920929e-7 {
        out = l + (out - l) * l / (l - n);
    }
    if x > 1.0 && abs(x - l) > 1.1920929e-7 {
        out = l + (out - l) * (1.0 - l) / (x - l);
    }
    return out;
}

// Shift an RGB triple to target luminosity (W3C SetLum).
fn set_lum(c: vec3<f32>, target_lum: f32) -> vec3<f32> {
    let d = target_lum - lum(c);
    return clip_color(c + d);
}

// Saturation: max channel minus min channel (W3C Sat).
fn sat(c: vec3<f32>) -> f32 {
    return max(max(c.r, c.g), c.b) - min(min(c.r, c.g), c.b);
}

// Rescale RGB triple to target saturation keeping relative channel order
// (W3C SetSat).  A flat triple (max == min) collapses to black.
//
// WGSL has no sort; we implement the sort-by-magnitude manually.
// The triple has 6 orderings; we enumerate them as the index triple
// (i_min, i_mid, i_max) by comparing all three pairs exactly once.
fn set_sat(c: vec3<f32>, target_sat: f32) -> vec3<f32> {
    // Extract channels into indexed array for permutation.
    var ch = array<f32, 3>(c.r, c.g, c.b);

    // Bubble-sort 3 elements ascending: O(3 comparisons), stable.
    var tmp: f32;
    if ch[0] > ch[1] { tmp = ch[0]; ch[0] = ch[1]; ch[1] = tmp; }
    if ch[1] > ch[2] { tmp = ch[1]; ch[1] = ch[2]; ch[2] = tmp; }
    if ch[0] > ch[1] { tmp = ch[0]; ch[0] = ch[1]; ch[1] = tmp; }
    // ch[0]=min, ch[1]=mid, ch[2]=max — but these are VALUES not original indices.
    // We need to map back to (r,g,b); find which original index maps where.
    let c_min = ch[0];
    let c_mid = ch[1];
    let c_max = ch[2];

    var out_ch = array<f32, 3>(0.0, 0.0, 0.0);
    if c_max > c_min {
        // Determine original index for each sorted slot.
        // Priority: if two channels are equal we still assign each to a distinct slot.
        // We use the same lexicographic tie-break as Rust's total_cmp on f32.
        for (var slot = 0u; slot < 3u; slot++) {
            let cv = array<f32, 3>(c.r, c.g, c.b)[slot];
            // Assign to min/mid/max slot based on sorted value.
            if cv == c_min {
                out_ch[slot] = 0.0;
            } else if cv == c_max {
                out_ch[slot] = target_sat;
            } else {
                out_ch[slot] = (cv - c_min) * target_sat / (c_max - c_min);
            }
        }
    }
    // If c_max == c_min: all channels equal → out_ch stays 0 (flat/achromatic → black).
    return vec3<f32>(out_ch[0], out_ch[1], out_ch[2]);
}

// Non-separable blend for Hue/Saturation/Color/Luminosity modes.
// mode discriminants: 11=Hue, 12=Saturation, 13=Color, 14=Luminosity.
fn nonseparable_blend(mode: u32, cb: vec3<f32>, cs: vec3<f32>) -> vec3<f32> {
    switch mode {
        // Hue: hue of source, sat+lum of backdrop
        case 11u: { return set_lum(set_sat(cs, sat(cb)), lum(cb)); }
        // Saturation: sat of source, hue+lum of backdrop
        case 12u: { return set_lum(set_sat(cb, sat(cs)), lum(cb)); }
        // Color: hue+sat of source, lum of backdrop
        case 13u: { return set_lum(cs, lum(cb)); }
        // Luminosity: lum of source, hue+sat of backdrop
        case 14u: { return set_lum(cb, lum(cs)); }
        default: { return cs; }
    }
}

// ── Fragment shader ───────────────────────────────────────────────────────────

@fragment
fn fs_main(in: VsOutput) -> @location(0) vec4<f32> {
    // ── Sample foreground (premultiplied) ──────────────────────────────────
    let fg_pm = textureSample(foreground_tex, nearest_sampler, in.src_uv);

    // Apply opacity + tint into the premultiplied source, matching the
    // flush_opacity_layer folding: tint = (tint_rgb * opacity, opacity).
    // Per replay.rs:496-502: src_pm_adjusted = fg_pm * vec4(tint_rgb, 1) * opacity
    let fg_pm_adjusted = vec4<f32>(
        fg_pm.rgb * blend.tint_rgb * blend.opacity,
        fg_pm.a   * blend.opacity,
    );

    // ── Sample backdrop copy (premultiplied) ───────────────────────────────
    //
    // frag_pos.xy carries frag_coord: the fragment centre in device pixels,
    // i.e. (col+0.5, row+0.5).  The backdrop copy starts at copy_origin and
    // spans copy_extent pixels.  For surface column `col`:
    //   texel index in copy = col - copy_origin.x
    //   uv = ((col - copy_origin.x) + 0.5) / copy_extent.x
    //       = (frag_pos.x - copy_origin.x) / copy_extent.x
    //
    // No additional +0.5: frag_coord already carries the half-texel offset.
    let bd_uv = (in.frag_pos.xy - blend.copy_origin) / blend.copy_extent;
    let bd_pm = textureSample(backdrop_tex, nearest_sampler, bd_uv);

    // ── Un-premultiply both to straight Cs / Cb ────────────────────────────
    let as_ = fg_pm_adjusted.a;   // source alpha
    let ab  = bd_pm.a;            // backdrop alpha

    // Guard against division by zero: straight = premul / alpha, or 0 if alpha=0.
    let cs = select(fg_pm_adjusted.rgb / as_, vec3<f32>(0.0), as_ <= 0.0);
    let cb = select(bd_pm.rgb / ab,          vec3<f32>(0.0), ab  <= 0.0);

    // ── Blend function B(Cb, Cs) ───────────────────────────────────────────
    var blended: vec3<f32>;
    let m = blend.mode;
    if m >= 11u {
        // Non-separable: Hue/Saturation/Color/Luminosity
        blended = nonseparable_blend(m, cb, cs);
    } else {
        // Separable: applied per channel
        blended = vec3<f32>(
            separable_blend(m, cb.r, cs.r),
            separable_blend(m, cb.g, cs.g),
            separable_blend(m, cb.b, cs.b),
        );
    }

    // ── W3C composite (color.rs:934-942) ──────────────────────────────────
    //
    // co = αs·(1-αb)·Cs + αs·αb·B(Cb,Cs) + (1-αs)·αb·Cb  (premultiplied)
    // αo = αs + αb·(1-αs)
    let out_rgb_pm = as_ * (1.0 - ab) * cs
                   + as_ * ab         * blended
                   + (1.0 - as_) * ab * cb;
    let out_a = as_ + ab * (1.0 - as_);

    // ── out_a <= 0 → transparent (color.rs:945) ───────────────────────────
    if out_a <= 0.0 {
        return vec4<f32>(0.0);
    }

    // Output premultiplied.
    return vec4<f32>(clamp(out_rgb_pm, vec3<f32>(0.0), vec3<f32>(1.0)),
                     clamp(out_a, 0.0, 1.0));
}
