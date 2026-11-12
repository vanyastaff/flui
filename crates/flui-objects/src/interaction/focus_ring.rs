//! [`FocusRingStyle`] — the colors and widths of the keyboard focus
//! indicator, shared by the widget that outlines a box and the render
//! objects that outline a part of themselves.

use flui_foundation::geometry::{RRect, Rect};
use flui_painting::Canvas;
use flui_painting::paint::Paint;
use flui_painting::styling::{Border, BorderSide, BorderStyle, BoxDecoration, Color};
use flui_painting::{DecorationPaintOptions, paint_box_decoration};

/// The colors and widths of a focus ring: two bands, `outer` the farther
/// from what is focused and `inner` against it.
///
/// Two contrasting bands keep the ring visible on any background: against a
/// dark surface the light band carries the contrast, against a light one the
/// dark band does. A width that is negative or not finite draws no band.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FocusRingStyle {
    /// The band farther from what is focused.
    pub outer: Color,
    /// The band against what is focused.
    pub inner: Color,
    /// Width of the outer band, in logical pixels.
    pub outer_width: f64,
    /// Width of the inner band, in logical pixels.
    pub inner_width: f64,
}

impl FocusRingStyle {
    /// The style a ring uses when nothing names one: a 2 px black band and a
    /// 1 px white band inside it, the shape of the WinUI focus visual.
    pub const BUILT_IN: Self = Self {
        outer: Color::BLACK,
        inner: Color::WHITE,
        outer_width: 2.0,
        inner_width: 1.0,
    };

    /// The outer band's width as it paints: finite and positive, else zero.
    #[must_use]
    pub fn outer_band(&self) -> f64 {
        band(self.outer_width)
    }

    /// The inner band's width as it paints: finite and positive, else zero.
    #[must_use]
    pub fn inner_band(&self) -> f64 {
        band(self.inner_width)
    }

    /// Paints the ring just outside `rect`: the inner band against it and
    /// the outer band around that, so the ring covers nothing inside `rect`.
    pub(crate) fn paint_around(&self, canvas: &mut Canvas, rect: Rect<f64>) {
        let (inner, outer) = (self.inner_band(), self.outer_band());
        if inner > 0.0 {
            canvas.draw_drrect(
                RRect::from_rect(rect.expand(inner)),
                RRect::from_rect(rect),
                &Paint::fill(self.inner),
            );
        }
        if outer > 0.0 {
            canvas.draw_drrect(
                RRect::from_rect(rect.expand(inner + outer)),
                RRect::from_rect(rect.expand(inner)),
                &Paint::fill(self.outer),
            );
        }
    }

    /// The two bands as borders of a box, `(outer, inner)`, for a ring drawn
    /// inside the box it marks. The inner border is as wide as both bands;
    /// the outer one is drawn over its outer part.
    #[must_use]
    pub fn bands_inside(&self) -> (BoxDecoration, BoxDecoration) {
        let (outer, inner) = (self.outer_band(), self.inner_band());
        let inner = if inner > 0.0 {
            band_border(self.inner, outer + inner)
        } else {
            BoxDecoration::new()
        };
        (band_border(self.outer, outer), inner)
    }

    /// Paints the ring just inside `rect`, over what is already drawn
    /// there: the outer band along the edge and the inner band within it.
    pub(crate) fn paint_inside(&self, canvas: &mut Canvas, rect: Rect<f64>) {
        let (outer, inner) = self.bands_inside();
        paint_box_decoration(canvas, rect, &inner, DecorationPaintOptions::default());
        paint_box_decoration(canvas, rect, &outer, DecorationPaintOptions::default());
    }
}

impl Default for FocusRingStyle {
    fn default() -> Self {
        Self::BUILT_IN
    }
}

fn band(width: f64) -> f64 {
    if width.is_finite() && width > 0.0 {
        width
    } else {
        0.0
    }
}

/// A uniform `width` border in `color`, or no border for a zero width.
fn band_border(color: Color, width: f64) -> BoxDecoration {
    if width > 0.0 {
        BoxDecoration::new().set_border(Some(Border::all(BorderSide::new(
            color,
            width,
            BorderStyle::Solid,
        ))))
    } else {
        BoxDecoration::new()
    }
}
