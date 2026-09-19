
// Fragment entry point: coverage folded into the source alpha.
//
// One output channel carries both quantities, so a blend mode's destination
// factor sees `alpha x coverage` where it should see `alpha`. That is exact
// only when the factor absorbs the difference (`One`, or `1 - k*srcAlpha`);
// `pipeline::destination_alpha_scale_for` returns `None` for exactly those
// modes, and this entry point serves them.
//
// It also serves EVERY mode on a device without
// `wgpu::Features::DUAL_SOURCE_BLENDING`, where the second channel does not
// exist. There the seven coverage-destructive modes keep a hard edge — a
// documented backend divergence, not a contract. See ADR-0057.
//
// The module this is appended to supplies `VertexOutput` and `shadeFragment`.
@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return premultipliedSource(shadeFragment(input));
}
