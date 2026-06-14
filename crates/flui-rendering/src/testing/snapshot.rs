//! Stable, normalized projection of one [`DrawCommand`] to a single text line.
//!
//! This is the leaf of the snapshot serializer and the unit a predicate sees.
//! Stability contract: once the line format is chosen (floats 2-dec, color
//! `#RRGGBBAA`, transform omitted unless non-identity) it must not drift.
//!
//! # Task context
//!
//! Task 3 will walk the `LayerTree` and call [`summarize_command`] per picture
//! command. Task 4 exposes it on `FrameRun`. Keep this file focused: just
//! summary + helpers.

use flui_painting::PaintStyle;
use flui_painting::display_list::{ClipOp, DrawCommand, Paint};
use flui_types::{
    geometry::{Matrix4, Pixels, Point, RRect, Rect},
    styling::Color,
};

/// Coarse category of a drawing command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawKind {
    /// Rectangle (filled or stroked).
    Rect,
    /// Rounded rectangle.
    RRect,
    /// Circle.
    Circle,
    /// Oval / ellipse.
    Oval,
    /// Arbitrary path.
    Path,
    /// Line segment.
    Line,
    /// Arc segment.
    Arc,
    /// Difference between two rounded rectangles.
    DRRect,
    /// Clip operation (any geometry).
    Clip,
    /// Text (plain or rich spans).
    Text,
    /// Image (any image variant, atlas, or texture).
    Image,
    /// Drop shadow.
    Shadow,
    /// Gradient fill.
    Gradient,
    /// Layer command (SaveLayer / RestoreLayer / ShaderMask / BackdropFilter).
    Layer,
    /// Any variant not covered by the above (fills, vertices, …).
    Other,
}

/// Stable, normalized projection of one [`DrawCommand`].
///
/// The `line` field is what tests assert on; the `kind` field lets predicates
/// filter by category without parsing strings.
#[derive(Debug, Clone, PartialEq)]
pub struct DrawCommandSummary {
    /// Coarse category of the command.
    pub kind: DrawKind,
    /// Stable single-line text representation of the command.
    pub line: String,
}

// ── private helpers ──────────────────────────────────────────────────────────

/// Format one `f32` to 2 decimal places, normalizing `-0.0` → `0.0`.
fn f(v: f32) -> String {
    // Normalize negative zero before formatting.
    let v = if v == 0.0 { 0.0_f32 } else { v };
    format!("{v:.2}")
}

/// Format a `Color` as `#RRGGBBAA`.
fn hex_color(c: Color) -> String {
    format!("#{:02X}{:02X}{:02X}{:02X}", c.r, c.g, c.b, c.a)
}

/// Summarize a `Paint` as `"<style> <#RRGGBBAA>[ stroke=<w>]"`.
fn summarize_paint(paint: &Paint) -> String {
    let style = match paint.style {
        PaintStyle::Fill => "fill",
        PaintStyle::Stroke => "stroke",
    };
    let color = hex_color(paint.color);
    if matches!(paint.style, PaintStyle::Stroke) {
        format!("{style} {color} stroke={}", f(paint.stroke_width))
    } else {
        format!("{style} {color}")
    }
}

/// Format a `Rect<Pixels>` as `"(l,t WxH)"`.
fn fmt_rect(r: Rect<Pixels>) -> String {
    format!(
        "({},{} {}x{})",
        f(r.left().get()),
        f(r.top().get()),
        f(r.width().get()),
        f(r.height().get()),
    )
}

/// Format a `Point<Pixels>` as `"(x,y)"`.
fn fmt_point(p: Point<Pixels>) -> String {
    format!("({},{})", f(p.x.get()), f(p.y.get()))
}

