//! `BoxDecoration` painting — the orchestrating painter the canvas
//! primitives were waiting for.
//!
//! Flutter's `_BoxDecorationPainter` (box_decoration.dart) draws, in
//! order: shadows → background (color/gradient) → image → border. All
//! the primitives (rect/rrect/drrect/gradient/shadow/image) already
//! exist on [`Canvas`]; this module sequences them and resolves the
//! alignment-relative gradient geometry against the concrete paint
//! rect.
//!
//! Everything here is sans-IO: commands are recorded into the canvas's
//! display list, never rasterized — the same contract as the rest of
//! the fragment paint model.

use std::sync::Once;

use flui_types::{
    Color, Offset, Pixels, Point, RRect, Rect,
    geometry::Circle,
    painting::{Paint, Path, Shader},
    styling::{BoxDecoration, BoxShadow, Gradient},
};

use crate::canvas::Canvas;

/// One-shot gate for the "`border_radius` ignored on `BoxShape::Circle`"
/// warning (see `resolve_silhouette` below).
///
/// `resolve_silhouette` runs from `paint_box_decoration` on every frame AND
/// from `box_decoration_hit_test` on every pointer event, so an ungated
/// `tracing::warn!` here would spam at frame-and-input rate for any
/// decoration that keeps a mismatched `border_radius` set across rebuilds
/// (e.g. an `AnimatedContainer` interpolating other fields). This is a
/// static-misconfiguration notice, not a per-event diagnostic — Flutter's
/// own `debugAssertIsValid` only ever fires once, at construction — so a
/// process-lifetime `Once` gate is the right frequency, matching the
/// precedent at `flui_rendering::delegates::custom_painter::WARN_ONCE`.
static WARN_CIRCLE_BORDER_RADIUS: Once = Once::new();

/// One-shot gate for the "non-uniform border on a circle is unimplemented"
/// warning (see `paint_circle_border` below) — same per-frame-spam
/// rationale as [`WARN_CIRCLE_BORDER_RADIUS`].
static WARN_CIRCLE_NON_UNIFORM_BORDER: Once = Once::new();

/// The decoration's resolved silhouette: the one shape used for the
/// background fill, the shadow cast, the border, hit testing, and — on a
/// circle — the image clip.
///
/// **The image is the partial case.** A circular decoration clips its image
/// to the circle; a rounded-rect one still paints its image to the full rect,
/// where Flutter would clip it too. That difference is deliberate and named
/// at the image step of `paint_box_decoration`, not an oversight of this
/// type.
///
/// Resolved once per paint/hit-test call (`resolve_silhouette`) rather
/// than re-derived at each paint site. Resolving once is what keeps
/// those sites from disagreeing about the shape, and it also means the
/// `border_radius`-on-circle conflict is detected in one place rather
/// than at every site that would have re-derived it (the warning itself
/// fires at most once per process — see [`WARN_CIRCLE_BORDER_RADIUS`]).
enum Silhouette {
    /// `BoxShape::Rectangle`, no border radius. Carries no payload: the
    /// plain paint rect is already in scope as the outer `rect`
    /// parameter at every call site that matches on this variant.
    Rect,
    /// `BoxShape::Rectangle` with a border radius.
    RRect(RRect),
    /// `BoxShape::Circle`, inscribed in the rect's shorter side.
    Circle(Circle<Pixels>),
}

