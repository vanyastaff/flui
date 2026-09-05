//! Readback evidence that a gradient honours the blend mode its paint carries.
//!
//! ## Why this file exists
//!
//! Gradients are instanced, not tessellated, so they never reach
//! `add_tessellated_with_key` and the blend funnel that keys the shape path's
//! pipelines by mode. `dispatch_shader_rect` diverted only ADVANCED modes;
//! every ordinary Porter-Duff mode fell through to gradient pipelines whose
//! blend state was the constant `ALPHA_BLENDING`. The mode was accepted,
//! carried on the paint, and dropped — and nothing in the suite looked, which
//! is why it stayed invisible.
//!
//! Nothing here is gradient-specific arithmetic. The gradient is a CONSTANT
//! colour in every oracle that asserts a blended byte, so the value under test
//! is the blender's, not `interpolateGradient`'s — and `blend_oracle`'s CPU
//! model, production's own `blend_state_for` table, predicts it. What varies
//! is only which pipeline the instance is drawn with.
//! `each_gradient_kind_paints_through_its_own_pipeline` is the premise that
//! keeps the constant colour honest: it fails if a solid fill quietly stood in.
//!
//! ## The two halves
//!
//! **Full strength** ([`gradient_at_full_strength`]): every non-`SrcOver`
//! Porter-Duff mode, on all three gradient kinds, over a flat destination with
//! no clip. Each mode's expected pixel is asserted to differ from
//! `SrcOver`'s BEFORE it is compared, so a pass cannot come from the two
//! agreeing — which is exactly how the defect hid.
//!
//! **Partial coverage** ([`gradient_through_an_anti_aliased_clip`]): the seven
//! modes whose destination factor cannot absorb `1 − coverage` must feather
//! under an anti-aliased clip on a device with `DUAL_SOURCE_BLENDING`, and
//! fall back to the folded value without it — the same contract, the same
//! scene, and the same derived coverage as the tessellated path's suite, per
//! ADR-0057. Keying the pipelines without this half would make a gradient
//! honour `Clear` and then feather it wrong.
//!
//! **The common path** ([`srcover_gradients_are_unchanged_to_within_one_bit`])
//! bounds how far `SrcOver` moved, because it is the path every gradient in
//! the workspace actually takes and this change reaches its shader as well as
//! its pipeline. It moved by exactly one bit, for a reason that constant names
//! and measures.

use flui_layer::{LayerTree, SceneBuilder};
use flui_painting::{BlendMode, Canvas, Paint};
use flui_types::{
    Color, Offset, Rect,
    geometry::{Pixels, RRect, px},
    painting::{Clip, ClipOp, Shader, TileMode},
};

use super::{
    blend_oracle::{
        CLIP_HEIGHT, CLIP_LEFT, CLIP_RADIUS, CLIP_TOP, CLIP_WIDTH, EdgeSamples, FRINGE_COVERAGE,
        PORTER_DUFF_MODES, SIDE, as_bytes, assert_pixel, blend, coverage_correct, coverage_folded,
        premultiplied, sample_the_clip_edge,
    },
    effects_pipeline::GradientKind,
    headless::HeadlessRenderer,
};

/// The destination every oracle blends into.
///
/// Distinct in all three channels so a per-channel factor error cannot hide,
/// and TRANSLUCENT because an opaque one cannot tell `SrcATop` from `SrcOver`:
/// `SrcATop`'s source factor is `DstAlpha`, which is `1` against an opaque
/// destination, leaving it the same `(One, OneMinusSrcAlpha)` pair as
/// `SrcOver`. Every mode's prediction is its own at this alpha —
/// `a_linear_gradient_renders_every_porter_duff_mode` asserts exactly that
/// before it compares a pixel.
const DESTINATION: Color = Color::rgba(200, 120, 60, 160);

