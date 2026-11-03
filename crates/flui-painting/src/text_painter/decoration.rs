//! Drawing a decoration's lines along the stretches a layout gives
//! (ADR-0310 §4).

use flui_foundation::geometry::{Offset, Point, Rect};

use crate::canvas::Canvas;
use crate::paint::{Paint, Path};
use crate::parley_text::{DecoratedStretch, LinePlacement};
use crate::styling::Color;
use crate::typography::{TextDecoration, TextDecorationLines, TextDecorationStyle};

/// A decoration as a stretch draws it, its colour resolved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Drawn {
    lines: TextDecorationLines,
    style: TextDecorationStyle,
    color: Color,
    /// The multiple of the font's thickness.
    scale: f64,
}

impl Drawn {
    /// `decoration` drawn along glyphs that paint in `glyph`, or in `root`
    /// when they take the paragraph's colour; `None` when it draws nothing
    /// (no line, or a transparent colour).
    pub(super) fn of(
        decoration: TextDecoration,
        glyph: Option<Color>,
        root: Color,
    ) -> Option<Self> {
        let color = decoration.color.or(glyph).unwrap_or(root);
        (!decoration.is_none() && !color.is_transparent()).then_some(Self {
            lines: decoration.lines,
            style: decoration.style,
            color,
            scale: f64::from(decoration.thickness.get()),
        })
    }
}

/// Which lines a pass draws: underlines and overlines under the glyphs, a
/// line-through over them, as CSS paints them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Pass {
    UnderGlyphs,
    OverGlyphs,
}

/// The most dots, dashes or half-waves one line is drawn with; a longer
/// line of a finer pattern is drawn solid, so a tiny font over a wide line
/// cannot record an unbounded path.
const MAX_PATTERN_STEPS: f64 = 4096.0;

/// How many spacings of the line's coordinates a pattern's thickness must
/// span to be drawn: far from the origin a coordinate cannot place a finer
/// dot or dash, and the line is drawn solid.
const MIN_PATTERN_RESOLUTION: f64 = 16.0;

/// Records the lines of `stretches` this pass draws, for a paragraph with
/// its top-left at `offset`.
pub(super) fn draw(
    canvas: &mut Canvas,
    stretches: &[DecoratedStretch<Drawn>],
    offset: Offset<f64>,
    pass: Pass,
) {
    for stretch in stretches {
        let lines: &[(TextDecorationLines, LinePlacement, Kind)] = match pass {
            Pass::UnderGlyphs => &[
                (
                    TextDecorationLines::UNDERLINE,
                    stretch.metrics.underline,
                    Kind::Under,
                ),
                (
                    TextDecorationLines::OVERLINE,
                    stretch.metrics.overline,
                    Kind::Over,
                ),
            ],
            Pass::OverGlyphs => &[(
                TextDecorationLines::LINE_THROUGH,
                stretch.metrics.line_through,
                Kind::Through,
            )],
        };
        for &(line, placement, kind) in lines {
            if stretch.value.lines.contains(line) {
                draw_line(canvas, stretch, placement, kind, offset);
            }
        }
    }
}

/// How a line grows from the font's position when it is thicker than the
/// font's, and where a double line's second stroke goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Grows down from its top; the second stroke is below.
    Under,
    /// Grows down from the ascent; the second stroke is above.
    Over,
    /// Grows about its centre; the two strokes straddle it.
    Through,
}

