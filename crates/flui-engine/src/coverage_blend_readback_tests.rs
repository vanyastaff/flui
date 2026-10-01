//! Readback evidence that partial clip coverage feathers a blend instead of
//! applying it at full strength — one oracle per mode that needs the
//! correction, plus explicit refusal without a second blend source.
//!
//! ## Why this file exists rather than more cases in the clip suite
//!
//! `clip_layer_readback_tests` answers "does a clip clip?" and counts pixels.
//! These answer "is the arithmetic right?" and assert an exact byte, which
//! needs a scene built backwards from a coverage value that can be predicted on
//! paper. Mixing the two would make both harder to read.
//!
//! ## The seven modes, and why they are seven
//!
//! Folding coverage into the source alpha makes the blender compute
//! `S(a·cov)·(src·cov) + D(a·cov)·dst` where the coverage-correct answer is
//! `mix(dst, S(a)·src + D(a)·dst, cov)`. The source halves always agree — no
//! mode's source factor reads source alpha — so the two differ exactly when the
//! mode's DESTINATION factor cannot absorb `1 − cov`. That is a property of the
//! factor pair, not of `Clear`: `Clear`, `Src`, `SrcIn`, `SrcOut`, `Modulate`,
//! `DstIn` and `DstATop` all fail it, while `DstOut` — the other erase-by-alpha
//! mode — passes, because `(Zero, OneMinusSrcAlpha)` has exactly the absorbing
//! shape. `crate::pipeline_cache::destination_alpha_scale_for` is the classification
//! under test; the exhaustive cross-check that it agrees with
//! `blend_state_for`'s own factor table lives beside it.
//!
//! `SrcOut`, `DstATop` and `Modulate` had no readback coverage at all before
//! this file: they were derived algebraically, and hardware is the authority.
//!
//! ## What makes a sample point evidence
//!
//! Each oracle renders on a device with DUAL_SOURCE_BLENDING and checks full,
//! partial and excluded samples against the coverage-correct definition. The
//! same scene on a device with the feature deliberately withheld must return
//! UnsupportedCoverageBlend; the folded prediction remains a witness showing
//! why accepting that operation would change its fringe.
//!
//! The CPU model lives in `crate::blend_oracle`, shared with the gradient
//! path's suite: it is built from `blend_state_for`'s factors — production's
//! mode table, so a mode cannot be classified one way here and another there —
//! but it never reproduces the correction itself. The corrected prediction is
//! the coverage-correct DEFINITION (`mix(dst, blend_at_full_coverage, cov)`),
//! so a fix that is consistently wrong fails rather than agreeing with
//! itself.

use flui_foundation::geometry::{RRect, Rect};
use flui_layer::SceneBuilder;
use flui_painting::{BlendMode, Canvas, Paint};
use flui_painting::{
    paint::{Clip, ClipOp},
    styling::Color,
};

use crate::{
    blend_oracle::{
        CLIP_HEIGHT, CLIP_LEFT, CLIP_RADIUS, CLIP_TOP, CLIP_WIDTH, EdgeSamples, FRINGE_COVERAGE,
        SIDE, as_bytes, assert_pixel, coverage_correct as coverage_correct_for,
        coverage_folded as coverage_folded_for, premultiplied, sample_the_clip_edge,
    },
    headless::HeadlessRenderer,
};

/// The destination every oracle blends into: opaque, and distinct in all three
/// channels so a per-channel factor error cannot hide.
const DESTINATION: Color = Color::rgba(200, 120, 60, 255);

/// The source every oracle blends. Translucent on purpose: the modes whose
/// destination factor is `SrcAlpha` (`DstIn`, `DstATop`) reduce to "leave the
/// destination alone" at full opacity, and would then pass against a broken
/// blender.
const SOURCE: Color = Color::rgba(0, 220, 40, 128);

