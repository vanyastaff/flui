//! [`FocusRingStyle`] — the colors and widths of the keyboard focus
//! indicator, shared by the widget that outlines a box and the render
//! objects that outline a part of themselves.

use flui_foundation::geometry::{RRect, Rect};
use flui_painting::Canvas;
use flui_painting::paint::Paint;
use flui_painting::styling::Color;

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
