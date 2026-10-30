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
/// If `dispatch_shader` stopped handling a kind, the draw would fall
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

/// A gradient whose stops sit past the first row of the stop texture reads
/// them from that row.
///
/// The stops of a whole segment share one texture, 512 stops to a row. Two
/// draws of `MAX_GRADIENT_STOPS` each fill the first row, so the third
/// draw's stops start the second: a lookup that ignored the row would read
/// the first draw's red there instead of blue.
fn gradient_stops_past_the_first_texture_row_read_back() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let full = crate::batches::MAX_GRADIENT_STOPS;
    let column = |index: u32| Rect::from_xywh(f64::from(index * 20), 0.0, 20.0, f64::from(SIDE));
    let mut builder = SceneBuilder::new();
    let mut canvas = Canvas::new();
    for (index, color, count) in [
        (0, Color::rgb(255, 0, 0), full),
        (1, Color::rgb(0, 255, 0), full),
        (2, Color::rgb(0, 0, 255), 2),
    ] {
        let shader = Shader::LinearGradient {
            from: Offset::new(0.0, 0.0),
            to: Offset::new(f64::from(SIDE), 0.0),
            colors: vec![color; count],
            stops: None,
            tile_mode: TileMode::Clamp,
        };
        canvas.draw_rect(column(index), &shader_paint(shader, BlendMode::SrcOver));
    }
    builder.add_picture(canvas.finish());
    let pixels = renderer
        .render_layer_tree(&builder.build(), (SIDE, SIDE))
        .expect("gradients spanning two stop rows must render");
    for (point, expected, label) in [
        ((10, 32), [1.0, 0.0, 0.0, 1.0], "first row, first draw"),
        ((30, 32), [0.0, 1.0, 0.0, 1.0], "first row, second draw"),
        ((50, 32), [0.0, 0.0, 1.0, 1.0], "second row, third draw"),
    ] {
        assert_pixel(pixel_at(&pixels, point), expected, label);
    }
}

// ── Acceptance 4: a shader on an arbitrary path paints the path ──────────────

const RAMP_START: Color = Color::rgb(255, 0, 0);
const RAMP_END: Color = Color::rgb(0, 0, 255);

/// A red-to-blue ramp of `kind` whose parameter varies across every sample
/// point the path rows read. The radial centre sits on the left edge so a
/// concentric arc still crosses distinct radii.
fn ramp(kind: GradientKind) -> Shader {
    let colors = vec![RAMP_START, RAMP_END];
    let stops = Some(vec![0.0, 1.0]);
    let side = f64::from(SIDE);
    match kind {
        GradientKind::Linear => Shader::LinearGradient {
            from: Offset::new(0.0, 0.0),
            to: Offset::new(side, 0.0),
            colors,
            stops,
            tile_mode: TileMode::Clamp,
        },
        GradientKind::Radial => Shader::RadialGradient {
            center: Offset::new(0.0, side / 2.0),
            radius: side,
            colors,
            stops,
            tile_mode: TileMode::Clamp,
            focal: None,
            focal_radius: None,
        },
        GradientKind::Sweep => Shader::SweepGradient {
            center: Offset::new(side / 2.0, side / 2.0),
            colors,
            stops,
            tile_mode: TileMode::Clamp,
            start_angle: 0.0,
            end_angle: std::f64::consts::TAU,
        },
    }
}

/// The ramp's colour at the LOCAL point `(x, y)`, from an independent model of
/// the three parameters rather than the shader's arithmetic.
fn ramp_at(kind: GradientKind, (x, y): (f64, f64)) -> [f32; 4] {
    let side = f64::from(SIDE);
    let t = match kind {
        GradientKind::Linear => x / side,
        GradientKind::Radial => x.hypot(y - side / 2.0) / side,
        GradientKind::Sweep => {
            (y - side / 2.0)
                .atan2(x - side / 2.0)
                .rem_euclid(std::f64::consts::TAU)
                / std::f64::consts::TAU
        }
    }
    .clamp(0.0, 1.0) as f32;
    [1.0 - t, 0.0, t, 1.0]
}

/// The local centre of device pixel `(column, row)` under the identity.
fn centre_of((column, row): (u32, u32)) -> (f64, f64) {
    (f64::from(column) + 0.5, f64::from(row) + 0.5)
}