/// Resolves `decoration`'s silhouette against `rect`.
///
/// `BoxShape::Circle` wins over `border_radius` unconditionally —
/// Flutter treats the combination as invalid
/// (`box_decoration.dart:134-137`'s `debugAssertIsValid`, a debug-only
/// assert). A `debug_assert!` here would make "the circle wins in
/// release" an untestable claim in every build profile that runs with
/// assertions on (this crate's own test suite included), so this warns
/// via `tracing` instead — testable in every profile — and paints the
/// circle regardless. The warn fires at most once per process
/// ([`WARN_CIRCLE_BORDER_RADIUS`]): this function is called from both
/// `paint_box_decoration` (every frame) and `box_decoration_hit_test`
/// (every pointer event), and an ungated warn would spam at that rate.
fn resolve_silhouette(rect: Rect<Pixels>, decoration: &BoxDecoration<Pixels>) -> Silhouette {
    if decoration.shape.is_circle() {
        if decoration.border_radius.is_some() {
            WARN_CIRCLE_BORDER_RADIUS.call_once(|| {
                tracing::warn!(
                    "BoxDecoration: `border_radius` is ignored when `shape` is \
                     `BoxShape::Circle`; the circle wins (this warn fires once \
                     per process)"
                );
            });
        }
        let radius = rect.shortest_side() / 2.0;
        return Silhouette::Circle(Circle::new(rect.center(), radius));
    }
    match decoration_rrect(rect, decoration) {
        Some(rrect) => Silhouette::RRect(rrect),
        None => Silhouette::Rect,
    }
}

/// How a decoration is rasterized, as opposed to what it depicts.
///
/// Separate from [`BoxDecoration`] on purpose: that type describes an
/// appearance and is serializable, while this is a rendering-quality hint the
/// render object owns — the same split Flutter makes by putting
/// `isAntiAlias` on `_RenderColoredBox` rather than on any decoration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct DecorationPaintOptions {
    /// Whether the decoration's solid-colour background is anti-aliased.
    ///
    /// `true` by default, matching Flutter's `Paint.isAntiAlias`. Turning it
    /// off is for a box whose edges are already pixel-aligned, where the
    /// feathered edge is a blur rather than a smoothing.
    ///
    /// **Reaches the colour fill only.** Borders, shadows and images keep the
    /// default because the reference says nothing about them here; gradient
    /// backgrounds keep it because `DrawGradient`/`DrawGradientRRect` carry a
    /// shader and no `Paint`, leaving nowhere to put the flag. Both limits are
    /// stated at the call site in `paint_box_decoration`.
    pub anti_alias: bool,
}

impl Default for DecorationPaintOptions {
    fn default() -> Self {
        Self { anti_alias: true }
    }
}

impl DecorationPaintOptions {
    /// Options with `anti_alias` set to `value`.
    #[must_use]
    pub const fn with_anti_alias(value: bool) -> Self {
        Self { anti_alias: value }
    }
}

