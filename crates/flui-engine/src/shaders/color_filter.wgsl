#define_import_path color_filter
#import blend_helpers::{hard_light, lum, set_lum, sat, set_sat}

struct ImageSourceFilter {
    matrix: mat4x4<f32>,
    offset: vec4<f32>,
    color: vec4<f32>,
    kind: u32,
    mode: u32,
    padding: vec2<u32>,
}

fn matrix_channel(straight: vec4<f32>, weights: vec4<f32>, offset: f32) -> f32 {
    // WGSL permits indeterminate results after overflow, even with a later
    // clamp. Bound the dot product and guard rescaling instead. Inputs and
    // coefficients are finite; straight channels are normalized. Cancellation
    // still has f32 precision and the device's subnormal behavior.
    let magnitudes = abs(weights);
    let scale = max(1.0, max(abs(offset), max(max(magnitudes.x, magnitudes.y), max(magnitudes.z, magnitudes.w))));
    let weighted = dot(weights / scale, straight);
    if weighted == 0.0 { return clamp(offset, 0.0, 1.0); }
    let sum = weighted + offset / scale;
    if sum <= 0.0 { return 0.0; }
    if sum >= 1.0 / scale { return 1.0; }
    return sum * scale;
}

fn filter_matrix(straight: vec4<f32>, m: mat4x4<f32>, offset: vec4<f32>) -> vec4<f32> {
    let rows = transpose(m);
    let value = vec4<f32>(matrix_channel(straight, rows[0], offset.x),
                          matrix_channel(straight, rows[1], offset.y),
                          matrix_channel(straight, rows[2], offset.z),
                          matrix_channel(straight, rows[3], offset.w));
    return vec4<f32>(value.rgb * value.a, value.a);
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { return c / 12.92; }
    return pow((c + 0.055) / 1.055, 2.4);
}

fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 { return 12.92 * c; }
    return 1.055 * pow(c, 1.0 / 2.4) - 0.055;
}

fn filter_gamma(straight: vec4<f32>, direction: u32) -> vec4<f32> {
    var rgb: vec3<f32>;
    if direction == 0u { rgb = vec3<f32>(srgb_to_linear(straight.r), srgb_to_linear(straight.g), srgb_to_linear(straight.b)); }
    else { rgb = vec3<f32>(linear_to_srgb(straight.r), linear_to_srgb(straight.g), linear_to_srgb(straight.b)); }
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)) * straight.a, straight.a);
}

// Both layer effects and decoded image taps use this premultiplied operator.
fn separable(mode: u32, cb: f32, cs: f32) -> f32 {
    if mode == 15u { return cb + cs - cb * cs; }
    if mode == 16u { return hard_light(cs, cb); }
    if mode == 17u { return min(cb, cs); }
    if mode == 18u { return max(cb, cs); }
    if mode == 19u {
        if cb <= 0.0 { return 0.0; }
        if cs >= 1.0 { return 1.0; }
        return min(cb / (1.0 - cs), 1.0);
    }
    if mode == 20u {
        if cb >= 1.0 { return 1.0; }
        if cs <= 0.0 { return 0.0; }
        return 1.0 - min((1.0 - cb) / cs, 1.0);
    }
    if mode == 21u { return hard_light(cb, cs); }
    if mode == 22u {
        if cs <= 0.5 { return cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb); }
        var d = sqrt(cb);
        if cb <= 0.25 { d = ((16.0 * cb - 12.0) * cb + 4.0) * cb; }
        return cb + (2.0 * cs - 1.0) * (d - cb);
    }
    if mode == 23u { return abs(cb - cs); }
    if mode == 24u { return cb + cs - 2.0 * cb * cs; }
    return cb * cs; // Multiply, mode 25.
}

fn factors(mode: u32, sa: f32, da: f32) -> vec2<f32> {
    if mode == 0u { return vec2<f32>(0.0); }
    if mode == 1u { return vec2<f32>(1.0, 0.0); }
    if mode == 2u { return vec2<f32>(0.0, 1.0); }
    if mode == 3u { return vec2<f32>(1.0, 1.0 - sa); }
    if mode == 4u { return vec2<f32>(1.0 - da, 1.0); }
    if mode == 5u { return vec2<f32>(da, 0.0); }
    if mode == 6u { return vec2<f32>(0.0, sa); }
    if mode == 7u { return vec2<f32>(1.0 - da, 0.0); }
    if mode == 8u { return vec2<f32>(0.0, 1.0 - sa); }
    if mode == 9u { return vec2<f32>(da, 1.0 - sa); }
    if mode == 10u { return vec2<f32>(1.0 - da, sa); }
    if mode == 11u { return vec2<f32>(1.0 - da, 1.0 - sa); }
    return vec2<f32>(1.0); // Plus, mode 12.
}

fn filter_mode(src: vec4<f32>, dst: vec4<f32>, mode: u32) -> vec4<f32> {
    let src_pm = src.rgb * src.a;
    if mode == 13u { return vec4<f32>(src_pm * dst.rgb, src.a * dst.a); }
    if mode <= 12u {
        let f = factors(mode, src.a, dst.a);
        return clamp(vec4<f32>(src_pm, src.a) * f.x + dst * f.y, vec4<f32>(0.0), vec4<f32>(1.0));
    }
    var cb = vec3<f32>(0.0);
    if dst.a > 0.0 { cb = dst.rgb / dst.a; }
    var blended: vec3<f32>;
    if mode == 26u { blended = set_lum(set_sat(src.rgb, sat(cb)), lum(cb)); }
    else if mode == 27u { blended = set_lum(set_sat(cb, sat(src.rgb)), lum(cb)); }
    else if mode == 28u { blended = set_lum(src.rgb, lum(cb)); }
    else if mode == 29u { blended = set_lum(cb, lum(src.rgb)); }
    else { blended = vec3<f32>(separable(mode, cb.r, src.r), separable(mode, cb.g, src.g), separable(mode, cb.b, src.b)); }
    return clamp(vec4<f32>(src.a * (1.0 - dst.a) * src.rgb + src.a * dst.a * blended + (1.0 - src.a) * dst.rgb,
                          src.a + dst.a * (1.0 - src.a)), vec4<f32>(0.0), vec4<f32>(1.0));
}