/// Paints `DESTINATION` over the surface, then `SOURCE` with `mode` through an
/// anti-aliased rounded clip whose left edge falls mid-column.
fn blend_through_an_anti_aliased_clip(renderer: &HeadlessRenderer, mode: BlendMode) -> EdgeSamples {
    try_blend_through_an_anti_aliased_clip(renderer, mode)
        .expect("the admitted capture must rasterize the scene")
}

fn try_blend_through_an_anti_aliased_clip(
    renderer: &HeadlessRenderer,
    mode: BlendMode,
) -> crate::EngineResult<EdgeSamples> {
    let full_surface = Rect::from_xywh(0.0, 0.0, f64::from(SIDE as f32), f64::from(SIDE as f32));

    let tree = {
        let mut builder = SceneBuilder::new();
        let mut canvas = Canvas::new();
        canvas.draw_rect(
            full_surface,
            &Paint::fill(DESTINATION).with_anti_alias(false),
        );
        canvas.save();
        canvas.clip_rrect_ext(
            RRect::from_rect_circular(
                Rect::from_xywh(
                    f64::from(CLIP_LEFT),
                    f64::from(CLIP_TOP),
                    f64::from(CLIP_WIDTH),
                    f64::from(CLIP_HEIGHT),
                ),
                f64::from(CLIP_RADIUS),
            ),
            ClipOp::Intersect,
            Clip::AntiAlias,
        );
        canvas.draw_rect(
            full_surface,
            &Paint::fill(SOURCE)
                .with_anti_alias(false)
                .with_blend_mode(mode),
        );
        canvas.restore();
        builder.add_picture(canvas.finish());
        builder.build()
    };

    let pixels = renderer.render_layer_tree(&tree, (SIDE, SIDE))?;
    Ok(sample_the_clip_edge(&pixels))
}

// ── This scene's bindings of the shared CPU blender ──────────────────────────

/// [`crate::blend_oracle::coverage_correct`] bound to this file's source and
/// destination.
fn coverage_correct(mode: BlendMode, coverage: f32) -> [f32; 4] {
    coverage_correct_for(mode, SOURCE, DESTINATION, coverage)
}

/// [`crate::blend_oracle::coverage_folded`] bound to this file's source and
/// destination.
fn coverage_folded(mode: BlendMode, coverage: f32) -> [f32; 4] {
    coverage_folded_for(mode, SOURCE, DESTINATION, coverage)
}

/// The whole contract for one blend mode, asserted against both devices.
fn assert_partial_coverage_feathers(mode: BlendMode) {
    let Some(feathering) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let folded = pollster::block_on(feathering.without_dual_source_blending())
        .expect("an adapter that answered once must answer again with fewer features");

    let feathered_fringe = coverage_correct(mode, FRINGE_COVERAGE);
    let folded_fringe = coverage_folded(mode, FRINGE_COVERAGE);
    assert_ne!(
        as_bytes(feathered_fringe),
        as_bytes(folded_fringe),
        "{mode:?}: the corrected and folded predictions agree at coverage \
         {FRINGE_COVERAGE}, so this scene cannot tell them apart — pick a source \
         colour, destination, or coverage that separates them before trusting a \
         pass here"
    );

    let untouched_destination = premultiplied(DESTINATION, 1.0);
    let at_full_coverage = coverage_correct(mode, 1.0);

    assert!(
        matches!(
            try_blend_through_an_anti_aliased_clip(&folded, mode),
            Err(crate::EngineError::UnsupportedCoverageBlend { mode: refused }) if refused == mode
        ),
        "{mode:?} must refuse unsupported coverage rather than return the folded fringe"
    );

    if !feathering.supports_dual_source_blending() {
        eprintln!(
            "skipping the feathered half of {mode:?}: this adapter does not expose \
             DUAL_SOURCE_BLENDING, so the direct operation is refused"
        );
        return;
    }

    let feathered_samples = blend_through_an_anti_aliased_clip(&feathering, mode);
    assert_pixel(
        feathered_samples.outside_the_clip,
        untouched_destination,
        &format!("{mode:?}, outside the clip"),
    );
    assert_pixel(
        feathered_samples.fully_covered,
        at_full_coverage,
        &format!(
            "{mode:?}, fully covered: the correction must not change a pixel the \
             clip admits whole"
        ),
    );
    assert_pixel(
        feathered_samples.partially_covered,
        feathered_fringe,
        &format!(
            "{mode:?}, partially covered: the pixel must read as the mode applied \
             at full strength and then mixed with the untouched destination by \
             coverage {FRINGE_COVERAGE}"
        ),
    );
}

