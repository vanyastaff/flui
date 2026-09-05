//! Readback evidence that partial clip coverage feathers a blend instead of
//! applying it at full strength — one oracle per mode that needs the
//! correction, plus the fallback each keeps without a second blend source.
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
//! shape. `super::pipeline::destination_alpha_scale_for` is the classification
//! under test; the exhaustive cross-check that it agrees with
//! `blend_state_for`'s own factor table lives beside it.
//!
//! `SrcOut`, `DstATop` and `Modulate` had no readback coverage at all before
//! this file: they were derived algebraically, and hardware is the authority.
//!
//! ## What makes a sample point evidence
//!
//! Each oracle renders the same scene twice — once on a device with
//! `DUAL_SOURCE_BLENDING`, once on a device built with it deliberately withheld
//! — and asserts three points against a CPU model of the fixed-function
//! blender:
//!
//! - **fully covered**: both renders must equal the mode at full strength. The
//!   two predictions are identical here by construction, so this is the check
//!   that the correction changed nothing it was not supposed to.
//! - **partially covered**: the two predictions DIFFER, and each render must
//!   match its own. The test asserts they differ before comparing anything, so
//!   it cannot pass by both formulas agreeing.
//! - **outside the clip**: both must leave the destination untouched.
//!
//! The CPU model is built from `blend_state_for`'s factors — production's mode
//! table, so a mode cannot be classified one way here and another there — but
//! it never reproduces the correction itself. The corrected prediction is the
//! coverage-correct DEFINITION (`mix(dst, blend_at_full_coverage, cov)`), so a
//! fix that is consistently wrong fails rather than agreeing with itself.

use flui_layer::{LayerTree, SceneBuilder};
use flui_painting::{BlendMode, Canvas, Paint};
use flui_types::{
    Color, Rect,
    geometry::{RRect, px},
    painting::{Clip, ClipOp},
};

use super::{headless::HeadlessRenderer, pipeline::blend_state_for};

const SIDE: u32 = 64;

/// The destination every oracle blends into: opaque, and distinct in all three
/// channels so a per-channel factor error cannot hide.
const DESTINATION: Color = Color::rgba(200, 120, 60, 255);

/// The source every oracle blends. Translucent on purpose: the modes whose
/// destination factor is `SrcAlpha` (`DstIn`, `DstATop`) reduce to "leave the
/// destination alone" at full opacity, and would then pass against a broken
/// blender.
const SOURCE: Color = Color::rgba(0, 220, 40, 128);

/// The clip's left edge, in device pixels.
///
/// Deliberately a quarter-pixel past a column boundary. The clip's bounding-box
/// scissor truncates (`state_stack::clip_rect`), so it starts at column 15 and
/// the feathered column at 15 survives it. The mirror-image choice on the RIGHT
/// edge does not: there the scissor ends at `floor(right)` and cuts the one
/// column the feather lives in — which is what `clip_rect`'s own comment means
/// by "the outer half of the feather is lost there".
const CLIP_LEFT: f32 = 15.75;

/// Coverage `sdfToAlpha` reports at [`FRINGE_COLUMN`], derived rather than
/// measured.
///
/// On the clip's straight left edge the rounded-box SDF reduces to
/// `distance = CLIP_LEFT − x`, whose screen-space gradient magnitude is 1, so
/// `edge_width = 0.5`. At the column's pixel centre `x = 15.5` that is
/// `distance = 0.25`, and `1 − smoothstep(−0.5, 0.5, 0.25)` with
/// `t = 0.75` is `1 − 0.75²·(3 − 2·0.75) = 1 − 0.84375`.
const FRINGE_COVERAGE: f32 = 0.15625;

/// The single column the clip's left edge partially covers.
const FRINGE_COLUMN: u32 = 15;
/// A column well inside the clip, clear of both the edge and the corner arcs.
const COVERED_COLUMN: u32 = 20;
/// A column outside the clip, and outside its bounding-box scissor.
const UNCOVERED_COLUMN: u32 = 10;
/// The sampled row: the clip's vertical midline, where the corner arcs cannot
/// reach and the edge is exactly vertical.
const SAMPLE_ROW: u32 = 32;