/// Lay [`DESTINATION`] down as the surface, replacing whatever the capture path
/// cleared to.
///
/// `Src`, not `SrcOver`: `HeadlessRenderer` clears to opaque white, so a
/// translucent destination painted over it would arrive at the blender opaque
/// and take [`DESTINATION`]'s whole reason for being translucent away. `Src`
/// is `(One, Zero)` — it writes the premultiplied source and nothing else — so
/// the surface really does carry alpha 160 when the gradient blends into it.
///
/// This makes the scene depend on `Src` at FULL coverage on the tessellated
/// path, which is self-checking rather than circular: every oracle here also
/// asserts the untouched destination outside the gradient or the clip, so a
/// `Src` that wrote the wrong pixel fails on that assertion first.
fn destination_paint() -> Paint {
    Paint::fill(DESTINATION)
        .with_anti_alias(false)
        .with_blend_mode(BlendMode::Src)
}

/// The colour every constant-colour gradient carries.
///
/// Translucent on purpose: the modes whose destination factor is `SrcAlpha`
/// (`DstIn`, `DstATop`) reduce to "leave the destination alone" at full
/// opacity, and would then pass against a broken blender.
const SOURCE: Color = Color::rgba(0, 220, 40, 128);

/// The colour of the PAINT under the shader, which nothing may ever sample.
///
/// A gradient paint carries both a base colour and a shader; only the shader's
/// stops reach the GPU. Making the base colour opaque yellow — nothing like
/// [`SOURCE`], nothing like [`DESTINATION`] — means a fall-through to the solid
/// fill path shows up as yellow rather than as a plausible near-miss.
const UNSHADED_BASE: Color = Color::rgb(255, 255, 0);

/// The centre of the surface: interior to every gradient this file draws, far
/// from any anti-aliased edge, so its coverage is exactly 1.
const CENTRE: (u32, u32) = (SIDE / 2, SIDE / 2);

/// Every kind, so a sweep over them cannot quietly skip one.
///
/// Production's own [`GradientKind`] rather than a copy: the three are separate
/// shader modules with separate fragment entry points and separate cache
/// entries, so a fix proven on one implies nothing about the others — and a
/// fourth kind added to the engine must fail to compile here until it is
/// covered.
const GRADIENT_KINDS: [GradientKind; 3] = [
    GradientKind::Linear,
    GradientKind::Radial,
    GradientKind::Sweep,
];

/// A gradient of `kind` between `from` and `to`, spanning the surface.
///
/// The geometry is chosen so that the sampled pixels lie strictly inside the
/// ramp rather than on a clamped end, and so the radial and sweep forms cover
/// the whole surface: a radius of `SIDE` from the centre reaches every corner,
/// and a full turn leaves no unswept wedge.
fn gradient(kind: GradientKind, from: Color, to: Color) -> Shader {
    let colors = vec![from, to];
    let stops = Some(vec![0.0, 1.0]);
    let centre = Offset::new(px(SIDE as f32 / 2.0), px(SIDE as f32 / 2.0));
    match kind {
        GradientKind::Linear => Shader::LinearGradient {
            from: Offset::new(px(0.0), px(0.0)),
            to: Offset::new(px(SIDE as f32), px(0.0)),
            colors,
            stops,
            tile_mode: TileMode::Clamp,
        },
        GradientKind::Radial => Shader::RadialGradient {
            center: centre,
            radius: SIDE as f32,
            colors,
            stops,
            tile_mode: TileMode::Clamp,
            focal: None,
            focal_radius: None,
        },
        GradientKind::Sweep => Shader::SweepGradient {
            center: centre,
            colors,
            stops,
            tile_mode: TileMode::Clamp,
            start_angle: 0.0,
            end_angle: std::f32::consts::TAU,
        },
    }
}

/// A gradient of `kind` whose every stop is `color`, so the fragment it
/// produces is `color` wherever it is sampled.
fn constant_gradient(kind: GradientKind, color: Color) -> Shader {
    gradient(kind, color, color)
}

/// The whole surface, in device pixels.
fn full_surface() -> Rect<Pixels> {
    Rect::from_xywh(px(0.0), px(0.0), px(SIDE as f32), px(SIDE as f32))
}

/// A `Fill` paint carrying `shader` and `mode` over [`UNSHADED_BASE`].
fn shader_paint(shader: Shader, mode: BlendMode) -> Paint {
    Paint::fill(UNSHADED_BASE)
        .with_anti_alias(false)
        .with_shader(shader)
        .with_blend_mode(mode)
}

