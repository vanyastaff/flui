
// Fragment entry point: coverage on its own channel, as a second blend source.
//
// Requires `wgpu::Features::DUAL_SOURCE_BLENDING`, and the `enable
// dual_source_blending;` directive that each assembly prepends. Used only for
// the blend modes whose destination factor does NOT absorb `1 - coverage` —
// see `pipeline::coverage_blend_state_for`, which pairs it with
// `dst_factor = OneMinusSrc1`.
//
// The module this is appended to supplies `VertexOutput` and `shadeFragment`.

/// The `k` in this pipeline's blend mode destination factor `D = k * srcAlpha`.
///
/// `0.0` for the modes whose destination factor is `Zero` (`Clear`, `Src`,
/// `SrcIn`, `SrcOut`, `Modulate`); `1.0` for those whose factor is `SrcAlpha`
/// (`DstIn`, `DstATop`). Supplied per pipeline by
/// `pipeline::destination_alpha_scale_for` — it is uniform across a pipeline
/// by construction (one pipeline per blend mode), so it is an overridable
/// constant rather than instance data that could disagree with the blend
/// state it must match.
override destination_alpha_scale: f32 = 0.0;

struct BlendSources {
    /// The premultiplied colour, identical to what the folded entry point emits.
    @location(0) @blend_src(0) color: vec4<f32>,
    /// The fraction of the destination this fragment replaces. The pipeline
    /// reads it as `OneMinusSrc1`, so `1 - this` is what survives.
    @location(0) @blend_src(1) destination_replaced: vec4<f32>,
}

@fragment
fn fs_main(input: VertexOutput) -> BlendSources {
    let shaded = shadeFragment(input);

    // The coverage-correct result is `mix(dst, blend(src, dst), coverage)`,
    // whose destination term is `(1 - coverage + coverage*D) * dst`, i.e.
    // `(1 - coverage*(1 - D)) * dst`. With `dst_factor = OneMinusSrc1` the
    // blender computes `(1 - src1) * dst`, so `src1 = coverage * (1 - D)`.
    // Substituting `D = destination_alpha_scale * alpha` gives the line below,
    // and it needs no branch: the scale is 0 or 1.
    let destination_replaced =
        shaded.coverage * (1.0 - destination_alpha_scale * shaded.color.a);

    var sources: BlendSources;
    sources.color = premultipliedSource(shaded);
    sources.destination_replaced = vec4<f32>(destination_replaced);
    return sources;
}