fn draw_line(
    canvas: &mut Canvas,
    stretch: &DecoratedStretch<Drawn>,
    placement: LinePlacement,
    kind: Kind,
    offset: Offset<f64>,
) {
    let drawn = stretch.value;
    let thickness = placement.thickness * drawn.scale;
    let font_top = stretch.baseline + placement.top;
    let top = match kind {
        Kind::Under | Kind::Over => font_top,
        Kind::Through => font_top + (placement.thickness - thickness) / 2.0,
    };
    let (left, right) = (stretch.left + offset.dx, stretch.right + offset.dx);
    let top = top + offset.dy;
    if !(thickness > 0.0
        && [left, right, top, thickness]
            .iter()
            .all(|value| value.is_finite())
        && right > left)
    {
        return;
    }
    let fill = Paint::fill(drawn.color);
    let bar = |top: f64| Rect::from_ltrb(left, top, right, top + thickness);
    // Each step is placed from `left` by its index, never by adding to a
    // running position, which a far offset would absorb.
    let spacing = left.abs().max(right.abs()) * f64::EPSILON;
    let steps = |period: f64| {
        let count = ((right - left) / period).ceil();
        (count.is_finite()
            && count <= MAX_PATTERN_STEPS
            && thickness >= spacing * MIN_PATTERN_RESOLUTION)
            .then_some(count as usize)
    };
    let step_at = |index: usize, period: f64| left + index as f64 * period;
    match drawn.style {
        TextDecorationStyle::Double => {
            let second = match kind {
                Kind::Under => top + 2.0 * thickness,
                Kind::Over => top - 2.0 * thickness,
                Kind::Through => top + thickness,
            };
            let first = match kind {
                Kind::Through => top - thickness,
                Kind::Under | Kind::Over => top,
            };
            canvas.draw_rect(bar(first), &fill);
            canvas.draw_rect(bar(second), &fill);
        }
        TextDecorationStyle::Dotted => match steps(2.5 * thickness) {
            // Round dots a thickness wide, a thickness and a half apart;
            // a dot that would end past the stretch is left out.
            Some(count) => {
                let mut dots = Path::new();
                for x in (0..count).map(|index| step_at(index, 2.5 * thickness)) {
                    if x + thickness <= right {
                        dots.add_oval(Rect::from_ltrb(x, top, x + thickness, top + thickness));
                    }
                }
                if !dots.is_empty() {
                    canvas.draw_path(&dots, &fill);
                }
            }
            None => canvas.draw_rect(bar(top), &fill),
        },
        TextDecorationStyle::Dashed => match steps(6.0 * thickness) {
            // Dashes four thicknesses long, two apart; the last one is cut
            // at the stretch's end.
            Some(count) => {
                let mut dashes = Path::new();
                for x in (0..count).map(|index| step_at(index, 6.0 * thickness)) {
                    let end = (x + 4.0 * thickness).min(right);
                    if end > x {
                        dashes.add_rect(Rect::from_ltrb(x, top, end, top + thickness));
                    }
                }
                if !dashes.is_empty() {
                    canvas.draw_path(&dashes, &fill);
                }
            }
            None => canvas.draw_rect(bar(top), &fill),
        },
        TextDecorationStyle::Wavy => match steps(3.0 * thickness) {
            Some(count) => canvas.draw_path(
                &wave(left, right, top + thickness / 2.0, thickness, count),
                &Paint::stroke(drawn.color, thickness),
            ),
            None => canvas.draw_rect(bar(top), &fill),
        },
        // Solid, and a style this version does not draw.
        _ => canvas.draw_rect(bar(top), &fill),
    }
}

/// A wave of `count` half-waves around `centre` from `left` to `right`, six
/// thicknesses long and three high: one quadratic curve per half-wave, the
/// last one shortened to end at `right`.
fn wave(left: f64, right: f64, centre: f64, thickness: f64, count: usize) -> Path {
    let half = 3.0 * thickness;
    // A quadratic's control point twice the amplitude off the line puts the
    // curve's peak at the amplitude.
    let reach = 3.0 * thickness;
    let mut path = Path::new();
    path.move_to(Point::new(left, centre));
    for index in 0..count {
        let start = left + index as f64 * half;
        let end = (start + half).min(right);
        let bend = reach * ((end - start) / half);
        let control_y = if index % 2 == 0 {
            centre - bend
        } else {
            centre + bend
        };
        path.quadratic_bezier_to(
            Point::new(f64::midpoint(start, end), control_y),
            Point::new(end, centre),
        );
    }
    path
}