/// One `Rgba8` pixel out of a [`SIDE`]×[`SIDE`] readback.
fn pixel_at(pixels: &[u8], (column, row): (u32, u32)) -> [u8; 4] {
    let index = ((row * SIDE + column) * 4) as usize;
    [
        pixels[index],
        pixels[index + 1],
        pixels[index + 2],
        pixels[index + 3],
    ]
}

/// Paints `DESTINATION` over the surface, then a full-surface gradient with
/// `mode`, and reads the whole frame back.
fn render_gradient_over_destination(
    renderer: &HeadlessRenderer,
    shader: Shader,
    mode: BlendMode,
) -> Vec<u8> {
    let mut tree = LayerTree::new();
    {
        let mut builder = SceneBuilder::new(&mut tree);
        let mut canvas = Canvas::new();
        canvas.draw_rect(full_surface(), &destination_paint());
        canvas.draw_rect(full_surface(), &shader_paint(shader, mode));
        builder.add_picture(canvas.finish());
        builder.build();
    }
    renderer
        .render_layer_tree(&tree, (SIDE, SIDE))
        .expect("the headless capture path must rasterize the scene")
}

/// The surface centre after a constant-[`SOURCE`] gradient of `kind` is blended
/// over `DESTINATION` with `mode`, at coverage 1.
fn gradient_at_full_strength(
    renderer: &HeadlessRenderer,
    kind: GradientKind,
    mode: BlendMode,
) -> [u8; 4] {
    let pixels = render_gradient_over_destination(renderer, constant_gradient(kind, SOURCE), mode);
    pixel_at(&pixels, CENTRE)
}

/// Paints `DESTINATION` over the surface, then a constant-[`SOURCE`] gradient
/// with `mode` through an anti-aliased rounded clip whose left edge falls
/// mid-column.
///
/// The clip is the only source of partial coverage the sampled row sees: the
/// gradient's own rounded-box SDF spans the whole surface, so at
/// `FRINGE_COLUMN` its distance is far inside and its own edge alpha is 1.
/// That makes the coverage at the sampled pixel exactly the clip's
/// [`FRINGE_COVERAGE`] — the same number, derived the same way, as the
/// tessellated path's suite.
fn gradient_through_an_anti_aliased_clip(
    renderer: &HeadlessRenderer,
    kind: GradientKind,
    mode: BlendMode,
) -> EdgeSamples {
    let mut tree = LayerTree::new();
    {
        let mut builder = SceneBuilder::new(&mut tree);
        let mut canvas = Canvas::new();
        canvas.draw_rect(full_surface(), &destination_paint());
        canvas.save();
        canvas.clip_rrect_ext(
            RRect::from_rect_circular(
                Rect::from_xywh(px(CLIP_LEFT), px(CLIP_TOP), px(CLIP_WIDTH), px(CLIP_HEIGHT)),
                px(CLIP_RADIUS),
            ),
            ClipOp::Intersect,
            Clip::AntiAlias,
        );
        canvas.draw_rect(
            full_surface(),
            &shader_paint(constant_gradient(kind, SOURCE), mode),
        );
        canvas.restore();
        builder.add_picture(canvas.finish());
        builder.build();
    }

    let pixels = renderer
        .render_layer_tree(&tree, (SIDE, SIDE))
        .expect("the headless capture path must rasterize the scene");
    sample_the_clip_edge(&pixels)
}

// ── Acceptance 1: every fixed-function mode reaches the gradient ─────────────

/// The premise every constant-colour oracle rests on: each kind's own pipeline
/// really renders the gradient's stops.
///
/// If `dispatch_shader_rect` stopped handling a kind, the draw would fall
/// through to the solid-fill path and paint [`UNSHADED_BASE`] instead —
/// opaque yellow, which no prediction in this file resembles. If a kind's
/// pipeline were never built, the pixel would be the bare destination.
#[test]
fn each_gradient_kind_paints_through_its_own_pipeline() {
    let Ok(renderer) = HeadlessRenderer::new() else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };

    let expected = blend(
        BlendMode::SrcOver,
        premultiplied(SOURCE, 1.0),
        premultiplied(DESTINATION, 1.0),
    );
    for kind in GRADIENT_KINDS {
        assert_pixel(
            gradient_at_full_strength(&renderer, kind, BlendMode::SrcOver),
            expected,
            &format!(
                "{kind:?}: the surface centre must carry the gradient's stop colour \
                 blended over the destination. Opaque yellow means the shader \
                 dispatch was skipped and a solid fill stood in; the bare \
                 destination means the gradient never drew"
            ),
        );
    }
}

