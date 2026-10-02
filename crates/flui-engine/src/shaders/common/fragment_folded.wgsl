
// Fragment entry point: coverage folded into the source alpha.
//
// One output channel carries both quantities, so a blend mode's destination
// factor sees `alpha x coverage` where it should see `alpha`. That is exact
// only when the factor absorbs the difference (`One`, or `1 - k*srcAlpha`);
// `pipeline::destination_alpha_scale_for` returns `None` for exactly those
// modes, and this entry point serves them.
//
// Destination-sensitive fractional coverage uses independent isolation planes
// when a second blend source is unavailable. Binary coverage can use this entry
// with exact fixed-function factors. Fractional saturating Plus uses isolation
// on either device because clamping and geometric coverage do not commute.
//
// The module this is appended to supplies `VertexOutput` and `shadeFragment`.
@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return premultipliedSource(shadeFragment(input));
}