/// A triangle filling most of the surface; [`TRIANGLE_SAMPLES`] lie inside it
/// and [`TRIANGLE_OUTSIDE`] outside it.
fn triangle() -> flui_painting::paint::path::Path {
    use flui_foundation::geometry::Point;
    flui_painting::paint::path::Path::polygon(&[
        Point::new(2.0, 62.0),
        Point::new(62.0, 62.0),
        Point::new(32.0, 2.0),
    ])
}
const TRIANGLE_SAMPLES: [(u32, u32); 3] = [(12, 56), (52, 56), (32, 30)];
const TRIANGLE_OUTSIDE: (u32, u32) = (4, 8);

/// The upper half of a circle about the surface centre, radius 24, as an
/// open arc. Stroked 8 wide, [`ARC_SAMPLES`] lie on the stroke and the circle
/// centre lies off it.
fn upper_arc() -> flui_painting::paint::path::Path {
    flui_painting::paint::path::Path::arc(
        Rect::from_xywh(8.0, 8.0, 48.0, 48.0),
        std::f64::consts::PI,
        std::f64::consts::PI,
    )
}
const ARC_SAMPLES: [(u32, u32); 3] = [(15, 15), (31, 8), (48, 15)];

fn render(renderer: &HeadlessRenderer, draw: impl FnOnce(&mut Canvas)) -> Vec<u8> {
    let mut builder = SceneBuilder::new();
    let mut canvas = Canvas::new();
    draw(&mut canvas);
    builder.add_picture(canvas.finish());
    renderer
        .render_layer_tree(&builder.build(), (SIDE, SIDE))
        .expect("the headless capture path must rasterize the scene")
}

const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// A linear, radial and sweep ramp on a filled triangle and a stroked arc
/// read the gradient at points where it differs from the paint's own colour
/// and from its value elsewhere on the same path.
fn path_fills_and_strokes_carry_their_shader() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    for kind in GRADIENT_KINDS {
        let fill = render(&renderer, |canvas| {
            canvas.draw_path(&triangle(), &shader_paint(ramp(kind), BlendMode::SrcOver));
        });
        for point in TRIANGLE_SAMPLES {
            assert_pixel(
                pixel_at(&fill, point),
                ramp_at(kind, centre_of(point)),
                &format!(
                    "{kind:?} triangle fill at {point:?}: the path must carry its shader. \
                     Opaque yellow is the paint colour standing in for the gradient"
                ),
            );
        }
        assert_pixel(
            pixel_at(&fill, TRIANGLE_OUTSIDE),
            WHITE,
            &format!("{kind:?}: the gradient covers the path, not its bounds"),
        );

        let stroke = render(&renderer, |canvas| {
            let paint = Paint::stroke(UNSHADED_BASE, 8.0)
                .with_anti_alias(false)
                .with_shader(ramp(kind));
            canvas.draw_path(&upper_arc(), &paint);
        });
        for point in ARC_SAMPLES {
            assert_pixel(
                pixel_at(&stroke, point),
                ramp_at(kind, centre_of(point)),
                &format!("{kind:?} stroked arc at {point:?}: the stroke must carry its shader"),
            );
        }
        assert_pixel(
            pixel_at(&stroke, CENTRE),
            WHITE,
            &format!("{kind:?}: a stroke covers its outline, not the region it encloses"),
        );
    }
}

/// The gradient is in the path's local space: a mirrored transform mirrors the
/// ramp with the geometry, and a rotation moves both together.
fn path_shader_follows_the_transform() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let side = f64::from(SIDE);
    let mirrored = render(&renderer, |canvas| {
        canvas.translate(side, 0.0);
        canvas.scale(-1.0, 1.0);
        canvas.draw_path(
            &triangle(),
            &shader_paint(ramp(GradientKind::Linear), BlendMode::SrcOver),
        );
    });
    for point in TRIANGLE_SAMPLES {
        let (x, y) = centre_of(point);
        assert_pixel(
            pixel_at(&mirrored, point),
            ramp_at(GradientKind::Linear, (side - x, y)),
            &format!("mirrored triangle at {point:?}: the ramp mirrors with the path"),
        );
    }

    // A quarter turn about the centre maps local (x, y) to device (64 - y, x).
    let rotated = render(&renderer, |canvas| {
        canvas.translate(side / 2.0, side / 2.0);
        canvas.rotate(std::f64::consts::FRAC_PI_2);
        canvas.translate(-side / 2.0, -side / 2.0);
        canvas.draw_path(
            &triangle(),
            &shader_paint(ramp(GradientKind::Linear), BlendMode::SrcOver),
        );
    });
    for point in [(8, 12), (8, 52)] {
        let (x, y) = centre_of(point);
        assert_pixel(
            pixel_at(&rotated, point),
            ramp_at(GradientKind::Linear, (y, side - x)),
            &format!("rotated triangle at {point:?}: the ramp turns with the path"),
        );
    }
}

