//! Readback evidence that a gradient honours the blend mode its paint carries.
//!
//! Gradients are instanced, not tessellated, so they never reach
//! `add_tessellated_with_key` and the blend funnel that keys the shape path's
//! pipelines by mode. Every Porter-Duff mode a caller set used to fall through
//! to gradient pipelines built with a constant `ALPHA_BLENDING` — accepted,
//! carried on the paint, and dropped.
//!
//! The gradient is a CONSTANT colour in every oracle that asserts a blended
//! byte, so the value under test is the blender's, not `interpolateGradient`'s —
//! `blend_oracle`'s CPU model and production's own `blend_state_for` table
//! predict it. `each_gradient_kind_paints_through_its_own_pipeline` keeps the
//! constant colour honest: it fails if a solid fill quietly stood in.
//! Each mode's expected pixel is asserted to differ from `SrcOver`'s before it
//! is compared, so a pass cannot come from the two agreeing.

use flui_foundation::geometry::{Offset, RRect, Rect};
use flui_layer::SceneBuilder;
use flui_painting::{BlendMode, Canvas, Paint};
use flui_painting::{
    paint::{Shader, TileMode},
    styling::Color,
};

use crate::{
    blend_oracle::{PORTER_DUFF_MODES, SIDE, as_bytes, assert_pixel, blend, premultiplied},
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
    let centre = Offset::new(f64::from(SIDE) / 2.0, f64::from(SIDE) / 2.0);
    match kind {
        GradientKind::Linear => Shader::LinearGradient {
            from: Offset::new(0.0, 0.0),
            to: Offset::new(f64::from(SIDE), 0.0),
            colors,
            stops,
            tile_mode: TileMode::Clamp,
        },
        GradientKind::Radial => Shader::RadialGradient {
            center: centre,
            radius: f64::from(SIDE),
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
            end_angle: f64::from(std::f32::consts::TAU),
        },
    }
}

/// A gradient of `kind` whose every stop is `color`, so the fragment it
/// produces is `color` wherever it is sampled.
fn constant_gradient(kind: GradientKind, color: Color) -> Shader {
    gradient(kind, color, color)
}

/// The whole surface, in device pixels.
fn full_surface() -> Rect<f64> {
    Rect::from_xywh(0.0, 0.0, f64::from(SIDE), f64::from(SIDE))
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
    let tree = {
        let mut builder = SceneBuilder::new();
        let mut canvas = Canvas::new();
        canvas.draw_rect(full_surface(), &destination_paint());
        canvas.draw_rect(full_surface(), &shader_paint(shader, mode));
        builder.add_picture(canvas.finish());
        builder.build()
    };
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

// ── Acceptance 1: every fixed-function mode reaches the gradient ─────────────

/// The premise every constant-colour oracle rests on: each kind's own pipeline
/// really renders the gradient's stops.
///
/// If `dispatch_shader_rect` stopped handling a kind, the draw would fall
/// through to the solid-fill path and paint [`UNSHADED_BASE`] instead —
/// opaque yellow, which no prediction in this file resembles. If a kind's
/// pipeline were never built, the pixel would be the bare destination.
fn each_gradient_kind_paints_through_its_own_pipeline() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
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
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
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

fn a_linear_gradient_renders_every_porter_duff_mode() {
    every_porter_duff_mode_renders_as_itself(GradientKind::Linear);
}

// ── Acceptance 2: partial coverage feathers, or falls back and says so ───────

// ── Acceptance 3: SrcOver must not move ──────────────────────────────────────

// ── Acceptance 3: a rounded gradient keeps each corner's own radius ──────────

/// A gradient-filled `RRect` reaches the GPU as the same shader-rect dispatch
/// a colour fill takes, with the rounded rect's real `[tl, tr, br, bl]`.
///
/// The asymmetric fixture is what discriminates: the top-left corner is
/// square and the bottom-right is rounded by a third of the side. A pixel
/// just inside the top-left corner must carry the gradient; a pixel the same
/// distance inside the bottom-right corner lies outside the rounding and must
/// show the bare destination. A uniform radius — either corner's value applied
/// to both — fails one of the two.
fn gradient_rrect_keeps_per_corner_radii() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let side = SIDE as f32;
    let rrect = RRect::from_rect_and_corners(
        full_surface(),
        flui_foundation::geometry::Radius::ZERO,
        flui_foundation::geometry::Radius::ZERO,
        flui_foundation::geometry::Radius::circular(f64::from(side / 3.0)),
        flui_foundation::geometry::Radius::ZERO,
    );
    let tree = {
        let mut builder = SceneBuilder::new();
        let mut canvas = Canvas::new();
        canvas.draw_rect(full_surface(), &destination_paint());
        canvas.draw_rrect(
            rrect,
            &shader_paint(
                constant_gradient(GradientKind::Linear, SOURCE),
                BlendMode::SrcOver,
            ),
        );
        builder.add_picture(canvas.finish());
        builder.build()
    };
    let pixels = renderer
        .render_layer_tree(&tree, (SIDE, SIDE))
        .expect("the headless capture path must rasterize the scene");

    let covered = blend(
        BlendMode::SrcOver,
        premultiplied(SOURCE, 1.0),
        premultiplied(DESTINATION, 1.0),
    );
    let bare = premultiplied(DESTINATION, 1.0);
    assert_pixel(
        pixel_at(&pixels, (1, 1)),
        covered,
        "the square top-left corner is inside the shape: the gradient must cover it",
    );
    assert_pixel(
        pixel_at(&pixels, (SIDE - 2, SIDE - 2)),
        bare,
        "the rounded bottom-right corner is outside the shape: only the destination \
         may show. Gradient colour here means the bottom-right radius was dropped \
         (a uniform radius taken from another corner)",
    );
}