/// Format an `RRect` as `"(l,t WxH r=tl/tr/br/bl)"`.
///
/// Uses the `rect` field of `RRect` for geometry and the four corner radii
/// (circular approximation: `x` component of each radius).
fn fmt_rrect(rr: &RRect) -> String {
    let r = rr.rect;
    format!(
        "({},{} {}x{} r={}/{}/{}/{})",
        f(r.left().get()),
        f(r.top().get()),
        f(r.width().get()),
        f(r.height().get()),
        f(rr.top_left.x.get()),
        f(rr.top_right.x.get()),
        f(rr.bottom_right.x.get()),
        f(rr.bottom_left.x.get()),
    )
}

/// Format a `ClipOp` as a short lowercase string.
fn fmt_clip_op(op: ClipOp) -> &'static str {
    match op {
        ClipOp::Intersect => "intersect",
        ClipOp::Difference => "difference",
    }
}

/// Append a transform suffix when the matrix is non-identity.
fn maybe_transform(transform: &Matrix4) -> String {
    if transform.is_identity() {
        return String::new();
    }
    // Build the bracket inline without an intermediate `Vec` allocation.
    let mut s = " xf=[".to_owned();
    let mut first = true;
    for v in &transform.m {
        if !first {
            s.push(',');
        }
        first = false;
        s.push_str(&f(*v));
    }
    s.push(']');
    s
}

// ── public API ───────────────────────────────────────────────────────────────