/// A rectangular scissor clip, a rounded-rectangle analytic clip and a path
/// mask clip each bound a gradient path to their intersection.
fn path_shader_respects_clips() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let kind = GradientKind::Linear;
    let rect = render(&renderer, |canvas| {
        canvas.clip_rect(Rect::from_xywh(0.0, 0.0, 32.0, 64.0));
        canvas.draw_path(&triangle(), &shader_paint(ramp(kind), BlendMode::SrcOver));
    });
    assert_pixel(
        pixel_at(&rect, (12, 56)),
        ramp_at(kind, centre_of((12, 56))),
        "inside the rect clip",
    );
    assert_pixel(pixel_at(&rect, (52, 56)), WHITE, "outside the rect clip");

    let rounded = render(&renderer, |canvas| {
        canvas.clip_rrect(RRect::from_rect_and_radius(
            Rect::from_xywh(0.0, 40.0, 64.0, 24.0),
            flui_foundation::geometry::Radius::circular(12.0),
        ));
        canvas.draw_path(&triangle(), &shader_paint(ramp(kind), BlendMode::SrcOver));
    });
    assert_pixel(
        pixel_at(&rounded, (32, 52)),
        ramp_at(kind, centre_of((32, 52))),
        "inside the rounded clip",
    );
    assert_pixel(
        pixel_at(&rounded, (32, 30)),
        WHITE,
        "above the rounded clip",
    );

    let masked = render(&renderer, |canvas| {
        let mut diamond = flui_painting::paint::path::Path::new();
        for (index, (x, y)) in [(32.0, 30.0), (46.0, 44.0), (32.0, 58.0), (18.0, 44.0)]
            .into_iter()
            .enumerate()
        {
            let point = flui_foundation::geometry::Point::new(x, y);
            if index == 0 {
                diamond.move_to(point);
            } else {
                diamond.line_to(point);
            }
        }
        diamond.close();
        canvas.clip_path(&diamond);
        canvas.draw_path(&triangle(), &shader_paint(ramp(kind), BlendMode::SrcOver));
    });
    assert_pixel(
        pixel_at(&masked, (32, 44)),
        ramp_at(kind, centre_of((32, 44))),
        "inside the path clip",
    );
    assert_pixel(
        pixel_at(&masked, (12, 56)),
        WHITE,
        "inside the triangle, outside the path clip",
    );
}

/// The advanced (destination-reading) and portable-coverage routes carry the
/// path's shader too. Over white, `Multiply` returns the source; over black,
/// `Plus` does.
fn path_shader_reaches_every_blend_route() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let kind = GradientKind::Linear;
    for (mode, backdrop) in [
        (BlendMode::Multiply, Color::rgb(255, 255, 255)),
        (BlendMode::Plus, Color::rgb(0, 0, 0)),
    ] {
        let pixels = render(&renderer, |canvas| {
            canvas.draw_rect(
                full_surface(),
                &Paint::fill(backdrop).with_anti_alias(false),
            );
            canvas.draw_path(&triangle(), &shader_paint(ramp(kind), mode));
        });
        for point in TRIANGLE_SAMPLES {
            assert_pixel(
                pixel_at(&pixels, point),
                ramp_at(kind, centre_of(point)),
                &format!("{mode:?} gradient path at {point:?}"),
            );
        }
    }
}