/// Every non-`SrcOver` Porter-Duff mode renders as itself on a gradient.
///
/// ## Why a pass here cannot be an accident
///
/// The defect rendered EVERY mode as `SrcOver`. So the discriminating question
/// is whether the expected pixel differs from `SrcOver`'s, and the loop asserts
/// exactly that before it compares anything — a mode whose prediction happened
/// to coincide with `SrcOver` would be evidence of nothing and fails the
/// premise instead of passing silently.
///
/// The sampled pixel is the surface centre, where coverage is 1 in both the
/// gradient's own SDF and the (absent) clip. That keeps this half independent
/// of the coverage correction: it asserts the mode, not the feathering.
fn every_porter_duff_mode_renders_as_itself(kind: GradientKind) {
    let Ok(renderer) = HeadlessRenderer::new() else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };

    let destination = premultiplied(DESTINATION, 1.0);
    let source = premultiplied(SOURCE, 1.0);
    let as_srcover = as_bytes(blend(BlendMode::SrcOver, source, destination));

    for mode in PORTER_DUFF_MODES {
        if mode == BlendMode::SrcOver {
            continue;
        }
        let expected = blend(mode, source, destination);
        assert_ne!(
            as_bytes(expected),
            as_srcover,
            "{kind:?}/{mode:?}: this mode's prediction equals SrcOver's, so the \
             scene cannot tell the fix from the defect — pick a source colour or \
             destination that separates them before trusting a pass here"
        );
        assert_pixel(
            gradient_at_full_strength(&renderer, kind, mode),
            expected,
            &format!(
                "{kind:?}/{mode:?}: a gradient paint carrying this blend mode must \
                 render it. Reading {as_srcover:?} instead means the mode was \
                 accepted and discarded, and the pipeline is still the fixed \
                 ALPHA_BLENDING one"
            ),
        );
    }
}

#[test]
fn a_linear_gradient_renders_every_porter_duff_mode() {
    every_porter_duff_mode_renders_as_itself(GradientKind::Linear);
}

#[test]
fn a_radial_gradient_renders_every_porter_duff_mode() {
    every_porter_duff_mode_renders_as_itself(GradientKind::Radial);
}

#[test]
fn a_sweep_gradient_renders_every_porter_duff_mode() {
    every_porter_duff_mode_renders_as_itself(GradientKind::Sweep);
}

// ── Acceptance 2: partial coverage feathers, or falls back and says so ───────

/// The coverage contract for one mode on one gradient kind, asserted against
/// both devices — the same shape as the tessellated path's suite, because it is
/// the same contract (ADR-0057).
fn assert_partial_coverage_feathers(kind: GradientKind, mode: BlendMode) {
    let Ok(feathering) = HeadlessRenderer::new() else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };
    let folded = HeadlessRenderer::without_dual_source_blending()
        .expect("an adapter that answered once must answer again with fewer features");

    let feathered_fringe = coverage_correct(mode, SOURCE, DESTINATION, FRINGE_COVERAGE);
    let folded_fringe = coverage_folded(mode, SOURCE, DESTINATION, FRINGE_COVERAGE);
    assert_ne!(
        as_bytes(feathered_fringe),
        as_bytes(folded_fringe),
        "{kind:?}/{mode:?}: the corrected and folded predictions agree at coverage \
         {FRINGE_COVERAGE}, so this scene cannot tell them apart"
    );

    let untouched_destination = premultiplied(DESTINATION, 1.0);
    let at_full_coverage = coverage_correct(mode, SOURCE, DESTINATION, 1.0);

    let folded_samples = gradient_through_an_anti_aliased_clip(&folded, kind, mode);
    assert_pixel(
        folded_samples.outside_the_clip,
        untouched_destination,
        &format!("{kind:?}/{mode:?} without a second blend source, outside the clip"),
    );
    assert_pixel(
        folded_samples.fully_covered,
        at_full_coverage,
        &format!("{kind:?}/{mode:?} without a second blend source, fully covered"),
    );
    assert_pixel(
        folded_samples.partially_covered,
        folded_fringe,
        &format!(
            "{kind:?}/{mode:?} without a second blend source, partially covered: \
             coverage has nowhere to ride but the source alpha. This is the \
             documented fallback, not the contract"
        ),
    );

    if !feathering.supports_dual_source_blending() {
        eprintln!(
            "skipping the feathered half of {kind:?}/{mode:?}: this adapter does not \
             expose DUAL_SOURCE_BLENDING, so both renderers take the folded path"
        );
        return;
    }

    let feathered_samples = gradient_through_an_anti_aliased_clip(&feathering, kind, mode);
    assert_pixel(
        feathered_samples.outside_the_clip,
        untouched_destination,
        &format!("{kind:?}/{mode:?}, outside the clip"),
    );
    assert_pixel(
        feathered_samples.fully_covered,
        at_full_coverage,
        &format!(
            "{kind:?}/{mode:?}, fully covered: the correction must not change a \
             pixel the clip admits whole"
        ),
    );
    assert_pixel(
        feathered_samples.partially_covered,
        feathered_fringe,
        &format!(
            "{kind:?}/{mode:?}, partially covered: the pixel must read as the mode \
             applied at full strength and then mixed with the untouched \
             destination by coverage {FRINGE_COVERAGE}. Reading the folded value \
             means the gradient honours the mode but still folds coverage into \
             the source alpha"
        ),
    );
}