fn gradient_and_solid_follow_painter_order(gradient_first: bool) {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    {
        let mut builder = SceneBuilder::new();
        let mut canvas = Canvas::new();
        let red = shader_paint(
            constant_gradient(GradientKind::Linear, Color::rgb(255, 0, 0)),
            BlendMode::SrcOver,
        );
        let blue = Paint::fill(Color::rgb(0, 0, 255)).with_anti_alias(false);
        let red_bounds = Rect::from_xywh(0.0, 0.0, 48.0, 48.0);
        let blue_bounds = Rect::from_xywh(16.0, 16.0, 48.0, 48.0);
        if gradient_first {
            canvas.draw_rect(red_bounds, &red);
            canvas.draw_rect(blue_bounds, &blue);
        } else {
            canvas.draw_rect(blue_bounds, &blue);
            canvas.draw_rect(red_bounds, &red);
        }
        builder.add_picture(canvas.finish());
        let pixels = renderer
            .render_layer_tree(&builder.build(), (SIDE, SIDE))
            .expect("ordered gradient and solid scene must render");
        assert_pixel(
            pixel_at(&pixels, (8, 8)),
            [1.0, 0.0, 0.0, 1.0],
            "red-only region",
        );
        assert_pixel(
            pixel_at(&pixels, (56, 56)),
            [0.0, 0.0, 1.0, 1.0],
            "blue-only region",
        );
        let expected = if gradient_first {
            [0.0, 0.0, 1.0, 1.0]
        } else {
            [1.0, 0.0, 0.0, 1.0]
        };
        assert_pixel(
            pixel_at(&pixels, (24, 24)),
            expected,
            &format!("overlap follows recorded order; gradient_first={gradient_first}"),
        );
    }
}

fn gradient_before_solid() {
    gradient_and_solid_follow_painter_order(true);
}

fn solid_before_gradient() {
    gradient_and_solid_follow_painter_order(false);
}