/// Byte tolerance per channel, absorbing `Rgba8Unorm` quantisation and the
/// difference between the GPU's `smoothstep` and this file's arithmetic.
const TOLERANCE: i32 = 2;

/// Every fixed-function mode `blend_state_for` maps, so the classification
/// cross-check below cannot quietly skip one.
const PORTER_DUFF_MODES: [BlendMode; 14] = [
    BlendMode::Clear,
    BlendMode::Src,
    BlendMode::Dst,
    BlendMode::SrcOver,
    BlendMode::DstOver,
    BlendMode::SrcIn,
    BlendMode::DstIn,
    BlendMode::SrcOut,
    BlendMode::DstOut,
    BlendMode::SrcATop,
    BlendMode::DstATop,
    BlendMode::Xor,
    BlendMode::Plus,
    BlendMode::Modulate,
];

/// The three sampled pixels of one render.
#[derive(Debug, Clone, Copy)]
struct EdgeSamples {
    outside_the_clip: [u8; 4],
    partially_covered: [u8; 4],
    fully_covered: [u8; 4],
}

/// Paints `DESTINATION` over the surface, then `SOURCE` with `mode` through an
/// anti-aliased rounded clip whose left edge falls mid-column.
fn blend_through_an_anti_aliased_clip(renderer: &HeadlessRenderer, mode: BlendMode) -> EdgeSamples {
    let full_surface = Rect::from_xywh(px(0.0), px(0.0), px(SIDE as f32), px(SIDE as f32));

    let mut tree = LayerTree::new();
    {
        let mut builder = SceneBuilder::new(&mut tree);
        let mut canvas = Canvas::new();
        canvas.draw_rect(
            full_surface,
            &Paint::fill(DESTINATION).with_anti_alias(false),
        );
        canvas.save();
        canvas.clip_rrect_ext(
            RRect::from_rect_circular(
                // Height 48 with radius 8 keeps both corner arcs inside
                // rows 8..16 and 48..56, clear of `SAMPLE_ROW`.
                Rect::from_xywh(px(CLIP_LEFT), px(8.0), px(32.0), px(48.0)),
                px(8.0),
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
        builder.build();
    }

    let pixels = renderer
        .render_layer_tree(&tree, (SIDE, SIDE))
        .expect("the headless capture path must rasterize the scene");
    let at = |column: u32| {
        let index = ((SAMPLE_ROW * SIDE + column) * 4) as usize;
        [
            pixels[index],
            pixels[index + 1],
            pixels[index + 2],
            pixels[index + 3],
        ]
    };

    EdgeSamples {
        outside_the_clip: at(UNCOVERED_COLUMN),
        partially_covered: at(FRINGE_COLUMN),
        fully_covered: at(COVERED_COLUMN),
    }
}

// ── CPU model of the fixed-function blender ───────────────────────────────────

/// One `wgpu::BlendFactor`, evaluated for a single channel.
///
/// `Src`/`Dst` take the channel itself, which is what the hardware does in
/// both the colour and the alpha component — in the alpha component the
/// channel IS the alpha.
fn factor(
    blend_factor: wgpu::BlendFactor,
    source: f32,
    source_alpha: f32,
    destination: f32,
    destination_alpha: f32,
) -> f32 {
    match blend_factor {
        wgpu::BlendFactor::Zero => 0.0,
        wgpu::BlendFactor::One => 1.0,
        wgpu::BlendFactor::Src => source,
        wgpu::BlendFactor::OneMinusSrc => 1.0 - source,
        wgpu::BlendFactor::SrcAlpha => source_alpha,
        wgpu::BlendFactor::OneMinusSrcAlpha => 1.0 - source_alpha,
        wgpu::BlendFactor::Dst => destination,
        wgpu::BlendFactor::OneMinusDst => 1.0 - destination,
        wgpu::BlendFactor::DstAlpha => destination_alpha,
        wgpu::BlendFactor::OneMinusDstAlpha => 1.0 - destination_alpha,
        unmodelled => panic!(
            "{unmodelled:?} reached this CPU blender, which models only the factors \
             `blend_state_for` emits; add it here rather than letting the oracle guess"
        ),
    }
}

/// `blend_state_for(mode)` applied on the CPU to a PREMULTIPLIED source and
/// destination.
fn blend(mode: BlendMode, source: [f32; 4], destination: [f32; 4]) -> [f32; 4] {
    let state = blend_state_for(mode);
    let mut out = [0.0; 4];
    for channel in 0..4 {
        let component = if channel == 3 {
            state.alpha
        } else {
            state.color
        };
        assert_eq!(
            component.operation,
            wgpu::BlendOperation::Add,
            "{mode:?} uses a blend operation this CPU model does not implement"
        );
        out[channel] = factor(
            component.src_factor,
            source[channel],
            source[3],
            destination[channel],
            destination[3],
        ) * source[channel]
            + factor(
                component.dst_factor,
                source[channel],
                source[3],
                destination[channel],
                destination[3],
            ) * destination[channel];
    }
    out
}

/// Premultiplied `color`, scaled by `alpha_scale`.
fn premultiplied(color: Color, alpha_scale: f32) -> [f32; 4] {
    let [r, g, b, a] = color.to_rgba_f32_array();
    let alpha = a * alpha_scale;
    [r * alpha, g * alpha, b * alpha, alpha]
}

/// The coverage-correct result: the mode applied at full strength, mixed with
/// the untouched destination by `coverage`.
///
/// This is the DEFINITION the fix must reproduce, not a restatement of it —
/// nothing here knows about a second blend source.
fn coverage_correct(mode: BlendMode, coverage: f32) -> [f32; 4] {
    let destination = premultiplied(DESTINATION, 1.0);
    let blended = blend(mode, premultiplied(SOURCE, 1.0), destination);
    std::array::from_fn(|channel| {
        destination[channel] + coverage * (blended[channel] - destination[channel])
    })
}

/// The result when coverage rides in the source alpha, which is what a device
/// without a second blend source can do and no more.
fn coverage_folded(mode: BlendMode, coverage: f32) -> [f32; 4] {
    blend(
        mode,
        premultiplied(SOURCE, coverage),
        premultiplied(DESTINATION, 1.0),
    )
}

fn as_bytes(channels: [f32; 4]) -> [u8; 4] {
    channels.map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn assert_pixel(actual: [u8; 4], expected: [f32; 4], what: &str) {
    let expected = as_bytes(expected);
    let within_tolerance = actual
        .iter()
        .zip(expected)
        .all(|(&got, want)| (i32::from(got) - i32::from(want)).abs() <= TOLERANCE);
    assert!(
        within_tolerance,
        "{what}: expected {expected:?} (±{TOLERANCE}), got {actual:?}"
    );
}

/// The whole contract for one blend mode, asserted against both devices.
fn assert_partial_coverage_feathers(mode: BlendMode) {
    let Ok(feathering) = HeadlessRenderer::new() else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };
    let folded = HeadlessRenderer::without_dual_source_blending()
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

    let folded_samples = blend_through_an_anti_aliased_clip(&folded, mode);
    assert_pixel(
        folded_samples.outside_the_clip,
        untouched_destination,
        &format!("{mode:?} without a second blend source, outside the clip"),
    );
    assert_pixel(
        folded_samples.fully_covered,
        at_full_coverage,
        &format!("{mode:?} without a second blend source, fully covered"),
    );
    assert_pixel(
        folded_samples.partially_covered,
        folded_fringe,
        &format!(
            "{mode:?} without a second blend source, partially covered: coverage \
             has nowhere to ride but the source alpha, so the mode applies at a \
             strength this pixel's coverage never asked for. This is the \
             documented fallback, not the contract"
        ),
    );

    if !feathering.supports_dual_source_blending() {
        eprintln!(
            "skipping the feathered half of {mode:?}: this adapter does not expose \
             DUAL_SOURCE_BLENDING, so both renderers take the folded path"
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

/// `Clear` is the mode the defect was reported against: `(Zero, Zero)` wipes the
/// destination whatever alpha the fragment emits.
#[test]
fn clear_feathers_its_partially_covered_edge() {
    assert_partial_coverage_feathers(BlendMode::Clear);
}

/// `Src` replaces the destination outright — `(One, Zero)` — so a fringe pixel
/// used to be replaced outright too.
#[test]
fn src_feathers_its_partially_covered_edge() {
    assert_partial_coverage_feathers(BlendMode::Src);
}

/// `SrcIn`'s `(DstAlpha, Zero)`: the destination factor is `Zero` like `Clear`'s,
/// so the same correction applies even though the source factor differs.
#[test]
fn src_in_feathers_its_partially_covered_edge() {
    assert_partial_coverage_feathers(BlendMode::SrcIn);
}

/// `DstIn`'s `(Zero, SrcAlpha)` — the first of the two modes whose destination
/// factor scales WITH source alpha, so its correction is
/// `coverage × (1 − alpha)` rather than `coverage`.
#[test]
fn dst_in_feathers_its_partially_covered_edge() {
    assert_partial_coverage_feathers(BlendMode::DstIn);
}

/// `SrcOut` — derived, never observed until here. `(OneMinusDstAlpha, Zero)`
/// against an opaque destination produces the same pixels as `Clear`, but
/// through its own pipeline and blend state, so nothing about `Clear` passing
/// implies this.
#[test]
fn src_out_feathers_its_partially_covered_edge() {
    assert_partial_coverage_feathers(BlendMode::SrcOut);
}

/// `DstATop` — derived, never observed until here. `(OneMinusDstAlpha,
/// SrcAlpha)` puts it in the `coverage × (1 − alpha)` class with `DstIn`.
#[test]
fn dst_atop_feathers_its_partially_covered_edge() {
    assert_partial_coverage_feathers(BlendMode::DstATop);
}

/// `Modulate` — derived, never observed until here. The only mode whose colour
/// and alpha components take different SOURCE factors (`Dst` and `DstAlpha`),
/// which is why it needs its own oracle rather than riding on `SrcIn`'s.
#[test]
fn modulate_feathers_its_partially_covered_edge() {
    assert_partial_coverage_feathers(BlendMode::Modulate);
}

/// Every mode `destination_alpha_scale_for` leaves alone is a mode whose folded
/// and coverage-correct results are the SAME value — exhaustively, over the
/// whole Porter-Duff set and a sweep of coverages.
///
/// This is the classification's real content, and it needs no GPU: a mode is
/// corrected iff folding coverage into the source alpha would change its
/// answer. Getting it wrong in either direction fails here — a mode left
/// uncorrected that needed it, and a mode corrected that did not (which would
/// then be corrected twice on the device).
#[test]
fn exactly_the_modes_that_need_correcting_are_the_ones_marked_for_it() {
    for mode in PORTER_DUFF_MODES {
        let folding_changes_the_answer =
            [0.05_f32, 0.25, 0.5, 0.75, 0.99]
                .into_iter()
                .any(|coverage| {
                    as_bytes(coverage_correct(mode, coverage))
                        != as_bytes(coverage_folded(mode, coverage))
                });
        assert_eq!(
            folding_changes_the_answer,
            super::pipeline::destination_alpha_scale_for(mode).is_some(),
            "{mode:?}: folding coverage into the source alpha {} its result, but \
             destination_alpha_scale_for {} it a correction",
            if folding_changes_the_answer {
                "CHANGES"
            } else {
                "preserves"
            },
            if folding_changes_the_answer {
                "denies"
            } else {
                "grants"
            },
        );
    }
}

/// `DstOut` is the erase-by-alpha mode one would expect to break alongside
/// `Clear`, and it does not: `(Zero, OneMinusSrcAlpha)` absorbs `1 − coverage`
/// on its own.
///
/// The assertion is that the two devices agree pixel-for-pixel, rather than a
/// predicted value, because `DstOut` is tile-safe and therefore renders through
/// the SSAA tile path (`pipeline::ssaa_eligible_for`), whose 2× supersampled
/// edge reports a different coverage for the same column than the tessellated
/// path does. Predicting that number would pin this test to the SSAA sample
/// grid; agreeing across devices pins what actually matters — that nothing in
/// this change reached a mode it was not meant to.
#[test]
fn dst_out_renders_the_same_with_and_without_a_second_blend_source() {
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