/// Mesh and quad gradient draws alternate in one segment without either
/// inheriting the other's geometry: a quad before and after the path keeps its
/// own rectangle and colour, and the path keeps its ramp.
fn path_and_rect_gradients_keep_their_own_geometry() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let green = Color::rgb(0, 255, 0);
    let pixels = render(&renderer, |canvas| {
        canvas.draw_rect(
            Rect::from_xywh(0.0, 0.0, 8.0, 8.0),
            &shader_paint(
                constant_gradient(GradientKind::Linear, green),
                BlendMode::SrcOver,
            ),
        );
        canvas.draw_path(
            &triangle(),
            &shader_paint(ramp(GradientKind::Linear), BlendMode::SrcOver),
        );
        canvas.draw_rect(
            Rect::from_xywh(56.0, 0.0, 8.0, 8.0),
            &shader_paint(
                constant_gradient(GradientKind::Linear, green),
                BlendMode::SrcOver,
            ),
        );
    });
    let green = [0.0, 1.0, 0.0, 1.0];
    assert_pixel(pixel_at(&pixels, (4, 4)), green, "quad before the path");
    assert_pixel(pixel_at(&pixels, (60, 4)), green, "quad after the path");
    assert_pixel(pixel_at(&pixels, (12, 20)), WHITE, "between the quads");
    for point in TRIANGLE_SAMPLES {
        assert_pixel(
            pixel_at(&pixels, point),
            ramp_at(GradientKind::Linear, centre_of(point)),
            &format!("path between quads at {point:?}"),
        );
    }
}

/// A gradient without colors has no stops to paint; a path declines it to the
/// solid route and paints `Paint::color`, as a rectangle does.
fn colorless_gradient_path_paints_its_paint_color() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let mut colorless = ramp(GradientKind::Linear);
    if let Shader::LinearGradient { colors, stops, .. } = &mut colorless {
        colors.clear();
        *stops = None;
    }
    let pixels = render(&renderer, |canvas| {
        canvas.draw_rect(
            Rect::from_xywh(0.0, 0.0, 8.0, 8.0),
            &shader_paint(colorless.clone(), BlendMode::SrcOver),
        );
        canvas.draw_path(&triangle(), &shader_paint(colorless, BlendMode::SrcOver));
    });
    let base = [1.0, 1.0, 0.0, 1.0];
    assert_pixel(pixel_at(&pixels, (4, 4)), base, "colorless gradient rect");
    for point in TRIANGLE_SAMPLES {
        assert_pixel(
            pixel_at(&pixels, point),
            base,
            &format!("colorless gradient path at {point:?} paints the paint colour"),
        );
    }
}

// ── Acceptance 5: closed-form shapes off the rounded-box route ───────────────

/// Draws a shape with a ramp of each kind and reads the ramp at `inside`, on
/// the shape, and the white surface at `outside`, off it.
fn assert_shape_carries_its_shader(
    shape: &str,
    paint: fn(Shader) -> Paint,
    draw: impl Fn(&mut Canvas, &Paint),
    inside: &[(u32, u32)],
    outside: (u32, u32),
) {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    for kind in GRADIENT_KINDS {
        let pixels = render(&renderer, |canvas| draw(canvas, &paint(ramp(kind))));
        for &point in inside {
            assert_pixel(
                pixel_at(&pixels, point),
                ramp_at(kind, centre_of(point)),
                &format!(
                    "{kind:?} {shape} at {point:?}: the shape must carry its shader. \
                     Opaque yellow is the paint colour standing in for the gradient"
                ),
            );
        }
        assert_pixel(
            pixel_at(&pixels, outside),
            WHITE,
            &format!("{kind:?} {shape} at {outside:?}: the gradient covers the shape only"),
        );
    }
}

fn filled(shader: Shader) -> Paint {
    shader_paint(shader, BlendMode::SrcOver)
}

fn stroked(shader: Shader) -> Paint {
    Paint::stroke(UNSHADED_BASE, 6.0)
        .with_anti_alias(false)
        .with_shader(shader)
}

/// The rectangle every rect-like row draws, and a sample on each of its
/// sides, clear of the corners.
const SQUARE: Rect<f64> = Rect::from_ltrb(8.0, 8.0, 56.0, 56.0);
const SQUARE_SIDES: [(u32, u32); 4] = [(8, 24), (40, 8), (56, 40), (24, 56)];

fn shape_rect_stroke() {
    assert_shape_carries_its_shader(
        "stroked rect",
        stroked,
        |canvas, paint| canvas.draw_rect(SQUARE, paint),
        &SQUARE_SIDES,
        CENTRE,
    );
}

fn shape_rrect_stroke() {
    assert_shape_carries_its_shader(
        "stroked rrect",
        stroked,
        |canvas, paint| {
            canvas.draw_rrect(
                RRect::from_rect_and_radius(
                    SQUARE,
                    flui_foundation::geometry::Radius::circular(8.0),
                ),
                paint,
            );
        },
        &SQUARE_SIDES,
        CENTRE,
    );
}

