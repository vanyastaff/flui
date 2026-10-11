// Per-pixel ColorFilter::Mode uses shared color_filter arithmetic.
// Input is a premultiplied layer texel; the filter color is straight sRGB.
// Blend IDs are encoded by mode::blend_mode_to_u32 (id 14 is unused).

// ─── Uniforms ─────────────────────────────────────────────────────────────────

// WGSL uniform-block layout:
//   vec4<f32>  color      → 16 bytes (align 16)
//   u32        blend_mode →  4 bytes
//   u32        _pad[3]    → 12 bytes padding
// Total: 32 bytes (multiple of 16 ✓).
//
// Rust side: the generated `ModeUniforms { color: [f32;4], blend_mode: u32,
// _pad0/_pad1/_pad2: u32 }` (wgsl_bindgen, see mode/generated.rs) = 32 bytes.
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

#import color_filter::filter_mode

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return filter_mode(u.color, textureSample(src_texture, src_sampler, in.uv), u.blend_mode);
}
