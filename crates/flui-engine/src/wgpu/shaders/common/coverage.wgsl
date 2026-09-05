// Coverage and paint alpha, and the one place that knows they are different.
//
// This file is not a standalone shader: it is prepended to every shader whose
// fragment stage ends in one of `common/fragment_folded.wgsl` or
// `common/fragment_second_source.wgsl` (see `shaders/mod.rs` and
// `effects_pipeline`), because WGSL has no include directive and module-scope
// declarations are order-independent.
//
// `clipAlpha` reports how much of the pixel the clip admits, and an instanced
// primitive's own SDF reports how much of the pixel its geometry covers;
// `color.a` is how opaque the paint is. Handing the blender their product as
// one number is exact only for a blend mode whose destination factor absorbs
// `1 - coverage` — see `pipeline::destination_alpha_scale_for` for which modes
// those are and what the others need instead.
//
// Each consuming module supplies its own `VertexOutput` and its own
// `shadeFragment`, and the two entry points are written once here, so a
// tessellated shape and a gradient cannot disagree about either quantity.

/// Paint colour and total coverage for one fragment.
struct ShadedFragment {
    /// Straight (NOT premultiplied) paint colour; `.a` is the paint's own alpha.
    color: vec4<f32>,
    /// The fraction of this pixel the fragment covers, in [0, 1] — the clip's
    /// coverage times the primitive's own, where it has one.
    coverage: f32,
}

/// The first blend source: PREMULTIPLIED colour, scaled by coverage.
///
/// Premultiplied is the correct input form for fixed-function Porter-Duff
/// blending, which every consumer of this file selects per blend mode. The
/// default SrcOver case pairs this with
/// `BlendState::PREMULTIPLIED_ALPHA_BLENDING` (src factor One).
///
/// Scaling the whole premultiplied source by coverage is exactly
/// `coverage x (the full-coverage source term)` for every mode in
/// `blend_state_for`, because no mode's SOURCE factor reads source alpha —
/// they read `0`, `1`, `dstAlpha`, `1 - dstAlpha`, or `dst`. That is why only
/// the DESTINATION half of the blend needs correcting for partial coverage,
/// and why both entry points can emit the same first source.
fn premultipliedSource(shaded: ShadedFragment) -> vec4<f32> {
    let a = shaded.color.a * shaded.coverage;
    return vec4<f32>(shaded.color.rgb * a, a);
}