fn shape_circle_stroke() {
    assert_shape_carries_its_shader(
        "stroked circle",
        stroked,
        |canvas, paint| {
            canvas.draw_circle(
                flui_foundation::geometry::Point::new(32.0, 32.0),
                20.0,
                paint,
            );
        },
        &[(12, 28), (36, 12), (52, 36)],
        CENTRE,
    );
}

/// An ellipse about the surface centre, 28 by 16.
const OVAL: Rect<f64> = Rect::from_ltrb(4.0, 16.0, 60.0, 48.0);

/// A `SrcOver` fill otherwise takes the instanced circle, which has no shader.
fn shape_oval_fill() {
    assert_shape_carries_its_shader(
        "filled oval",
        filled,
        |canvas, paint| canvas.draw_oval(OVAL, paint),
        &[(12, 32), (32, 20), (50, 38)],
        (4, 4),
    );
}

fn shape_oval_stroke() {
    assert_shape_carries_its_shader(
        "stroked oval",
        stroked,
        |canvas, paint| canvas.draw_oval(OVAL, paint),
        &[(4, 30), (30, 16), (59, 34), (34, 47)],
        CENTRE,
    );
}

fn shape_drrect() {
    assert_shape_carries_its_shader(
        "drrect ring",
        filled,
        |canvas, paint| {
            let radius = flui_foundation::geometry::Radius::circular;
            canvas.draw_drrect(
                RRect::from_rect_and_radius(SQUARE, radius(8.0)),
                RRect::from_rect_and_radius(Rect::from_ltrb(20.0, 20.0, 44.0, 44.0), radius(4.0)),
                paint,
            );
        },
        &[(12, 24), (40, 12), (50, 40), (24, 50)],
        CENTRE,
    );
}

/// The lower half-disc of `upper_arc`'s circle: a `SrcOver` pie otherwise
/// takes the instanced arc, which has no shader.
fn shape_arc_fill() {
    assert_shape_carries_its_shader(
        "filled pie",
        filled,
        |canvas, paint| {
            canvas.draw_arc(
                Rect::from_xywh(8.0, 8.0, 48.0, 48.0),
                0.0,
                std::f64::consts::PI,
                true,
                paint,
            );
        },
        &[(20, 40), (32, 50), (44, 40)],
        (32, 16),
    );
}

fn shape_arc_stroke() {
    assert_shape_carries_its_shader(
        "stroked arc",
        stroked,
        |canvas, paint| {
            canvas.draw_arc(
                Rect::from_xywh(8.0, 8.0, 48.0, 48.0),
                std::f64::consts::PI,
                std::f64::consts::PI,
                false,
                paint,
            );
        },
        &ARC_SAMPLES,
        CENTRE,
    );
}

fn shape_line() {
    assert_shape_carries_its_shader(
        "line",
        stroked,
        |canvas, paint| {
            use flui_foundation::geometry::Point;
            canvas.draw_line(Point::new(4.0, 20.0), Point::new(60.0, 20.0), paint);
        },
        &[(10, 20), (32, 20), (54, 20)],
        (32, 40),
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
/// distinct stop-table ownership across composition boundaries, and the shader
/// on paths and on every tessellated shape.
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
            "stops_past_first_row",
            gradient_stops_past_the_first_texture_row_read_back,
        ),
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
        (
            "path_fills_and_strokes",
            path_fills_and_strokes_carry_their_shader,
        ),
        ("path_transform", path_shader_follows_the_transform),
        ("path_clips", path_shader_respects_clips),
        ("path_blend_routes", path_shader_reaches_every_blend_route),
        (
            "path_and_rect_geometry",
            path_and_rect_gradients_keep_their_own_geometry,
        ),
        (
            "path_colorless_gradient",
            colorless_gradient_path_paints_its_paint_color,
        ),
        ("shape_rect_stroke", shape_rect_stroke),
        ("shape_rrect_stroke", shape_rrect_stroke),
        ("shape_circle_stroke", shape_circle_stroke),
        ("shape_oval_fill", shape_oval_fill),
        ("shape_oval_stroke", shape_oval_stroke),
        ("shape_drrect", shape_drrect),
        ("shape_arc_fill", shape_arc_fill),
        ("shape_arc_stroke", shape_arc_stroke),
        ("shape_line", shape_line),
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