/// Paints `decoration` into `rect` on `canvas` in Flutter's order:
/// shadows, then background color/gradient, then the decoration image,
/// then the border.
pub fn paint_box_decoration(
    canvas: &mut Canvas,
    rect: Rect<Pixels>,
    decoration: &BoxDecoration<Pixels>,
    options: DecorationPaintOptions,
) {
    let silhouette = resolve_silhouette(rect, decoration);

    // 1. Shadows (behind everything). Inset shadows are an inner-glow
    //    effect drawn INSIDE the shape above the background — a
    //    different compositing path the engine does not expose yet;
    //    painting them as outer drop shadows would be visually wrong,
    //    so they are skipped loudly rather than rendered wrongly.
    if let Some(shadows) = &decoration.box_shadow {
        for shadow in shadows {
            if shadow.inset {
                tracing::warn!(?shadow.color, "inset box shadows are not painted yet");
                continue;
            }
            match &silhouette {
                Silhouette::Circle(circle) => paint_circle_shadow(canvas, *circle, shadow),
                Silhouette::RRect(rrect) => paint_shadow(canvas, rect, Some(*rrect), shadow),
                Silhouette::Rect => paint_shadow(canvas, rect, None, shadow),
            }
        }
    }

    // 2. Background: a gradient wins over a flat color (Flutter:
    //    "if gradient is specified, color has no effect").
    //
    // A background covering no area is not recorded at all. Flutter guards
    // this in `_RenderColoredBox.paint` (`size > Size.zero`, false when
    // EITHER dimension is zero) but not in `RenderDecoratedBox`, because
    // there the guard lives on the class rather than on the primitive. FLUI
    // has one decoration painter serving both `ColoredBox` and
    // `DecoratedBox`, so the guard goes on the FILL instead of the caller:
    // that matches `ColoredBox` exactly, and for `DecoratedBox` it drops a
    // command that covers zero pixels either way. The border, shadows and
    // image below are deliberately outside it — a border on a degenerate box
    // still draws lines, and skipping the whole decoration would lose them.
    //
    // The test is `== 0`, not `<= 0`. A rect whose min exceeds its max is
    // INVERTED, not empty, and this module deliberately normalizes those
    // through `shortest_side`'s `.abs()` — `circle_zero_size_and_negative_area_rects_do_not_panic`
    // pins an inverted 100x100 resolving to the same r=50 circle an upright
    // one gives. A signed test would have swallowed that whole case.
    let paints_no_area = rect.width() == Pixels::ZERO || rect.height() == Pixels::ZERO;
    if paints_no_area {
        // fall through to the border/shadow/image passes
    } else if let Some(gradient) = &decoration.gradient {
        let shader = resolve_gradient(gradient, rect);
        match &silhouette {
            Silhouette::Circle(circle) => {
                // No dedicated `DrawGradientCircle` command exists (unlike
                // `DrawGradientRRect`); route through `draw_circle`'s
                // existing shader-paint dispatch instead, which the wgpu
                // backend already renders as an exact circle (`circle()`'s
                // `paint.has_shader()` branch calls `dispatch_shader_rect`
                // with `[radius; 4]` corners — an exact circle through the
                // same `sdRoundedBox` identity `DrawGradientRRect` uses).
                // The fill color is unreachable except as the fallback for
                // a stopless shader (`dispatch_shader_rect` returning
                // `false` when the gradient has no color stops —
                // `Gradient::new` does not validate that, so this is
                // reachable from safe input). `Color::TRANSPARENT` keeps
                // that fallback consistent with the rect/rrect gradient
                // paths: `render_gradient`/`render_gradient_rrect`
                // early-return and paint NOTHING on an empty color list
                // (`wgpu/backend.rs`); a solid fallback color here would
                // make the circle the only gradient-silhouette that paints
                // something visible from a stopless shader.
                // Deliberately NOT `options.anti_alias`: see the scope note
                // on the colour arm below. This silhouette could carry it —
                // it goes through a `Paint` — but its rect and rrect
                // neighbours cannot, and one gradient shape smoothing
                // differently from the others is worse than a limitation
                // that holds uniformly.
                let paint = Paint::fill(Color::TRANSPARENT).with_shader(shader);
                canvas.draw_circle(circle.center, circle.radius, &paint);
            }
            Silhouette::RRect(rrect) => canvas.draw_gradient_rrect(*rrect, shader),
            Silhouette::Rect => canvas.draw_gradient(rect, shader),
        }
    } else if let Some(color) = decoration.color {
        // `options.anti_alias` reaches THIS arm — the solid colour fill — and
        // nothing else.
        //
        // Not the border, shadow or image: Flutter's `isAntiAlias` is a
        // `ColoredBox` parameter and a `ColoredBox` has none of those, so
        // nothing in the reference says what they should do when it is off.
        //
        // Not the gradients either, and that one is a capability limit rather
        // than a choice: `DrawGradient`/`DrawGradientRRect` carry a shader and
        // no `Paint`, so there is nowhere to put the flag without widening the
        // closed `DrawCommand` enum that is the trust boundary with the wgpu
        // backend. A gradient-filled `ColoredBox` does not exist — the widget
        // is colour-only — so the only reachable case is a `DecoratedBox`
        // gradient, where the reference has no anti-alias knob at all.
        let paint = Paint::fill(color).with_anti_alias(options.anti_alias);
        match &silhouette {
            Silhouette::Circle(circle) => canvas.draw_circle(circle.center, circle.radius, &paint),
            Silhouette::RRect(rrect) => canvas.draw_rrect(*rrect, &paint),
            Silhouette::Rect => canvas.draw_rect(rect, &paint),
        }
    }

    // 3. Decoration image (above the background, below the border).
    //
    // Flutter clips the image to the decoration's shape
    // (`box_decoration.dart` `_paintBackgroundImage`). On a circle this
    // routes through a scoped SDF clip: `save` opens a scope the backend can
    // unwind, `clip_rrect` with radii at half the shorter side narrows it to
    // the circle, and `restore` closes it so the border painted afterwards is
    // untouched.
    //
    // The clip is a distance field, not tessellated geometry, so the circle
    // is exact — the ~6% diagonal bulge that rules out drrect for a circular
    // *ring* does not apply to a clip.
    if let Some(image) = &decoration.image {
        match &silhouette {
            Silhouette::Circle(circle) => {
                let diameter = circle.radius * 2.0;
                let bounds = Rect::from_xywh(
                    circle.center.x - circle.radius,
                    circle.center.y - circle.radius,
                    diameter,
                    diameter,
                );
                canvas.save();
                canvas.clip_rrect(RRect::from_rect_circular(bounds, circle.radius));
                paint_decoration_image(canvas, rect, image);
                canvas.restore();
            }
            // A rounded-rect decoration still paints its image to the full
            // rect. Flutter clips that case too; closing it is the same
            // mechanism as above, but it changes what existing rounded
            // decorations render, and no readback oracle covers a clipped
            // image yet — so it is named here rather than changed blind.
            Silhouette::RRect(_) | Silhouette::Rect => {
                paint_decoration_image(canvas, rect, image);
            }
        }
    }

    // 4. Border (on top).
    if let Some(border) = &decoration.border {
        match &silhouette {
            Silhouette::Circle(circle) => paint_circle_border(canvas, *circle, border),
            Silhouette::RRect(rrect) => paint_border(canvas, rect, Some(*rrect), border),
            Silhouette::Rect => paint_border(canvas, rect, None, border),
        }
    }
}