fn distinct_gradient_tables_survive_opacity_boundary() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let mut builder = SceneBuilder::new();
    let mut left = Canvas::new();
    left.draw_rect(
        Rect::from_xywh(0.0, 0.0, 24.0, 64.0),
        &shader_paint(
            constant_gradient(GradientKind::Linear, Color::rgb(255, 0, 0)),
            BlendMode::SrcOver,
        ),
    );
    builder.add_picture(left.finish());
    builder.push_opacity(0.5);
    let mut right = Canvas::new();
    right.draw_rect(
        Rect::from_xywh(40.0, 0.0, 24.0, 64.0),
        &shader_paint(
            constant_gradient(GradientKind::Linear, Color::rgb(0, 0, 255)),
            BlendMode::SrcOver,
        ),
    );
    builder.add_picture(right.finish());
    builder.pop();
    let pixels = renderer
        .render_layer_tree(&builder.build(), (SIDE, SIDE))
        .expect("distinct gradient tables across opacity must render");
    assert_pixel(
        pixel_at(&pixels, (8, 32)),
        [1.0, 0.0, 0.0, 1.0],
        "left table remains red after later blue table upload",
    );
    assert_pixel(
        pixel_at(&pixels, (56, 32)),
        [0.5, 0.5, 1.0, 1.0],
        "half-opacity blue over the white capture background",
    );
    assert_pixel(
        pixel_at(&pixels, (32, 32)),
        [1.0, 1.0, 1.0, 1.0],
        "gap stays white",
    );
}

fn repeated_gradient_kinds_keep_distinct_tables_and_order() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let mut builder = SceneBuilder::new();
    let mut canvas = Canvas::new();
    for (kind, color, bounds) in [
        (GradientKind::Linear, Color::rgb(255, 0, 0), full_surface()),
        (
            GradientKind::Radial,
            Color::rgb(0, 255, 0),
            Rect::from_xywh(16.0, 16.0, 48.0, 48.0),
        ),
        (
            GradientKind::Linear,
            Color::rgb(0, 0, 255),
            Rect::from_xywh(32.0, 32.0, 32.0, 32.0),
        ),
    ] {
        canvas.draw_rect(
            bounds,
            &shader_paint(constant_gradient(kind, color), BlendMode::SrcOver),
        );
    }
    builder.add_picture(canvas.finish());
    let pixels = renderer
        .render_layer_tree(&builder.build(), (SIDE, SIDE))
        .expect("repeated gradient kinds must render in recorded order");
    for (point, expected, label) in [
        ((8, 8), [1.0, 0.0, 0.0, 1.0], "first linear only"),
        ((24, 24), [0.0, 1.0, 0.0, 1.0], "radial over first linear"),
        ((48, 48), [0.0, 0.0, 1.0, 1.0], "last linear over radial"),
    ] {
        assert_pixel(pixel_at(&pixels, point), expected, label);
    }
}

fn ninth_gradient_stop_is_not_silently_clamped() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let mut colors = vec![Color::rgb(255, 0, 0); 8];
    colors.push(Color::rgb(0, 0, 255));
    let shader = Shader::LinearGradient {
        from: Offset::new(0.0, 0.0),
        to: Offset::new(f64::from(SIDE), 0.0),
        colors,
        stops: Some(vec![0.0, 0.05, 0.1, 0.15, 0.2, 0.25, 0.3, 0.35, 0.5]),
        tile_mode: TileMode::Clamp,
    };
    let pixels = render_gradient_over_destination(&renderer, shader, BlendMode::SrcOver);
    assert_pixel(
        pixel_at(&pixels, (48, 32)),
        [0.0, 0.0, 1.0, 1.0],
        "past ninth stop must clamp to blue, not the eighth red stop",
    );
    assert_pixel(
        pixel_at(&pixels, (8, 32)),
        [1.0, 0.0, 0.0, 1.0],
        "early stops stay red",
    );
}

/// Runs every contract row even after an ordinary assertion panic.
fn run_gradient_cases(cases: &[(&str, fn())]) {
    let mut failed = Vec::new();
    for &(name, case) in cases {
        if std::panic::catch_unwind(case).is_err() {
            failed.push(name);
        }
    }
    assert!(
        failed.is_empty(),
        "failed gradient contract rows: {failed:?}"
    );
}