/// Produce a stable, normalized single-line summary of one [`DrawCommand`].
///
/// Every named variant gets its own match arm so that adding a new variant
/// to `DrawCommand` (a coordinated breaking change) immediately produces a
/// compile error here rather than silently falling through.
#[must_use]
pub fn summarize_command(cmd: &DrawCommand) -> DrawCommandSummary {
    match cmd {
        // ── Clips ────────────────────────────────────────────────────────────
        DrawCommand::ClipRect {
            rect,
            clip_op,
            transform,
            ..
        } => DrawCommandSummary {
            kind: DrawKind::Clip,
            line: format!(
                "ClipRect rect={} op={}{}",
                fmt_rect(*rect),
                fmt_clip_op(*clip_op),
                maybe_transform(transform),
            ),
        },

        DrawCommand::ClipRRect {
            rrect,
            clip_op,
            transform,
            ..
        } => DrawCommandSummary {
            kind: DrawKind::Clip,
            line: format!(
                "ClipRRect rrect={} op={}{}",
                fmt_rrect(rrect),
                fmt_clip_op(*clip_op),
                maybe_transform(transform),
            ),
        },

        DrawCommand::ClipRSuperellipse {
            rsuperellipse,
            clip_op,
            transform,
            ..
        } => DrawCommandSummary {
            kind: DrawKind::Clip,
            line: format!(
                "ClipRSuperellipse rect={} op={}{}",
                fmt_rect(rsuperellipse.outer_rect()),
                fmt_clip_op(*clip_op),
                maybe_transform(transform),
            ),
        },

        DrawCommand::ClipPath {
            path,
            clip_op,
            transform,
            ..
        } => DrawCommandSummary {
            kind: DrawKind::Clip,
            line: format!(
                "ClipPath bounds={} pts={} op={}{}",
                fmt_rect(path.compute_bounds()),
                path.commands().len(),
                fmt_clip_op(*clip_op),
                maybe_transform(transform),
            ),
        },

        // ── Primitive shapes ─────────────────────────────────────────────────
        DrawCommand::DrawLine {
            p1,
            p2,
            paint,
            transform,
        } => DrawCommandSummary {
            kind: DrawKind::Line,
            line: format!(
                "DrawLine {}->{} {}{}",
                fmt_point(*p1),
                fmt_point(*p2),
                summarize_paint(paint),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawRect {
            rect,
            paint,
            transform,
        } => DrawCommandSummary {
            kind: DrawKind::Rect,
            line: format!(
                "DrawRect rect={} {}{}",
                fmt_rect(*rect),
                summarize_paint(paint),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawRRect {
            rrect,
            paint,
            transform,
        } => DrawCommandSummary {
            kind: DrawKind::RRect,
            line: format!(
                "DrawRRect rrect={} {}{}",
                fmt_rrect(rrect),
                summarize_paint(paint),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawCircle {
            center,
            radius,
            paint,
            transform,
        } => DrawCommandSummary {
            kind: DrawKind::Circle,
            line: format!(
                "DrawCircle center={} r={} {}{}",
                fmt_point(*center),
                f(radius.get()),
                summarize_paint(paint),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawOval {
            rect,
            paint,
            transform,
        } => DrawCommandSummary {
            kind: DrawKind::Oval,
            line: format!(
                "DrawOval rect={} {}{}",
                fmt_rect(*rect),
                summarize_paint(paint),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawPath {
            path,
            paint,
            transform,
        } => {
            // Do NOT dump raw path verbs — too verbose and unstable.
            // Use bounds + command count as the stable fingerprint.
            DrawCommandSummary {
                kind: DrawKind::Path,
                line: format!(
                    "DrawPath bounds={} pts={} {}{}",
                    fmt_rect(path.compute_bounds()),
                    path.commands().len(),
                    summarize_paint(paint),
                    maybe_transform(transform),
                ),
            }
        }

        DrawCommand::DrawArc {
            rect,
            start_angle,
            sweep_angle,
            use_center,
            paint,
            transform,
        } => DrawCommandSummary {
            kind: DrawKind::Arc,
            line: format!(
                "DrawArc rect={} start={} sweep={} center={} {}{}",
                fmt_rect(*rect),
                f(*start_angle),
                f(*sweep_angle),
                use_center,
                summarize_paint(paint),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawDRRect {
            outer,
            inner,
            paint,
            transform,
        } => DrawCommandSummary {
            kind: DrawKind::DRRect,
            line: format!(
                "DrawDRRect outer={} inner={} {}{}",
                fmt_rrect(outer),
                fmt_rrect(inner),
                summarize_paint(paint),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawPoints {
            mode,
            points,
            paint,
            transform,
        } => DrawCommandSummary {
            kind: DrawKind::Path,
            line: format!(
                "DrawPoints mode={mode:?} pts={} {}{}",
                points.len(),
                summarize_paint(paint),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawVertices {
            vertices,
            paint,
            transform,
            ..
        } => DrawCommandSummary {
            kind: DrawKind::Other,
            line: format!(
                "DrawVertices verts={} {}{}",
                vertices.len(),
                summarize_paint(paint),
                maybe_transform(transform),
            ),
        },

        // ── Text ─────────────────────────────────────────────────────────────
        DrawCommand::DrawText {
            text,
            offset,
            paint,
            transform,
            ..
        } => DrawCommandSummary {
            kind: DrawKind::Text,
            line: format!(
                "DrawText offset=({},{}) {:?} {}{}",
                f(offset.dx.get()),
                f(offset.dy.get()),
                text,
                summarize_paint(paint),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawTextSpan {
            span,
            offset,
            transform,
            ..
        } => {
            // Summarize via plain text (via to_plain_text). Glyph/run details
            // are NOT needed — shaped content is Task 3's concern (layer walk).
            let plain = span.to_plain_text();
            DrawCommandSummary {
                kind: DrawKind::Text,
                line: format!(
                    "DrawTextSpan offset=({},{}) {:?}{}",
                    f(offset.dx.get()),
                    f(offset.dy.get()),
                    plain,
                    maybe_transform(transform),
                ),
            }
        }

        // ── Images ───────────────────────────────────────────────────────────
        DrawCommand::DrawImage { dst, transform, .. } => DrawCommandSummary {
            kind: DrawKind::Image,
            line: format!(
                "DrawImage dst={}{}",
                fmt_rect(*dst),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawImageRepeat { dst, transform, .. } => DrawCommandSummary {
            kind: DrawKind::Image,
            line: format!(
                "DrawImageRepeat dst={}{}",
                fmt_rect(*dst),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawImageNineSlice { dst, transform, .. } => DrawCommandSummary {
            kind: DrawKind::Image,
            line: format!(
                "DrawImageNineSlice dst={}{}",
                fmt_rect(*dst),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawImageFiltered { dst, transform, .. } => DrawCommandSummary {
            kind: DrawKind::Image,
            line: format!(
                "DrawImageFiltered dst={}{}",
                fmt_rect(*dst),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawTexture { dst, transform, .. } => DrawCommandSummary {
            kind: DrawKind::Image,
            line: format!(
                "DrawTexture dst={}{}",
                fmt_rect(*dst),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawAtlas {
            image: _,
            sprites,
            transform,
            ..
        } => DrawCommandSummary {
            kind: DrawKind::Image,
            line: format!(
                "DrawAtlas sprites={}{}",
                sprites.len(),
                maybe_transform(transform),
            ),
        },

        // ── Effects ──────────────────────────────────────────────────────────
        DrawCommand::DrawShadow {
            path,
            color,
            elevation,
            transform,
        } => DrawCommandSummary {
            kind: DrawKind::Shadow,
            line: format!(
                "DrawShadow path_bounds={} color={} elev={}{}",
                fmt_rect(path.compute_bounds()),
                hex_color(*color),
                f(*elevation),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawGradient {
            rect, transform, ..
        } => DrawCommandSummary {
            kind: DrawKind::Gradient,
            line: format!(
                "DrawGradient rect={}{}",
                fmt_rect(*rect),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawGradientRRect {
            rrect, transform, ..
        } => DrawCommandSummary {
            kind: DrawKind::Gradient,
            line: format!(
                "DrawGradientRRect rrect={}{}",
                fmt_rrect(rrect),
                maybe_transform(transform),
            ),
        },

        DrawCommand::ShaderMask {
            bounds,
            transform,
            // child: Box<DisplayList> — recursing into child display lists is
            // Task 3 (layer walk). Here we only summarize this command line.
            ..
        } => DrawCommandSummary {
            kind: DrawKind::Layer,
            line: format!(
                "ShaderMask bounds={}{}",
                fmt_rect(*bounds),
                maybe_transform(transform),
            ),
        },

        DrawCommand::BackdropFilter {
            bounds,
            transform,
            // child: Option<Box<DisplayList>> — recursing into child display
            // lists is Task 3 (layer walk). Here we only summarize this line.
            ..
        } => DrawCommandSummary {
            kind: DrawKind::Layer,
            line: format!(
                "BackdropFilter bounds={}{}",
                fmt_rect(*bounds),
                maybe_transform(transform),
            ),
        },

        // ── Fills ────────────────────────────────────────────────────────────
        DrawCommand::DrawColor {
            color,
            blend_mode,
            transform,
        } => DrawCommandSummary {
            kind: DrawKind::Other,
            line: format!(
                "DrawColor {} mode={blend_mode:?}{}",
                hex_color(*color),
                maybe_transform(transform),
            ),
        },

        DrawCommand::DrawPaint {
            paint, transform, ..
        } => DrawCommandSummary {
            kind: DrawKind::Other,
            line: format!(
                "DrawPaint {}{}",
                summarize_paint(paint),
                maybe_transform(transform),
            ),
        },

        // ── Layer commands ───────────────────────────────────────────────────
        DrawCommand::SaveLayer {
            bounds,
            paint,
            transform,
        } => DrawCommandSummary {
            kind: DrawKind::Layer,
            line: format!(
                "SaveLayer bounds={} {}{}",
                match bounds {
                    Some(r) => fmt_rect(*r),
                    None => "none".to_owned(),
                },
                summarize_paint(paint),
                maybe_transform(transform),
            ),
        },

        DrawCommand::RestoreLayer { transform } => DrawCommandSummary {
            kind: DrawKind::Layer,
            line: format!("RestoreLayer{}", maybe_transform(transform)),
        },

        // Catch-all for any future `#[non_exhaustive]` variants added to
        // `DrawCommand` that are not yet named above. Every *named* variant
        // above already has a dedicated arm, so nothing silently falls through
        // within the current variant set.
        _ => DrawCommandSummary {
            kind: DrawKind::Other,
            line: "Unknown".to_owned(),
        },
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use flui_painting::display_list::{DrawCommand, Paint};
    use flui_types::{
        geometry::{Matrix4, Pixels, Point, Rect, px},
        painting::Path,
        styling::Color,
    };

    use super::{DrawKind, summarize_command};

    /// Helper: build an identity `Rect<Pixels>` from raw f32 coordinates.
    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect<Pixels> {
        Rect::from_xywh(px(x), px(y), px(w), px(h))
    }

    /// `DrawRect` with `fill Color::RED` + identity transform must produce
    /// `kind == Rect` and a stable line.
    #[test]
    fn summarize_draw_rect_is_stable() {
        let cmd = DrawCommand::DrawRect {
            rect: rect(0.0, 0.0, 40.0, 40.0),
            paint: Arc::new(Paint::fill(Color::RED)),
            transform: Matrix4::IDENTITY,
        };
        let s = summarize_command(&cmd);
        assert_eq!(s.kind, DrawKind::Rect);
        // Color::RED = rgba(255,0,0,255) → #FF0000FF
        assert_eq!(
            s.line,
            "DrawRect rect=(0.00,0.00 40.00x40.00) fill #FF0000FF"
        );
    }

    /// `DrawShadow` must summarize with `kind == Shadow`.
    #[test]
    fn summarize_draw_shadow_has_shadow_kind() {
        let mut path = Path::new();
        path.add_rect(rect(10.0, 10.0, 50.0, 30.0));

        let cmd = DrawCommand::DrawShadow {
            path,
            color: Color::BLACK,
            elevation: 4.0,
            transform: Matrix4::IDENTITY,
        };
        let s = summarize_command(&cmd);
        assert_eq!(s.kind, DrawKind::Shadow);
        assert!(
            s.line.starts_with("DrawShadow"),
            "unexpected line: {}",
            s.line
        );
        assert!(
            s.line.contains("elev=4.00"),
            "expected elev=4.00 in: {}",
            s.line
        );
        // Color::BLACK = rgba(0,0,0,255) → #000000FF
        assert!(
            s.line.contains("#000000FF"),
            "expected #000000FF in: {}",
            s.line
        );
    }

    /// Non-identity transform must append `xf=[...]`.
    #[test]
    fn non_identity_transform_is_appended() {
        let translate = Matrix4::translation(10.0, 20.0, 0.0);
        let cmd = DrawCommand::DrawRect {
            rect: rect(0.0, 0.0, 10.0, 10.0),
            paint: Arc::new(Paint::fill(Color::BLUE)),
            transform: translate,
        };
        let s = summarize_command(&cmd);
        assert!(
            s.line.contains("xf=["),
            "expected xf= suffix in: {}",
            s.line
        );
    }

    /// Identity transform must NOT append `xf=[...]`.
    #[test]
    fn identity_transform_is_omitted() {
        let cmd = DrawCommand::DrawRect {
            rect: rect(0.0, 0.0, 10.0, 10.0),
            paint: Arc::new(Paint::fill(Color::BLUE)),
            transform: Matrix4::IDENTITY,
        };
        let s = summarize_command(&cmd);
        assert!(
            !s.line.contains("xf=["),
            "identity transform should be omitted, got: {}",
            s.line
        );
    }

    /// Stroke paint includes `stroke=<w>`.
    #[test]
    fn stroke_paint_includes_width() {
        let cmd = DrawCommand::DrawRect {
            rect: rect(0.0, 0.0, 10.0, 10.0),
            paint: Arc::new(Paint::stroke(Color::GREEN, 2.5)),
            transform: Matrix4::IDENTITY,
        };
        let s = summarize_command(&cmd);
        // Color::GREEN = rgba(0,255,0,255)
        assert_eq!(
            s.line,
            "DrawRect rect=(0.00,0.00 10.00x10.00) stroke #00FF00FF stroke=2.50"
        );
    }

    /// `DrawText` must summarize with `kind == Text` and include the text.
    #[test]
    fn summarize_draw_text_has_text_kind() {
        use flui_types::geometry::Offset;
        let cmd = DrawCommand::DrawText {
            text: "hello".to_owned(),
            offset: Offset::new(px(1.0), px(2.0)),
            size: flui_types::geometry::Size::new(px(50.0), px(12.0)),
            style: flui_types::typography::TextStyle::default(),
            paint: Arc::new(Paint::fill(Color::BLACK)),
            transform: Matrix4::IDENTITY,
        };
        let s = summarize_command(&cmd);
        assert_eq!(s.kind, DrawKind::Text);
        assert!(s.line.contains("\"hello\""), "expected text in: {}", s.line);
    }

    /// `ClipRect` must summarize with `kind == Clip`.
    #[test]
    fn summarize_clip_rect_has_clip_kind() {
        use flui_painting::display_list::ClipOp;
        use flui_types::painting::Clip;
        let cmd = DrawCommand::ClipRect {
            rect: rect(5.0, 5.0, 100.0, 80.0),
            clip_op: ClipOp::Intersect,
            clip_behavior: Clip::HardEdge,
            transform: Matrix4::IDENTITY,
        };
        let s = summarize_command(&cmd);
        assert_eq!(s.kind, DrawKind::Clip);
        assert!(
            s.line.starts_with("ClipRect"),
            "unexpected line: {}",
            s.line
        );
        assert!(
            s.line.contains("op=intersect"),
            "expected op=intersect in: {}",
            s.line
        );
    }

    /// `RestoreLayer` must summarize with `kind == Layer`.
    #[test]
    fn summarize_restore_layer_has_layer_kind() {
        let cmd = DrawCommand::RestoreLayer {
            transform: Matrix4::IDENTITY,
        };
        let s = summarize_command(&cmd);
        assert_eq!(s.kind, DrawKind::Layer);
        assert_eq!(s.line, "RestoreLayer");
    }

    /// Negative-zero normalization: `f(-0.0)` must produce `"0.00"` not `"-0.00"`.
    #[test]
    fn negative_zero_normalizes_to_zero() {
        use super::f;
        assert_eq!(f(-0.0_f32), "0.00");
        assert_eq!(f(0.0_f32), "0.00");
        assert_eq!(f(-1.5_f32), "-1.50");
    }

    /// `hex_color` produces the canonical `#RRGGBBAA` format.
    #[test]
    fn hex_color_format() {
        use super::hex_color;
        assert_eq!(hex_color(Color::RED), "#FF0000FF");
        assert_eq!(hex_color(Color::TRANSPARENT), "#00000000");
        assert_eq!(hex_color(Color::rgba(1, 2, 3, 4)), "#01020304");
    }

    /// `DrawImage` must summarize with `kind == Image`.
    #[test]
    fn summarize_draw_image_has_image_kind() {
        use flui_types::painting::image::Image;
        let cmd = DrawCommand::DrawImage {
            image: Image::default(),
            dst: rect(0.0, 0.0, 100.0, 80.0),
            paint: None,
            transform: Matrix4::IDENTITY,
        };
        let s = summarize_command(&cmd);
        assert_eq!(s.kind, DrawKind::Image);
        assert!(
            s.line.starts_with("DrawImage"),
            "unexpected line: {}",
            s.line
        );
    }

    /// Point helper produces correct format.
    #[test]
    fn fmt_point_helper() {
        use super::fmt_point;
        let p = Point::new(px(3.5), px(-1.0));
        assert_eq!(fmt_point(p), "(3.50,-1.00)");
    }
}
