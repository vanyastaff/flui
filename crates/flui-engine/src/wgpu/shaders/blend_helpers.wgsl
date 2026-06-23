// blend_helpers.wgsl
//
// Shared W3C Compositing and Blending Level 1 leaf helpers used by both
// `effects/mode.wgsl` and `advanced_blend.wgsl`.
//
// ## Single source of truth
//
// Both shaders concatenate this snippet at the top of their module (via
// `concat!(include_str!("blend_helpers.wgsl"), "\n\n", include_str!("..."))`)
// so naga sees one unified WGSL module.  Any duplicate fn definition would
// cause a "redefined" error at `create_shader_module` — the concatenation
// enforces that this file is the sole definition site.
//
// ## Correctness contract
//
// These helpers are a verbatim port of the corresponding private functions in
// `flui-types/src/styling/color.rs` (lines 1112-1184).  Any divergence from
// that CPU oracle is a correctness bug.  The epsilon `1.1920929e-7` is the
// exact decimal representation of `f32::EPSILON` (2^-23 ≈ 1.1920929e-7) —
// the same value the oracle uses in `clip_color`.
//
// ## What is NOT in this file
//
// Per-shader dispatch functions (`separable_blend` / `nonseparable_blend` and
// any mode-index switch) are NOT shared: `mode.wgsl` encodes modes 15-29 and
// `advanced_blend.wgsl` encodes modes 0-14.  Only the leaf helpers below are
// identical between the two shaders and belong here.

// ── Leaf helpers (verbatim port of color.rs) ─────────────────────────────────

/// W3C `HardLight(cb, cs)`: multiply for a dark source, screen for a light one.
/// Also the kernel of Overlay with arguments swapped.
/// Mirrors `hard_light` in `flui-types/src/styling/color.rs:1114`.
fn hard_light(cb: f32, cs: f32) -> f32 {
    if cs <= 0.5 {
        return 2.0 * cb * cs;
    }
    return 1.0 - 2.0 * (1.0 - cb) * (1.0 - cs);
}

/// Luminosity of an RGB triple (W3C `Lum`).
/// Mirrors `lum` in `flui-types/src/styling/color.rs:1136`.
fn lum(c: vec3<f32>) -> f32 {
    return 0.3 * c.r + 0.59 * c.g + 0.11 * c.b;
}

/// Clip an RGB triple back into `[0, 1]` while preserving luminosity (W3C `ClipColor`).
/// The epsilon `1.1920929e-7` = `f32::EPSILON` guards avoid 0/0 when all channels
/// are equal (degenerate flat triple has nothing to scale).
/// Mirrors `clip_color` in `flui-types/src/styling/color.rs:1143`.
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

/// Shift an RGB triple to the target luminosity `l` (W3C `SetLum`).
/// Mirrors `set_lum` in `flui-types/src/styling/color.rs:1162`.
fn set_lum(c: vec3<f32>, target_lum: f32) -> vec3<f32> {
    let d = target_lum - lum(c);
    return clip_color(c + d);
}

/// Saturation of an RGB triple (W3C `Sat`): max channel minus min channel.
/// Mirrors `sat` in `flui-types/src/styling/color.rs:1168`.
fn sat(c: vec3<f32>) -> f32 {
    return max(max(c.r, c.g), c.b) - min(min(c.r, c.g), c.b);
}

/// Rescale an RGB triple to the target saturation `s` (W3C `SetSat`), keeping
/// the relative channel ordering.  A flat triple (max == min) collapses to black.
///
/// Implementation: bubble-sort the 3 channels to find min/mid/max values, then
/// map each original channel back to its sorted slot using value equality.
/// Algorithmically equivalent to the `idx.sort_by` approach in
/// `flui-types/src/styling/color.rs:1174`.
///
/// WGSL has no sort primitive; this uses 3 swap comparisons (O(3), stable).
fn set_sat(c: vec3<f32>, target_sat: f32) -> vec3<f32> {
    // Extract channels into indexed array for permutation.
    var ch = array<f32, 3>(c.r, c.g, c.b);

    // Bubble-sort 3 elements ascending: O(3 comparisons), stable.
    var tmp: f32;
    if ch[0] > ch[1] { tmp = ch[0]; ch[0] = ch[1]; ch[1] = tmp; }
    if ch[1] > ch[2] { tmp = ch[1]; ch[1] = ch[2]; ch[2] = tmp; }
    if ch[0] > ch[1] { tmp = ch[0]; ch[0] = ch[1]; ch[1] = tmp; }
    // ch[0]=min, ch[1]=mid, ch[2]=max — values only; original index unknown here.
    let c_min = ch[0];
    let c_mid = ch[1];
    let c_max = ch[2];

    var out_ch = array<f32, 3>(0.0, 0.0, 0.0);
    if c_max > c_min {
        // Map each original channel back to its sorted role by value comparison.
        for (var slot = 0u; slot < 3u; slot++) {
            let cv = array<f32, 3>(c.r, c.g, c.b)[slot];
            if cv == c_min {
                out_ch[slot] = 0.0;
            } else if cv == c_max {
                out_ch[slot] = target_sat;
            } else {
                out_ch[slot] = (cv - c_min) * target_sat / (c_max - c_min);
            }
        }
    }
    // If c_max == c_min: all channels equal → out_ch stays 0 (achromatic → black).
    return vec3<f32>(out_ch[0], out_ch[1], out_ch[2]);
}