/// Hit test against the decoration's geometry: inside the circle when
/// `shape` is `BoxShape::Circle`, inside the rounded rect when a
/// border radius is set, inside the plain rect otherwise (Flutter
/// `BoxDecoration.hitTest`).
#[must_use]
pub fn box_decoration_hit_test(
    rect: Rect<Pixels>,
    decoration: &BoxDecoration<Pixels>,
    position: Offset<Pixels>,
) -> bool {
    let point = Point::new(position.dx, position.dy);
    if !rect.contains(point) {
        return false;
    }
    match resolve_silhouette(rect, decoration) {
        Silhouette::Circle(circle) => circle.contains(point),
        Silhouette::RRect(rrect) => rrect_contains(&rrect, point),
        Silhouette::Rect => true,
    }
}

/// The decoration's rounded rect, when a border radius is set.
fn decoration_rrect(rect: Rect<Pixels>, decoration: &BoxDecoration<Pixels>) -> Option<RRect> {
    decoration.border_radius.map(|radius| {
        RRect::from_rect_and_corners(
            rect,
            radius.top_left,
            radius.top_right,
            radius.bottom_right,
            radius.bottom_left,
        )
    })
}

/// The circle-shape shadow silhouette: the fill circle's radius
/// inflated by the spread radius and its center displaced by the
/// shadow offset — Flutter re-derives `rect.shortestSide / 2` from
/// `rect.shift(offset).inflate(spread)` (`box_decoration.dart:448-462`
/// `_paintShadows` -> `_paintBox`), which is equivalent to inflating
/// the radius directly for a square rect. `Circle::inflate` clamps the
/// radius at 0, so a spread large enough to invert the circle degrades
/// to a zero-radius point rather than panicking in `Canvas::draw_shadow`.
///
/// **Documented divergence from Flutter:** for a spread radius large
/// enough to invert the rect (e.g. a 100×100 rect with
/// `spread_radius = -1000`), Flutter's re-derivation inflates the RECT
/// first — `rect.inflate(-1000)` yields `LTRB(1000, 1000, -900, -900)` —
/// and only then takes `shortestSide / 2` of that inverted rect, landing
/// on radius **950** (`math.min((-900 - 1000).abs(), (-900 - 1000).abs())
/// / 2`). FLUI instead clamps `Circle::inflate`'s radius at 0, yielding
/// radius **0** for the same input — a deliberate choice, not a missed
/// case: Flutter's 950-radius shadow for a -1000 spread on a 100px box is
/// arguably the more surprising number, and FLUI's own rect/rrect shadow
/// path (`paint_shadow` below) does not clamp its `RRect::inflate` at all,
/// so the two silhouettes already disagree on this edge case independent
/// of this function. Clamping the circle path does not introduce a new
/// inconsistency; it just avoids a shadow radius nearly an order of
/// magnitude larger than the box it decorates.
fn paint_circle_shadow(canvas: &mut Canvas, circle: Circle<Pixels>, shadow: &BoxShadow<Pixels>) {
    let silhouette = circle
        .inflate(shadow.spread_radius)
        .translate(shadow.offset.into());
    canvas.draw_shadow(
        &Path::circle(silhouette.center, silhouette.radius.get()),
        shadow.color,
        shadow.blur_radius.get(),
    );
}