/// Gradient contract, read back from the GPU: every Porter-Duff mode, each
/// gradient kind through its own pipeline, per-corner rrect radii, painter order,
/// and distinct stop-table ownership across composition boundaries.
#[test]
fn gradients_read_back_as_specified() {
    run_gradient_cases(&[
        #[cfg(feature = "testing")]
        (
            "large_stop_refusal_recovery",
            excessive_gradient_work_refuses_then_next_frame_renders,
        ),
        ("ninth_stop", ninth_gradient_stop_is_not_silently_clamped),
        (
            "porter_duff_modes",
            a_linear_gradient_renders_every_porter_duff_mode,
        ),
        (
            "gradient_pipeline_kinds",
            each_gradient_kind_paints_through_its_own_pipeline,
        ),
        ("per_corner_radii", gradient_rrect_keeps_per_corner_radii),
        ("gradient_before_solid", gradient_before_solid),
        ("solid_before_gradient", solid_before_gradient),
        (
            "distinct_tables_opacity",
            distinct_gradient_tables_survive_opacity_boundary,
        ),
        (
            "linear_radial_linear",
            repeated_gradient_kinds_keep_distinct_tables_and_order,
        ),
    ]);
}

#[cfg(feature = "testing")]
fn excessive_gradient_work_refuses_then_next_frame_renders() {
    let (device, queue) = crate::test_support::test_device_and_queue("gradient work limit");
    let (target, view) = crate::test_support::create_target(
        &device,
        "bounded gradient work",
        SIDE,
        SIDE,
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    crate::test_support::clear_target(&device, &queue, &view, wgpu::Color::BLACK);
    let mut painter = crate::WgpuPainter::with_shared_device(
        std::sync::Arc::clone(&device),
        std::sync::Arc::clone(&queue),
        wgpu::TextureFormat::Rgba8Unorm,
        (SIDE, SIDE),
    );
    for kind in GRADIENT_KINDS {
        painter
            .begin_frame()
            .expect("bounded gradient frame begins");
        let mut shader = constant_gradient(kind, Color::rgb(255, 0, 0));
        match &mut shader {
            Shader::LinearGradient { colors, stops, .. }
            | Shader::RadialGradient { colors, stops, .. }
            | Shader::SweepGradient { colors, stops, .. } => {
                *colors = vec![Color::rgb(255, 0, 0); 100_000];
                *stops = None;
            }
            _ => unreachable!("gradient fixture always supplies a gradient"),
        }
        painter.draw_rect(full_surface(), &shader_paint(shader, BlendMode::SrcOver));
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        assert!(
            matches!(
                painter.render_to_view(&view, &mut encoder),
                Err(crate::EngineError::PreparedResourceLimit {
                    resource: "gradient stops per draw",
                    requested: 100_000,
                    ..
                })
            ),
            "{kind:?} must reject costly per-fragment work before submission"
        );
        drop(encoder);
        painter.finish_frame();
        let unchanged = crate::test_support::readback_bytes(&device, &queue, &target, SIDE, SIDE);
        assert_eq!(
            pixel_at(&unchanged, CENTRE),
            [0, 0, 0, 255],
            "refused gradient leaves target untouched"
        );
    }
    painter
        .begin_frame()
        .expect("valid gradient frame begins after refusals");
    painter.draw_rect(
        full_surface(),
        &shader_paint(
            constant_gradient(GradientKind::Linear, Color::rgb(0, 255, 0)),
            BlendMode::SrcOver,
        ),
    );
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    painter
        .render_to_view(&view, &mut encoder)
        .expect("next valid gradient encodes");
    painter
        .submit_encoder(encoder)
        .expect("next valid gradient submits");
    painter.finish_frame();
    let pixels = crate::test_support::readback_bytes(&device, &queue, &target, SIDE, SIDE);
    assert_eq!(
        pixel_at(&pixels, CENTRE),
        [0, 255, 0, 255],
        "valid frame progresses after gradient work refusal"
    );
}
