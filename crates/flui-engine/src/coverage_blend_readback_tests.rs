//! Readback evidence that partial clip coverage feathers a blend instead of
//! applying it at full strength — one oracle per mode that needs the
//! correction, including portable rendering without a second blend source.
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
//! Each oracle checks full, partial and excluded samples on both a native
//! dual-source device and a device with the feature deliberately withheld.
//! The folded prediction witnesses the difference from the old fringe.
//!
//! The CPU model lives in `crate::blend_oracle`, shared with the gradient
//! path's suite: it is built from `blend_state_for`'s factors — production's
//! mode table, so a mode cannot be classified one way here and another there —
//! but it never reproduces the correction itself. The corrected prediction is
//! the coverage-correct DEFINITION (`mix(dst, blend_at_full_coverage, cov)`),
//! so a fix that is consistently wrong fails rather than agreeing with
//! itself.

use flui_foundation::geometry::{Offset, RRect, Rect};
use flui_layer::SceneBuilder;
use flui_painting::{BlendMode, Canvas, Paint};
use flui_painting::{
    paint::{Clip, ClipOp, Shader, TileMode},
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
fn assert_partial_coverage_feathers(
    mode: BlendMode,
    feathering: &HeadlessRenderer,
    folded: &HeadlessRenderer,
) {
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

    let portable_samples = blend_through_an_anti_aliased_clip(folded, mode);
    assert_pixel(
        portable_samples.outside_the_clip,
        untouched_destination,
        "portable outside",
    );
    assert_pixel(
        portable_samples.fully_covered,
        at_full_coverage,
        "portable full",
    );
    assert_pixel(
        portable_samples.partially_covered,
        feathered_fringe,
        "portable fringe",
    );
    if !feathering.supports_dual_source_blending() {
        return;
    }

    let feathered_samples = blend_through_an_anti_aliased_clip(feathering, mode);
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
fn modes_that_cannot_absorb_coverage_feather_their_partially_covered_edge(
    feathering: &HeadlessRenderer,
    folded: &HeadlessRenderer,
) {
    let mut failures = Vec::new();
    for mode in [
        BlendMode::Clear,
        BlendMode::Src,
        BlendMode::SrcIn,
        BlendMode::DstIn,
        BlendMode::SrcOut,
        BlendMode::DstATop,
        BlendMode::Modulate,
    ] {
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            assert_partial_coverage_feathers(mode, feathering, folded);
        }))
        .is_err()
        {
            failures.push(mode);
        }
    }
    assert!(failures.is_empty(), "coverage modes failed: {failures:?}");
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
fn dst_out_renders_the_same_with_and_without_a_second_blend_source(
    feathering: &HeadlessRenderer,
    folded: &HeadlessRenderer,
) {
    if !feathering.supports_dual_source_blending() {
        eprintln!("skipping: this adapter does not expose DUAL_SOURCE_BLENDING");
        return;
    }

    let feathered_samples = blend_through_an_anti_aliased_clip(feathering, BlendMode::DstOut);
    let folded_samples = blend_through_an_anti_aliased_clip(folded, BlendMode::DstOut);

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

/// Independent coverage survives transparent source paint and sequential operations.
fn portable_transparent_clear_plus_and_overlap(
    _native: &HeadlessRenderer,
    renderer: &HeadlessRenderer,
) {
    for (mode, source, destination, count, expected) in [
        (
            BlendMode::Clear,
            Color::rgba(0, 0, 0, 0),
            Color::WHITE,
            1,
            [0.5; 4],
        ),
        (
            BlendMode::Clear,
            Color::rgba(0, 0, 0, 0),
            Color::WHITE,
            2,
            [0.25; 4],
        ),
        (
            BlendMode::Plus,
            Color::rgba(204, 204, 204, 255),
            Color::rgba(204, 204, 204, 255),
            1,
            [0.9, 0.9, 0.9, 1.0],
        ),
    ] {
        let mut canvas = Canvas::new();
        let full = Rect::from_xywh(0.0, 0.0, 8.0, 8.0);
        canvas.draw_rect(full, &Paint::fill(destination).with_anti_alias(false));
        canvas.clip_rect_ext(
            Rect::from_xywh(2.5, 0.0, 5.5, 8.0),
            ClipOp::Intersect,
            Clip::AntiAlias,
        );
        for _ in 0..count {
            canvas.draw_rect(
                full,
                &Paint::fill(source)
                    .with_anti_alias(false)
                    .with_blend_mode(mode),
            );
        }
        let mut builder = SceneBuilder::new();
        builder.add_picture(canvas.finish());
        let pixels = renderer
            .render_layer_tree(&builder.build(), (8, 8))
            .expect("portable coverage render");
        let offset = (4 * 8 + 2) * 4;
        let actual = pixels[offset..offset + 4].try_into().expect("RGBA pixel");
        assert_pixel(
            actual,
            expected,
            &format!("{mode:?} count={count}, half coverage"),
        );
    }
    // Group compositing must use the same clamp-before-coverage definition.
    let full = Rect::from_xywh(0.0, 0.0, 8.0, 8.0);
    let color = Color::rgb(204, 204, 204);
    let mut canvas = Canvas::new();
    canvas.draw_rect(full, &Paint::fill(color).with_anti_alias(false));
    canvas.clip_rect_ext(
        Rect::from_xywh(2.5, 0.0, 5.5, 8.0),
        ClipOp::Intersect,
        Clip::AntiAlias,
    );
    canvas.save_layer(
        Some(full),
        &Paint::fill(Color::WHITE).with_blend_mode(BlendMode::Plus),
    );
    canvas.draw_rect(full, &Paint::fill(color).with_anti_alias(false));
    canvas.restore();
    let mut builder = SceneBuilder::new();
    builder.add_picture(canvas.finish());
    let pixels = renderer
        .render_layer_tree(&builder.build(), (8, 8))
        .expect("group saturated Plus render");
    let fringe = (4 * 8 + 2) * 4;
    assert_pixel(
        pixels[fringe..fringe + 4].try_into().expect("RGBA pixel"),
        [0.9, 0.9, 0.9, 1.0],
        "group saturated Plus fringe",
    );
    let outside = 4 * 8 * 4;
    assert_eq!(
        &pixels[outside..outside + 4],
        &[204, 204, 204, 255],
        "group Plus excluded destination"
    );
}

/// Varying source alpha must not stand in for independent clip coverage.
fn portable_fractional_src_gradient_preserves_destination(
    _native: &HeadlessRenderer,
    renderer: &HeadlessRenderer,
) {
    let mut canvas = Canvas::new();
    let full = Rect::from_xywh(0.0, 0.0, 8.0, 8.0);
    canvas.draw_rect(full, &Paint::fill(Color::WHITE).with_anti_alias(false));
    canvas.clip_rect_ext(
        Rect::from_xywh(2.5, 0.0, 5.5, 8.0),
        ClipOp::Intersect,
        Clip::AntiAlias,
    );
    canvas.draw_rect(
        full,
        &Paint::fill(Color::WHITE)
            .with_anti_alias(false)
            .with_blend_mode(BlendMode::Src)
            .with_shader(Shader::LinearGradient {
                from: Offset::new(0.0, 0.0),
                to: Offset::new(0.0, 8.0),
                colors: vec![Color::rgba(255, 0, 0, 0), Color::rgba(255, 0, 0, 255)],
                stops: Some(vec![0.0, 1.0]),
                tile_mode: TileMode::Clamp,
            }),
    );
    let mut builder = SceneBuilder::new();
    builder.add_picture(canvas.finish());
    let pixels = renderer
        .render_layer_tree(&builder.build(), (8, 8))
        .expect("portable gradient render");
    for y in [1usize, 4, 6] {
        let alpha = (y as f32 + 0.5) / 8.0;
        let offset = (y * 8 + 2) * 4;
        let actual = pixels[offset..offset + 4].try_into().expect("RGBA pixel");
        assert_pixel(
            actual,
            [0.5 + 0.5 * alpha, 0.5, 0.5, 0.5 + 0.5 * alpha],
            "Src varying alpha at half clip coverage",
        );
    }
}

/// Intrinsic gradient AA supplies geometry coverage even without a clip chain.
fn portable_intrinsic_gradient_clear_ignores_source_alpha(
    _native: &HeadlessRenderer,
    renderer: &HeadlessRenderer,
) {
    let render = |family, rounded, clipped, mode, color| {
        let mut canvas = Canvas::new();
        let full = Rect::from_xywh(0.0, 0.0, 16.0, 16.0);
        canvas.draw_rect(full, &Paint::fill(Color::WHITE).with_anti_alias(false));
        if clipped {
            canvas.clip_rect_ext(
                Rect::from_xywh(3.0, 3.0, 9.0, 9.0),
                ClipOp::Intersect,
                Clip::HardEdge,
            );
        }
        let colors = vec![color, color];
        let stops = Some(vec![0.0, 1.0]);
        let shader = match family {
            0 => Shader::LinearGradient {
                from: Offset::ZERO,
                to: Offset::new(16.0, 0.0),
                colors,
                stops,
                tile_mode: TileMode::Clamp,
            },
            1 => Shader::RadialGradient {
                center: Offset::new(8.0, 8.0),
                radius: 16.0,
                colors,
                stops,
                tile_mode: TileMode::Clamp,
                focal: None,
                focal_radius: None,
            },
            _ => Shader::SweepGradient {
                center: Offset::new(8.0, 8.0),
                colors,
                stops,
                tile_mode: TileMode::Clamp,
                start_angle: 0.0,
                end_angle: std::f64::consts::TAU,
            },
        };
        let paint = Paint::fill(Color::WHITE)
            .with_anti_alias(false)
            .with_blend_mode(mode)
            .with_shader(shader);
        let bounds = Rect::from_xywh(2.5, 2.0, 11.0, 12.0);
        if rounded {
            canvas.draw_rrect(RRect::from_rect_circular(bounds, 3.0), &paint);
        } else {
            canvas.draw_rect(bounds, &paint);
        }
        let mut builder = SceneBuilder::new();
        builder.add_picture(canvas.finish());
        renderer
            .render_layer_tree(&builder.build(), (16, 16))
            .expect("intrinsic coverage render")
    };
    for family in 0..3 {
        for rounded in [false, true] {
            for clipped in [false, true] {
                let reference = render(family, rounded, clipped, BlendMode::SrcOver, Color::BLACK);
                let cleared = render(
                    family,
                    rounded,
                    clipped,
                    BlendMode::Clear,
                    Color::rgba(0, 0, 0, 0),
                );
                if !clipped {
                    let fringe = reference[(8 * 16 + 2) * 4];
                    assert!(
                        (32..224).contains(&fringe),
                        "family {family}, rounded {rounded}: partial intrinsic coverage required: {fringe}"
                    );
                }
                // Corners exercise derivative quads; the odd hard clip also
                // checks that aligning the scratch origin cannot widen writes.
                for (pixel, (expected, actual)) in reference
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(cleared.as_chunks::<4>().0)
                    .enumerate()
                {
                    for channel in actual {
                        assert!(
                            channel.abs_diff(expected[0]) <= 2,
                            "family {family}, rounded {rounded}, clipped {clipped}, pixel ({}, {}): Clear {channel}, reference {}",
                            pixel % 16,
                            pixel / 16,
                            expected[0],
                        );
                    }
                    let (x, y) = (pixel % 16, pixel / 16);
                    if clipped && (!(3..12).contains(&x) || !(3..12).contains(&y)) {
                        assert_eq!(actual, &[255; 4], "outside hard clip at ({x}, {y})");
                    }
                }
            }
        }
    }
}

/// SSAA must clamp Plus at full strength before resolving geometric coverage.
fn ssaa_saturated_plus_clamps_before_coverage(
    renderer: &HeadlessRenderer,
    _portable: &HeadlessRenderer,
) {
    let render = |mode, source, destination| {
        let mut canvas = Canvas::new();
        canvas.draw_rect(
            Rect::from_xywh(0.0, 0.0, 64.0, 64.0),
            &Paint::fill(destination).with_anti_alias(false),
        );
        // Invisible AA paths must not prevent the following visible operation
        // or a subsequent frame, including paths exactly at the target edge.
        for (x, y) in [
            (80.0, 8.0),
            (8.0, 80.0),
            (64.0, 8.0),
            (8.0, 64.0),
            (-80.0, 8.0),
            (8.0, -80.0),
        ] {
            let mut outside = flui_painting::paint::Path::new();
            outside.add_rect(Rect::from_xywh(x, y, 32.0, 32.0));
            canvas.draw_path(
                &outside,
                &Paint::fill(source)
                    .with_anti_alias(true)
                    .with_blend_mode(mode),
            );
        }
        let mut path = flui_painting::paint::Path::new();
        path.add_rect(Rect::from_xywh(8.5, 8.0, 47.0, 48.0));
        canvas.draw_path(
            &path,
            &Paint::fill(source)
                .with_anti_alias(true)
                .with_blend_mode(mode),
        );
        let mut builder = SceneBuilder::new();
        builder.add_picture(canvas.finish());
        renderer
            .render_layer_tree(&builder.build(), (64, 64))
            .expect("SSAA Plus witness render")
    };
    let reference = render(BlendMode::SrcOver, Color::BLACK, Color::WHITE);
    let plus = render(
        BlendMode::Plus,
        Color::rgb(204, 204, 204),
        Color::rgb(204, 204, 204),
    );
    #[cfg(feature = "testing")]
    crate::readback_dump::dump_rgba_png("portable-plus-ssaa", 64, 64, &plus);
    let offset = (32 * 64 + 8) * 4;
    let coverage = 1.0 - f32::from(reference[offset]) / 255.0;
    assert!(
        (0.2..0.8).contains(&coverage),
        "SSAA edge must be fractional: {coverage}"
    );
    let actual = plus[offset..offset + 4].try_into().expect("RGBA pixel");
    assert_pixel(
        actual,
        [
            0.8 + 0.2 * coverage,
            0.8 + 0.2 * coverage,
            0.8 + 0.2 * coverage,
            1.0,
        ],
        "SSAA saturated Plus fringe",
    );
    for (x, expected) in [(2, [204, 204, 204, 255]), (32, [255; 4])] {
        let offset = (32 * 64 + x) * 4;
        assert_eq!(&plus[offset..offset + 4], &expected);
    }
}

/// Two cropped gradient operations keep their order around an ordinary draw.
fn cropped_gradient_clear_runs_preserve_barrier_and_order(
    _native: &HeadlessRenderer,
    renderer: &HeadlessRenderer,
) {
    let mut canvas = Canvas::new();
    canvas.draw_rect(
        Rect::from_xywh(0.0, 0.0, 32.0, 32.0),
        &Paint::fill(Color::RED).with_anti_alias(false),
    );
    let clear = Paint::fill(Color::WHITE)
        .with_blend_mode(BlendMode::Clear)
        .with_shader(Shader::LinearGradient {
            from: Offset::ZERO,
            to: Offset::new(32.0, 0.0),
            colors: vec![Color::rgba(0, 0, 0, 0), Color::rgba(0, 0, 0, 0)],
            stops: Some(vec![0.0, 1.0]),
            tile_mode: TileMode::Clamp,
        });
    let bounds = RRect::from_rect_circular(Rect::from_xywh(2.5, 2.0, 20.0, 28.0), 1.0);
    canvas.draw_rrect(bounds, &clear);
    canvas.draw_rect(
        Rect::from_xywh(16.0, 0.0, 16.0, 32.0),
        &Paint::fill(Color::GREEN).with_anti_alias(false),
    );
    canvas.draw_rrect(bounds, &clear);
    let mut builder = SceneBuilder::new();
    builder.add_picture(canvas.finish());
    let pixels = renderer
        .render_layer_tree(&builder.build(), (32, 32))
        .expect("cropped ordered gradients");
    let sample = |x: usize| -> [u8; 4] {
        pixels[(16 * 32 + x) * 4..(16 * 32 + x) * 4 + 4]
            .try_into()
            .expect("RGBA pixel")
    };
    assert_pixel(
        sample(2),
        [0.25, 0.0, 0.0, 0.25],
        "two independent half-coverage Clears",
    );
    assert_eq!(
        sample(18),
        [0; 4],
        "second gradient clears the intervening green draw"
    );
    assert_eq!(
        sample(26),
        [0, 255, 0, 255],
        "crop preserves green outside both gradients"
    );
}

/// Fragment clip sampling remains in attachment coordinates after a cropped isolation.
fn cropped_fragment_clip_origin_matches_attachment(
    _native: &HeadlessRenderer,
    portable: &HeadlessRenderer,
) {
    let color = Color::rgba(0, 0, 255, 128);
    let colors = vec![color, color];
    let stops = Some(vec![0.0, 1.0]);
    let shaders = [
        None,
        Some(Shader::LinearGradient {
            from: Offset::ZERO,
            to: Offset::new(32.0, 0.0),
            colors: colors.clone(),
            stops: stops.clone(),
            tile_mode: TileMode::Clamp,
        }),
        Some(Shader::RadialGradient {
            center: Offset::new(16.0, 16.0),
            radius: 32.0,
            colors: colors.clone(),
            stops: stops.clone(),
            tile_mode: TileMode::Clamp,
            focal: None,
            focal_radius: None,
        }),
        Some(Shader::SweepGradient {
            center: Offset::new(16.0, 16.0),
            colors,
            stops,
            tile_mode: TileMode::Clamp,
            start_angle: 0.0,
            end_angle: std::f64::consts::TAU,
        }),
    ];
    for (family, shader) in shaders.into_iter().enumerate() {
        let mut canvas = Canvas::new();
        canvas.draw_rect(
            Rect::from_xywh(0.0, 0.0, 32.0, 32.0),
            &Paint::fill(Color::WHITE).with_anti_alias(false),
        );
        canvas.clip_rect_ext(
            Rect::from_xywh(10.5, 8.5, 8.0, 8.0),
            ClipOp::Intersect,
            Clip::AntiAlias,
        );
        let mut paint = Paint::fill(color)
            .with_anti_alias(false)
            .with_blend_mode(BlendMode::Src);
        let bounds = if let Some(shader) = shader {
            paint = paint.with_shader(shader);
            Rect::from_xywh(9.0, 7.0, 12.0, 12.0)
        } else {
            Rect::from_xywh(10.0, 8.0, 10.0, 10.0)
        };
        canvas.draw_rect(bounds, &paint);
        let mut builder = SceneBuilder::new();
        builder.add_picture(canvas.finish());
        let pixels = portable
            .render_layer_tree(&builder.build(), (32, 32))
            .expect("cropped clip render");
        let sample = |x: usize| -> [u8; 4] {
            pixels[(12 * 32 + x) * 4..(12 * 32 + x) * 4 + 4]
                .try_into()
                .expect("RGBA pixel")
        };
        let alpha = 128.0 / 255.0;
        assert_pixel(
            sample(10),
            [0.5, 0.5, 0.5 + 0.5 * alpha, 0.5 + 0.5 * alpha],
            &format!("family {family} cropped fringe"),
        );
        assert_eq!(
            sample(13),
            [0, 0, 128, 128],
            "family {family} full coverage"
        );
        assert_eq!(sample(9), [255; 4], "family {family} excluded pixel");
    }
}

/// Translucent destination alpha discriminates destination-dependent factors.
fn portable_porter_duff_translucent_destination(
    _native: &HeadlessRenderer,
    portable: &HeadlessRenderer,
) {
    let destination = Color::rgba(200, 120, 60, 128);
    let source = Color::rgba(30, 220, 80, 128);
    let mut failures = Vec::new();
    for mode in [
        BlendMode::Clear,
        BlendMode::Src,
        BlendMode::SrcIn,
        BlendMode::SrcOut,
        BlendMode::DstIn,
        BlendMode::DstATop,
        BlendMode::Modulate,
    ] {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let full = Rect::from_xywh(0.0, 0.0, 8.0, 8.0);
            let mut canvas = Canvas::new();
            canvas.draw_rect(
                full,
                &Paint::fill(Color::rgba(0, 0, 0, 0))
                    .with_anti_alias(false)
                    .with_blend_mode(BlendMode::Clear),
            );
            canvas.draw_rect(full, &Paint::fill(destination).with_anti_alias(false));
            canvas.clip_rect_ext(
                Rect::from_xywh(2.5, 0.0, 5.5, 8.0),
                ClipOp::Intersect,
                Clip::AntiAlias,
            );
            canvas.draw_rect(
                full,
                &Paint::fill(source)
                    .with_anti_alias(false)
                    .with_blend_mode(mode),
            );
            let mut builder = SceneBuilder::new();
            builder.add_picture(canvas.finish());
            let pixels = portable
                .render_layer_tree(&builder.build(), (8, 8))
                .expect("translucent destination render");
            for (x, coverage) in [(0usize, 0.0), (2, 0.5), (4, 1.0)] {
                let offset = (4 * 8 + x) * 4;
                let actual = pixels[offset..offset + 4].try_into().expect("RGBA pixel");
                assert_pixel(
                    actual,
                    coverage_correct_for(mode, source, destination, coverage),
                    &format!("{mode:?} translucent destination C={coverage}"),
                );
            }
        }));
        if result.is_err() {
            failures.push(mode);
        }
    }
    assert!(
        failures.is_empty(),
        "translucent destination modes failed: {failures:?}"
    );
}