/// The seven modes whose destination factor cannot absorb `1 − coverage`, on a
/// linear gradient.
///
/// One test per mode rather than a loop, so a failure names the mode without a
/// message having to.
#[test]
fn a_linear_gradient_feathers_clear() {
    assert_partial_coverage_feathers(GradientKind::Linear, BlendMode::Clear);
}

#[test]
fn a_linear_gradient_feathers_src() {
    assert_partial_coverage_feathers(GradientKind::Linear, BlendMode::Src);
}

#[test]
fn a_linear_gradient_feathers_src_in() {
    assert_partial_coverage_feathers(GradientKind::Linear, BlendMode::SrcIn);
}

#[test]
fn a_linear_gradient_feathers_dst_in() {
    assert_partial_coverage_feathers(GradientKind::Linear, BlendMode::DstIn);
}

#[test]
fn a_linear_gradient_feathers_src_out() {
    assert_partial_coverage_feathers(GradientKind::Linear, BlendMode::SrcOut);
}

#[test]
fn a_linear_gradient_feathers_dst_atop() {
    assert_partial_coverage_feathers(GradientKind::Linear, BlendMode::DstATop);
}

#[test]
fn a_linear_gradient_feathers_modulate() {
    assert_partial_coverage_feathers(GradientKind::Linear, BlendMode::Modulate);
}

/// The radial and sweep shaders are separate modules with their own fragment
/// entry points, so the second blend source has to reach each of them
/// separately. `Clear` is the mode the defect was reported against, and the one
/// whose failure is loudest: a wrong fringe there is a hard-edged hole.
#[test]
fn a_radial_gradient_feathers_clear() {
    assert_partial_coverage_feathers(GradientKind::Radial, BlendMode::Clear);
}

#[test]
fn a_sweep_gradient_feathers_clear() {
    assert_partial_coverage_feathers(GradientKind::Sweep, BlendMode::Clear);
}

/// `DstIn` is the other class — destination factor `SrcAlpha`, so its second
/// blend source is `coverage × (1 − alpha)` rather than `coverage`. Proving
/// `Clear` on a kind says nothing about whether that kind's
/// `destination_alpha_scale` override arrived.
#[test]
fn a_radial_gradient_feathers_dst_in() {
    assert_partial_coverage_feathers(GradientKind::Radial, BlendMode::DstIn);
}

#[test]
fn a_sweep_gradient_feathers_dst_in() {
    assert_partial_coverage_feathers(GradientKind::Sweep, BlendMode::DstIn);
}