/// The seven modes whose destination factor cannot absorb `1 - coverage`
/// (`Clear`, `Src`, `SrcIn`, `SrcOut` and `Modulate` scale the destination by
/// coverage alone; `DstIn` and `DstATop` by `coverage * (1 - alpha)`) each
/// feather a partially covered edge.
fn modes_that_cannot_absorb_coverage_feather_their_partially_covered_edge() {
    for mode in [
        BlendMode::Clear,
        BlendMode::Src,
        BlendMode::SrcIn,
        BlendMode::DstIn,
        BlendMode::SrcOut,
        BlendMode::DstATop,
        BlendMode::Modulate,
    ] {
        assert_partial_coverage_feathers(mode);
    }
}

/// `DstOut` is the erase-by-alpha mode one would expect to break alongside
/// `Clear`, and it does not: `(Zero, OneMinusSrcAlpha)` absorbs `1 − coverage`
/// on its own.
///
/// The assertion is that the two devices agree pixel-for-pixel, rather than a
/// predicted value, because `DstOut` is tile-safe and therefore renders through
/// the SSAA tile path (`pipeline_cache::ssaa_eligible_for`), whose 2× supersampled
/// edge reports a different coverage for the same column than the tessellated
/// path does. Predicting that number would pin this test to the SSAA sample
/// grid; agreeing across devices pins what actually matters — that nothing in
/// this change reached a mode it was not meant to.
fn dst_out_renders_the_same_with_and_without_a_second_blend_source() {
    let Some(feathering) = crate::test_support::renderer_or_skip() else {
        return;
    };
    if !feathering.supports_dual_source_blending() {
        eprintln!("skipping: this adapter does not expose DUAL_SOURCE_BLENDING");
        return;
    }
    let folded = pollster::block_on(feathering.without_dual_source_blending())
        .expect("an adapter that answered once must answer again with fewer features");

    let feathered_samples = blend_through_an_anti_aliased_clip(&feathering, BlendMode::DstOut);
    let folded_samples = blend_through_an_anti_aliased_clip(&folded, BlendMode::DstOut);

    // Premise: the sampled column really is a partially covered one. Between
    // the untouched destination and the mode at full strength, and equal to
    // neither — otherwise the equality below would hold trivially.
    let untouched = as_bytes(premultiplied(DESTINATION, 1.0));
    let at_full_coverage = as_bytes(coverage_correct(BlendMode::DstOut, 1.0));
    let fringe = feathered_samples.partially_covered;
    assert!(
        (at_full_coverage[0]..untouched[0]).contains(&fringe[0]),
        "premise: the sampled column must be partially covered — expected a red \
         channel strictly between {} (full strength) and {} (untouched), got {}",
        at_full_coverage[0],
        untouched[0],
        fringe[0],
    );

    assert_eq!(
        feathered_samples.partially_covered, folded_samples.partially_covered,
        "DstOut's destination factor already absorbs 1 - coverage, so a second \
         blend source must not reach it: correcting it would apply the \
         correction twice"
    );
    assert_eq!(
        feathered_samples.fully_covered, folded_samples.fully_covered,
        "DstOut at full coverage must be untouched by this change"
    );
}

/// Coverage-blend contract, read back from the GPU: dst-out independence of the
/// second blend source, and feathered edges for modes that cannot absorb coverage.
#[test]
fn coverage_blend_reads_back_as_specified() {
    dst_out_renders_the_same_with_and_without_a_second_blend_source();
    modes_that_cannot_absorb_coverage_feather_their_partially_covered_edge();
}