/// Paints a uniform border on a circle as a STROKED circle (Flutter
/// `box_border.dart:346-350` `_paintUniformBorderWithCircle`,
/// `drawCircle(center, (shortestSide + strokeOffset) / 2, side.toPaint())`),
/// NOT `draw_drrect` on an inscribed rounded rect: `tessellate_drrect`
/// approximates each 90-degree corner with a single quadratic Bezier,
/// which bulges the ring outward by about 6% on the diagonals relative
/// to `draw_circle`'s exact SDF fill, so the two shapes would visibly
/// disagree.
///
/// The stroke is centered so its OUTER edge lands exactly on the fill
/// radius, matching the inside-stroke convention `paint_border` above
/// already uses for the rect/rrect path (a filled outer/inner pair via
/// `draw_drrect`) rather than Flutter's `strokeAlign`, which FLUI's
/// `BorderSide` does not thread through this call. A width *exactly* at
/// the diameter still satisfies that invariant — the stroke centers at
/// radius 0 and spans `±radius`, so it paints a full disc in the border
/// color. Only a width that *exceeds* the diameter is unsatisfiable
/// (it would need a negative stroke-center radius), and that case is
/// skipped rather than clamped, since clamping paints a disc **outside**
/// the fill instead of a ring inside it (see the guard below).
///
/// A non-uniform border on a circle is UNIMPLEMENTED, and the blocker is
/// a missing primitive rather than missing work here.
///
/// Flutter paints a subset of these: `Border.paint` accepts a
/// non-uniform border on a circle when exactly one distinct *visible*
/// colour is present and no side is a hairline, and routes it to
/// `BoxBorder.paintNonUniformBorder` (`box_border.dart`, tag `3.44.0`).
/// That function does not walk the sides as arcs — it builds an `RRect`
/// from the circle's bounding rect, deflates and inflates it by each
/// side's stroke inset/outset, and emits a single `drawDRRect`. So a
/// border visible on one side only comes out as a crescent of varying
/// thickness, not as a constant-width arc.
///
/// Reproducing that here would mean a `draw_drrect` whose corners are a
/// true circle. FLUI's tessellates each 90° corner with one quadratic
/// Bézier, which bulges roughly 6% on the diagonals — the same reason
/// the uniform path above strokes a circle instead of inscribing a
/// rounded rect. Emitting it anyway would put a visibly non-circular
/// ring around a circular fill, so this warns (at most once per process,
/// [`WARN_CIRCLE_NON_UNIFORM_BORDER`]) and paints nothing. Falling
/// through to the per-side strip painter below is not an option either:
/// that draws a square frame around a circular fill.
fn paint_circle_border(
    canvas: &mut Canvas,
    circle: Circle<Pixels>,
    border: &flui_types::styling::Border<Pixels>,
) {
    if !border.is_uniform() {
        WARN_CIRCLE_NON_UNIFORM_BORDER.call_once(|| {
            tracing::warn!(
                "non-uniform border on a BoxShape::Circle is not painted; \
                 Flutter renders the single-visible-colour form of it via \
                 drawDRRect, which needs a drrect whose corners are a true \
                 circle -- ours approximates each corner with one quadratic \
                 Bezier (this warn fires once per process)"
            );
        });
        return;
    }
    let Some(side) = border.top else {
        return;
    };
    let width = side.width.get();
    if width <= 0.0 {
        return;
    }
    let diameter = circle.radius.get() * 2.0;
    if width > diameter {
        // Past the diameter the invariant is unsatisfiable: the stroke
        // center would need a negative radius, and pinning it at 0 while
        // keeping the full `width` puts the stroke's outer edge at
        // `width / 2 > radius` -- a disc bulging OUTSIDE the fill rather
        // than a ring inside it. Skip instead of painting that.
        //
        // Exactly AT the diameter is not this case and must not be folded
        // into it: `radius - width / 2` is 0, and a stroke of width
        // `2 * radius` centered there spans `±radius`, landing its outer
        // edge exactly on the fill radius. That is a full disc in the
        // border color, which is the correct rendering of a border as
        // thick as the shape -- skipping it would silently drop the
        // border color instead.
        //
        // Divergence from Flutter, deliberate: Flutter's
        // `_paintUniformBorderWithCircle` (`box_border.dart:346-350`) has
        // no such guard and `BorderSide` only asserts `width >= 0`, so an
        // over-wide border there reaches `drawCircle` with a zero-or-
        // negative radius. FLUI declines to paint rather than depend on a
        // backend's handling of a degenerate radius.
        return;
    }
    let stroke_radius = circle.radius.get() - width / 2.0;
    let paint = Paint::stroke(side.color, width);
    canvas.draw_circle(circle.center, Pixels(stroke_radius), &paint);
}