/// `DstOut` is the erase-by-alpha mode that must NOT be corrected: its
/// `(Zero, OneMinusSrcAlpha)` pair absorbs `1 − coverage` on its own, and
/// correcting it would apply the correction twice.
///
/// Unlike the tessellated path's `DstOut` oracle, this one can predict the
/// value: a gradient never takes the SSAA tile path, so its fringe coverage is
/// the clip's derived [`FRINGE_COVERAGE`] and the folded and corrected
/// predictions are the same number by construction.
#[test]
fn a_gradient_does_not_correct_dst_out() {
    let Ok(feathering) = HeadlessRenderer::new() else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };
    if !feathering.supports_dual_source_blending() {
        eprintln!("skipping: this adapter does not expose DUAL_SOURCE_BLENDING");
        return;
    }
    let folded = HeadlessRenderer::without_dual_source_blending()
        .expect("an adapter that answered once must answer again with fewer features");

    let fringe = coverage_correct(BlendMode::DstOut, SOURCE, DESTINATION, FRINGE_COVERAGE);
    assert_eq!(
        as_bytes(fringe),
        as_bytes(coverage_folded(
            BlendMode::DstOut,
            SOURCE,
            DESTINATION,
            FRINGE_COVERAGE
        )),
        "premise: DstOut's factor pair is supposed to make these two the same \
         value; if they differ, DstOut belongs in the corrected set"
    );

    let untouched = as_bytes(premultiplied(DESTINATION, 1.0));
    let at_full_coverage = as_bytes(coverage_correct(
        BlendMode::DstOut,
        SOURCE,
        DESTINATION,
        1.0,
    ));
    assert_ne!(
        as_bytes(fringe),
        untouched,
        "premise: the sampled column must be partially covered, not untouched"
    );
    assert_ne!(
        as_bytes(fringe),
        at_full_coverage,
        "premise: the sampled column must be partially covered, not fully"
    );

    for renderer in [&feathering, &folded] {
        let samples = gradient_through_an_anti_aliased_clip(
            renderer,
            GradientKind::Linear,
            BlendMode::DstOut,
        );
        assert_pixel(
            samples.partially_covered,
            fringe,
            "DstOut on a gradient: already coverage-correct, so neither device may \
             change it",
        );
    }
}

// ── Acceptance 3: SrcOver must not move ──────────────────────────────────────

/// The columns [`srcover_gradients_are_unchanged_to_within_one_bit`] samples on
/// the ramp.
///
/// All three are interior — no clip, and far enough from the gradient rect's
/// own anti-aliased border that their coverage is exactly 1 — so the only
/// quantisation between the shader and the byte is the `Rgba8Unorm` write.
/// Their `t` values are `8.5/64`, `32.5/64` and `56.5/64`, well clear of both
/// clamped ends.
const RAMP_COLUMNS: [u32; 3] = [8, 32, 56];

/// The left end of the ramp: translucent, so the source alpha is neither 0 nor
/// 1 and where the premultiply happens is observable.
const RAMP_START: Color = Color::rgba(244, 60, 20, 188);
/// The right end of the ramp, differing from [`RAMP_START`] in two channels.
const RAMP_END: Color = Color::rgba(20, 60, 208, 188);

/// The colour the whole-interior check paints, at every stop.
///
/// Constant so that every interior pixel has ONE prediction and the check can
/// cover the frame rather than three points.
const CONSTANT_SOURCE: Color = Color::rgba(116, 4, 44, 94);

/// Inset from the surface edge for the whole-interior check, in pixels.
///
/// The gradient rect spans the whole surface, so even the outermost pixel
/// centre is 31.5 inside its rounded-box SDF and its coverage is 1. Two pixels
/// of margin costs nothing and keeps the check clear of the rasteriser's own
/// edge entirely.
const INTERIOR_INSET: u32 = 2;

/// What a `SrcOver` gradient rendered before the pipelines became
/// blend-mode-keyed, captured from that tree at [`RAMP_COLUMNS`].
const SRCOVER_RAMP: [[u8; 4]; 3] = [[191, 64, 43, 230], [129, 64, 95, 230], [67, 64, 147, 230]];