/// Coverage-blend contract, read back from the GPU: dst-out independence of the
/// second blend source, and feathered edges for modes that cannot absorb coverage.
#[test]
fn coverage_blend_reads_back_as_specified() {
    let Some(native) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let portable = pollster::block_on(native.without_dual_source_blending())
        .expect("adapter must answer with fewer features");
    type CoverageCase = fn(&HeadlessRenderer, &HeadlessRenderer);
    let cases: [(&str, CoverageCase); 9] = [
        (
            "translucent destination Porter-Duff",
            portable_porter_duff_translucent_destination,
        ),
        (
            "cropped fragment clip origin",
            cropped_fragment_clip_origin_matches_attachment,
        ),
        (
            "cropped gradient ordering",
            cropped_gradient_clear_runs_preserve_barrier_and_order,
        ),
        (
            "SSAA saturated Plus",
            ssaa_saturated_plus_clamps_before_coverage,
        ),
        (
            "intrinsic gradient transparent Clear",
            portable_intrinsic_gradient_clear_ignores_source_alpha,
        ),
        (
            "fractional Src with gradient alpha",
            portable_fractional_src_gradient_preserves_destination,
        ),
        (
            "transparent Clear, overlap and Plus",
            portable_transparent_clear_plus_and_overlap,
        ),
        (
            "DstOut independent of optional feature",
            dst_out_renders_the_same_with_and_without_a_second_blend_source,
        ),
        (
            "fractional Porter-Duff modes",
            modes_that_cannot_absorb_coverage_feather_their_partially_covered_edge,
        ),
    ];
    let mut failures = Vec::new();
    for (name, case) in cases {
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| case(&native, &portable)))
            .is_err()
        {
            failures.push(name);
        }
    }
    assert!(
        failures.is_empty(),
        "coverage readbacks failed: {failures:?}"
    );
}