/// One box shadow: the casting silhouette is the decoration's shape
/// (the rounded rect, or the plain rect as a zero-radius one),
/// inflated by the spread radius and displaced by the offset; the blur
/// radius rides as the shadow primitive's elevation (the engine's blur
/// input).
fn paint_shadow(
    canvas: &mut Canvas,
    rect: Rect<Pixels>,
    rrect: Option<RRect>,
    shadow: &BoxShadow<Pixels>,
) {
    let base = rrect.unwrap_or_else(|| RRect::from_rect_circular(rect, Pixels(0.0)));
    let mut silhouette = base.inflate(shadow.spread_radius);
    silhouette.rect = silhouette.rect.translate_offset(shadow.offset);
    canvas.draw_shadow(
        &Path::from_rrect(silhouette),
        shadow.color,
        shadow.blur_radius.get(),
    );
}

/// Resolves an alignment-relative [`Gradient`] into a pixel-space
/// [`Shader`] for the given rect. Alignment is the (-1,-1)..(1,1)
/// space over the rect; the radial radius is a fraction of the
/// shortest side (Flutter parity).
#[must_use]
pub fn resolve_gradient(gradient: &Gradient, rect: Rect<Pixels>) -> Shader {
    let center = rect.center();
    let half_w = rect.width().get() / 2.0;
    let half_h = rect.height().get() / 2.0;
    let at = |alignment: flui_types::Alignment| {
        Offset::new(
            Pixels(center.x.get() + alignment.x * half_w),
            Pixels(center.y.get() + alignment.y * half_h),
        )
    };

    match gradient {
        Gradient::Linear(linear) => Shader::LinearGradient {
            from: at(linear.begin),
            to: at(linear.end),
            colors: linear.colors.clone(),
            stops: linear.stops.clone(),
            tile_mode: linear.tile_mode,
        },
        Gradient::Radial(radial) => Shader::RadialGradient {
            center: at(radial.center),
            radius: radial.radius * half_w.min(half_h) * 2.0,
            colors: radial.colors.clone(),
            stops: radial.stops.clone(),
            tile_mode: radial.tile_mode,
            focal: None,
            focal_radius: None,
        },
        Gradient::Sweep(sweep) => Shader::SweepGradient {
            center: at(sweep.center),
            colors: sweep.colors.clone(),
            stops: sweep.stops.clone(),
            tile_mode: sweep.tile_mode,
            start_angle: sweep.start_angle,
            end_angle: sweep.end_angle,
        },
    }
}