/// How far a `SrcOver` gradient may move: one least-significant bit, and no
/// more.
///
/// ## What moved, and why it is not zero
///
/// A gradient's fragment now emits PREMULTIPLIED colour, because that is the
/// only form in which fixed-function Porter-Duff blending is correct and
/// thirteen other modes need it. `SrcOver` therefore pairs with
/// `PREMULTIPLIED_ALPHA_BLENDING` (source factor `One`) where it used to pair
/// with `ALPHA_BLENDING` (source factor `SrcAlpha`).
///
/// `colour x alpha` is the same product either way, but not the same
/// arithmetic: computing it in the shader means it is rounded to the render
/// target's 8 bits BEFORE the blender adds the destination term, where the
/// blender used to multiply two 8-bit values at its own higher precision. The
/// source term therefore loses up to half a bit, and about half of all pixels
/// land on the other side of a rounding boundary.
///
/// Measured on this workspace's headless capture over twelve full-surface
/// `SrcOver` gradients (three kinds x four colour pairs, 49 152 pixels): 51% of
/// pixels changed and EVERY change was exactly 1 in a single channel — no pixel
/// moved by 2. The tessellated shape path took the identical change, for the
/// identical reason, when its shader began premultiplying.
///
/// So this is not "pixel-identical", and the test says so rather than hiding a
/// real shift behind a tolerance that sounds like slack. What the bound buys is
/// still decisive: a wrong blend state, a dropped coverage term, or a pipeline
/// keyed to the wrong mode moves a pixel by far more than one bit.
const SRCOVER_QUANTISATION: i32 = 1;

/// Assert `actual` is within [`SRCOVER_QUANTISATION`] of `expected` per channel.
fn assert_within_one_bit(actual: [u8; 4], expected: [u8; 4], what: &str) {
    let within = actual
        .iter()
        .zip(expected)
        .all(|(&got, want)| (i32::from(got) - i32::from(want)).abs() <= SRCOVER_QUANTISATION);
    assert!(
        within,
        "{what}: expected {expected:?} (±{SRCOVER_QUANTISATION}), got {actual:?}"
    );
}

/// `SrcOver` — the path every gradient in the workspace takes — renders what it
/// rendered before, to within the one bit [`SRCOVER_QUANTISATION`] accounts for.
///
/// Two halves, because three sample points are not a frame:
///
/// - the ramp columns against [`SRCOVER_RAMP`], which was measured on the tree
///   before this change and is the direct before/after comparison;
/// - every interior pixel of a constant-colour gradient of each kind against
///   the CPU model, which covers the whole surface and all three shader
///   modules rather than a chosen row.
#[test]
fn srcover_gradients_are_unchanged_to_within_one_bit() {
    let Ok(renderer) = HeadlessRenderer::new() else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };

    let pixels = render_gradient_over_destination(
        &renderer,
        gradient(GradientKind::Linear, RAMP_START, RAMP_END),
        BlendMode::SrcOver,
    );
    let ramp: [[u8; 4]; 3] =
        std::array::from_fn(|i| pixel_at(&pixels, (RAMP_COLUMNS[i], SIDE / 2)));

    // Premise: this really is a ramp. A constant colour here would mean the
    // stops never reached the shader, and the comparison below would be
    // pinning a solid fill.
    assert!(
        ramp[0][0] > ramp[2][0] && ramp[0][2] < ramp[2][2],
        "premise: the ramp must run red to blue left to right, got {ramp:?}"
    );

    for ((column, actual), before) in RAMP_COLUMNS.into_iter().zip(ramp).zip(SRCOVER_RAMP) {
        assert_within_one_bit(
            actual,
            before,
            &format!(
                "column {column} under SrcOver: this is the common path, and it must \
                 render what it rendered before gradients were keyed by blend mode"
            ),
        );
    }

    // The whole frame, per kind, against the model rather than a capture — so
    // this half also fails if the capture above was taken from a broken tree.
    let expected = as_bytes(blend(
        BlendMode::SrcOver,
        premultiplied(CONSTANT_SOURCE, 1.0),
        premultiplied(DESTINATION, 1.0),
    ));
    for kind in GRADIENT_KINDS {
        let pixels = render_gradient_over_destination(
            &renderer,
            constant_gradient(kind, CONSTANT_SOURCE),
            BlendMode::SrcOver,
        );
        for row in INTERIOR_INSET..SIDE - INTERIOR_INSET {
            for column in INTERIOR_INSET..SIDE - INTERIOR_INSET {
                assert_within_one_bit(
                    pixel_at(&pixels, (column, row)),
                    expected,
                    &format!("{kind:?} under SrcOver at ({column}, {row})"),
                );
            }
        }
    }
}