/// The decoration image, fitted into the rect per its `BoxFit` (the
/// repeat modes tile the image at its natural size).
fn paint_decoration_image(
    canvas: &mut Canvas,
    rect: Rect<Pixels>,
    image: &flui_types::styling::DecorationImage,
) {
    use flui_types::layout::BoxFit;
    use flui_types::styling::ImageRepeat;

    if image.repeat != ImageRepeat::NoRepeat {
        canvas.draw_image_repeat(image.image.clone(), rect, image.repeat, None);
        return;
    }

    // image dimensions are far below f32's 24-bit integer range
    let (src_w, src_h) = (image.image.width() as f32, image.image.height() as f32);
    let (dst_w, dst_h) = (rect.width().get(), rect.height().get());
    let fit = image.fit.unwrap_or(BoxFit::ScaleDown);

    let (out_w, out_h) = if src_w <= 0.0 || src_h <= 0.0 {
        (dst_w, dst_h)
    } else {
        match fit {
            BoxFit::Fill => (dst_w, dst_h),
            BoxFit::Contain => {
                let scale = (dst_w / src_w).min(dst_h / src_h);
                (src_w * scale, src_h * scale)
            }
            BoxFit::Cover => {
                let scale = (dst_w / src_w).max(dst_h / src_h);
                (src_w * scale, src_h * scale)
            }
            BoxFit::FitWidth => {
                let scale = dst_w / src_w;
                (dst_w, src_h * scale)
            }
            BoxFit::FitHeight => {
                let scale = dst_h / src_h;
                (src_w * scale, dst_h)
            }
            BoxFit::None => (src_w, src_h),
            BoxFit::ScaleDown => {
                let scale = (dst_w / src_w).min(dst_h / src_h).min(1.0);
                (src_w * scale, src_h * scale)
            }
        }
    };

    // Alignment positions the fitted box within the paint rect.
    let free_w = dst_w - out_w;
    let free_h = dst_h - out_h;
    let left = rect.min.x.get() + f32::midpoint(image.alignment.x, 1.0) * free_w;
    let top = rect.min.y.get() + f32::midpoint(image.alignment.y, 1.0) * free_h;
    let dst = Rect::from_ltrb(
        Pixels(left),
        Pixels(top),
        Pixels(left + out_w),
        Pixels(top + out_h),
    );

    let paint = (image.opacity < 1.0).then(|| {
        // clamped 0..=1 then scaled to u8 range
        let alpha = (image.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
        Paint::fill(Color::rgba(255, 255, 255, alpha))
    });
    canvas.draw_image(image.image.clone(), dst, paint.as_ref());
}

/// The border, on top of everything.
///
/// A uniform border strokes the shape exactly INSIDE its edge via a
/// filled outer/inner rounded-rect pair (`draw_drrect`) — Flutter's
/// inside-stroke semantics without relying on stroke centering. A
/// non-uniform border falls back to four filled edge rects; combining
/// per-side widths with a border radius is unsupported in Flutter as
/// well (it asserts), so the radius is ignored on that path.
///
/// `pub(crate)`: also reused by `crate::table_border::paint_table_border`
/// for `TableBorder`'s outer edge, so the uniform/non-uniform split is
/// written once.
pub(crate) fn paint_border(
    canvas: &mut Canvas,
    rect: Rect<Pixels>,
    rrect: Option<RRect>,
    border: &flui_types::styling::Border<Pixels>,
) {
    if border.is_uniform() {
        // Uniform ⇒ all four sides are the same `Some` (or all `None`,
        // in which case there is nothing to draw).
        let Some(side) = border.top else {
            return;
        };
        if side.width.get() <= 0.0 {
            return;
        }
        let outer = rrect.unwrap_or_else(|| RRect::from_rect_circular(rect, Pixels(0.0)));
        let inner = outer.inflate(Pixels(-side.width.get()));
        canvas.draw_drrect(outer, inner, &Paint::fill(side.color));
        return;
    }

    let side_width = |side: &Option<flui_types::styling::BorderSide<Pixels>>| {
        side.map_or(0.0, |s| s.width.get())
    };
    let side_color = |side: &Option<flui_types::styling::BorderSide<Pixels>>| {
        side.map_or(Color::TRANSPARENT, |s| s.color)
    };
    let (l, t, r, b) = (
        side_width(&border.left),
        side_width(&border.top),
        side_width(&border.right),
        side_width(&border.bottom),
    );
    let (x0, y0, x1, y1) = (
        rect.min.x.get(),
        rect.min.y.get(),
        rect.max.x.get(),
        rect.max.y.get(),
    );
    if t > 0.0 {
        canvas.draw_rect(
            Rect::from_ltrb(Pixels(x0), Pixels(y0), Pixels(x1), Pixels(y0 + t)),
            &Paint::fill(side_color(&border.top)),
        );
    }
    if b > 0.0 {
        canvas.draw_rect(
            Rect::from_ltrb(Pixels(x0), Pixels(y1 - b), Pixels(x1), Pixels(y1)),
            &Paint::fill(side_color(&border.bottom)),
        );
    }
    if l > 0.0 {
        canvas.draw_rect(
            Rect::from_ltrb(Pixels(x0), Pixels(y0 + t), Pixels(x0 + l), Pixels(y1 - b)),
            &Paint::fill(side_color(&border.left)),
        );
    }
    if r > 0.0 {
        canvas.draw_rect(
            Rect::from_ltrb(Pixels(x1 - r), Pixels(y0 + t), Pixels(x1), Pixels(y1 - b)),
            &Paint::fill(side_color(&border.right)),
        );
    }
}

/// Point-in-rounded-rect: inside the base rect AND outside none of the
/// four corner ellipses.
fn rrect_contains(rrect: &RRect, point: Point<Pixels>) -> bool {
    let rect = rrect.rect;
    if !rect.contains(point) {
        return false;
    }
    let (px_, py) = (point.x.get(), point.y.get());
    let (x0, y0, x1, y1) = (
        rect.min.x.get(),
        rect.min.y.get(),
        rect.max.x.get(),
        rect.max.y.get(),
    );

    // For each corner: if the point lies within the corner's radius
    // box, it must satisfy the ellipse equation.
    let in_ellipse = |cx: f32, cy: f32, rx: f32, ry: f32| {
        if rx <= 0.0 || ry <= 0.0 {
            return true;
        }
        let nx = (px_ - cx) / rx;
        let ny = (py - cy) / ry;
        nx * nx + ny * ny <= 1.0
    };

    let tl = rrect.top_left;
    if px_ < x0 + tl.x.get() && py < y0 + tl.y.get() {
        return in_ellipse(x0 + tl.x.get(), y0 + tl.y.get(), tl.x.get(), tl.y.get());
    }
    let tr = rrect.top_right;
    if px_ > x1 - tr.x.get() && py < y0 + tr.y.get() {
        return in_ellipse(x1 - tr.x.get(), y0 + tr.y.get(), tr.x.get(), tr.y.get());
    }
    let bl = rrect.bottom_left;
    if px_ < x0 + bl.x.get() && py > y1 - bl.y.get() {
        return in_ellipse(x0 + bl.x.get(), y1 - bl.y.get(), bl.x.get(), bl.y.get());
    }
    let br = rrect.bottom_right;
    if px_ > x1 - br.x.get() && py > y1 - br.y.get() {
        return in_ellipse(x1 - br.x.get(), y1 - br.y.get(), br.x.get(), br.y.get());
    }
    true
}
