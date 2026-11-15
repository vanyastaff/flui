//! The scale a Wayland window's buffer is drawn at.
//!
//! With `wp_viewporter` the surface's destination is its logical size and
//! the buffer any size, so the buffer is drawn at the scale the compositor
//! prefers: a fractional one where it offers `wp_fractional_scale_v1`
//! (`preferred_scale`, as a numerator over 120), else the whole one `wl_surface`
//! version 6 prefers, else that of the output the surface entered.
//! Without the viewporter the buffer is the surface's size: scale 1, and
//! the compositor scales it.
//!
//! Each of those numbers is the compositor's, and a buffer drawn at one
//! is as many pixels. So a scale is a [`BufferScale`], which no scale
//! past [`BufferScale::MAX`] gets into: the window's content and the
//! frame it draws itself are drawn at that one and the viewport
//! stretches them. And a window's metrics are [`metrics`]'s, which
//! publishes none for a content past [`MAX_CONTENT_EXTENT`] device
//! pixels, the size a configure of any `int` can ask for.

#![forbid(unsafe_code)]

use flui_foundation::geometry::{DevicePixelRatio, Size, WindowMetrics};

/// The scale a surface's buffer is drawn at: the scale the compositor
/// named, up to [`Self::MAX`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BufferScale(DevicePixelRatio);

impl BufferScale {
    /// The largest scale a buffer is drawn at: screens are driven at one
    /// to four, so a buffer drawn finer than twice the densest shows no
    /// pixel more, and only costs memory and fill.
    pub(crate) const MAX: f64 = 8.0;
    /// One device pixel per logical pixel.
    pub(crate) const ONE: Self = Self(DevicePixelRatio::ONE);

    /// The scale a buffer is drawn at where the compositor names `told`;
    /// `None` for one that is no positive finite number.
    pub(crate) fn try_new(told: f64) -> Option<Self> {
        let told = DevicePixelRatio::try_new(told)?;
        DevicePixelRatio::try_new(told.get().min(Self::MAX)).map(Self)
    }

    /// Device pixels per logical pixel.
    pub(crate) const fn get(self) -> f64 {
        self.0.get()
    }
}

/// The widest or tallest a window's content is drawn, in device pixels:
/// the largest `max_texture_dimension_2d` a `wgpu` adapter reports, so
/// the largest surface any renderer makes.
pub(crate) const MAX_CONTENT_EXTENT: i32 = 32_768;

/// The metrics of a window whose content is `content` logical pixels
/// drawn at `scale`. `None` where it has no drawable area: an extent
/// that is no size, or one past [`MAX_CONTENT_EXTENT`] in device pixels,
/// for which nothing is published and nothing drawn until a configure
/// gives a size a surface can have.
pub(crate) fn metrics(content: Size<f64>, scale: BufferScale) -> Option<WindowMetrics> {
    let metrics = WindowMetrics::from_logical(content, scale.get()).ok()?;
    let physical = metrics.physical();
    (physical.width <= MAX_CONTENT_EXTENT && physical.height <= MAX_CONTENT_EXTENT)
        .then_some(metrics)
}

/// What the compositor said of the scale a surface is drawn at.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct ScaleTold {
    /// `wp_fractional_scale_v1.preferred_scale`, as a numerator over 120.
    pub(crate) fractional: Option<u32>,
    /// `wl_surface.preferred_buffer_scale`.
    pub(crate) preferred: Option<i32>,
    /// The scale of the output the surface is on.
    pub(crate) output: Option<f64>,
}

/// A fractional scale as the protocol carries it, as a numerator over 120; `None` for
/// zero, which is no scale.
fn over_120(numerator: u32) -> Option<f64> {
    (numerator > 0).then(|| f64::from(numerator) / 120.0)
}

/// The scale the window's buffer is drawn at.
pub(crate) fn buffer_scale(viewported: bool, told: ScaleTold) -> BufferScale {
    if !viewported {
        return BufferScale::ONE;
    }
    told.fractional
        .and_then(over_120)
        .or_else(|| told.preferred.filter(|scale| *scale > 0).map(f64::from))
        .or(told.output)
        .and_then(BufferScale::try_new)
        .unwrap_or(BufferScale::ONE)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fractional scale wins over the whole ones, and each falls back
    /// to the next where the compositor said nothing usable.
    #[test]
    fn a_buffer_is_drawn_at_the_scale_the_compositor_prefers() {
        let told = |fractional, preferred, output| ScaleTold {
            fractional,
            preferred,
            output,
        };
        for (case, viewported, told, expected) in [
            ("nothing said", true, told(None, None, None), 1.0),
            (
                "a fractional scale",
                true,
                told(Some(180), Some(2), Some(2.0)),
                1.5,
            ),
            (
                "a whole preferred scale",
                true,
                told(None, Some(2), Some(1.0)),
                2.0,
            ),
            ("the output's", true, told(None, None, Some(3.0)), 3.0),
            ("a zero fraction", true, told(Some(0), Some(2), None), 2.0),
            (
                "a zero preferred scale",
                true,
                told(None, Some(0), Some(1.25)),
                1.25,
            ),
            (
                "a broken output scale",
                true,
                told(None, None, Some(f64::NAN)),
                1.0,
            ),
            (
                "no viewporter",
                false,
                told(Some(180), Some(2), Some(2.0)),
                1.0,
            ),
            (
                "a numerator over 120",
                true,
                told(Some(150), None, None),
                1.25,
            ),
            // A compositor names any scale: past the largest, the buffer
            // is drawn at the largest.
            (
                "a fraction in the hundreds",
                true,
                told(Some(442 * 120), None, None),
                BufferScale::MAX,
            ),
            (
                "the largest fraction the protocol carries",
                true,
                told(Some(u32::MAX), None, None),
                BufferScale::MAX,
            ),
            (
                "the largest whole scale the protocol carries",
                true,
                told(None, Some(i32::MAX), None),
                BufferScale::MAX,
            ),
            (
                "an output scale past the largest",
                true,
                told(None, None, Some(1e300)),
                BufferScale::MAX,
            ),
            (
                "an infinite output scale",
                true,
                told(None, None, Some(f64::INFINITY)),
                1.0,
            ),
        ] {
            assert_eq!(buffer_scale(viewported, told).get(), expected, "{case}");
        }
    }

    /// A window publishes what a surface can be, whatever the compositor
    /// says. Told a scale in the hundreds, a 400 by 300 window is drawn
    /// at the largest scale, not 176 800 pixels across; given a size
    /// past any surface it publishes no metrics, and no product of a
    /// size and a scale overflows or is published non-finite.
    #[test]
    fn a_window_publishes_what_a_surface_can_be() {
        let hostile = buffer_scale(
            true,
            ScaleTold {
                fractional: Some(442 * 120),
                preferred: None,
                output: None,
            },
        );
        let published =
            metrics(Size::new(400.0, 300.0), hostile).expect("drawn at the largest scale");
        assert_eq!(
            (published.physical(), published.scale().get()),
            (Size::new(3_200, 2_400), BufferScale::MAX)
        );

        let widest = f64::from(MAX_CONTENT_EXTENT) / BufferScale::MAX;
        for (case, (width, height), scale, expected) in [
            ("a window at one", (400.0, 300.0), 1.0, Some((400, 300))),
            ("a fractional scale", (400.0, 300.0), 1.5, Some((600, 450))),
            (
                "the widest content",
                (widest, 300.0),
                BufferScale::MAX,
                Some((MAX_CONTENT_EXTENT, 2_400)),
            ),
            (
                "a pixel past the widest",
                (widest + 1.0, 300.0),
                BufferScale::MAX,
                None,
            ),
            ("a configure of two thousand million", (2e9, 2e9), 1.0, None),
            (
                "the largest configure at the largest scale",
                (f64::from(i32::MAX), f64::from(i32::MAX)),
                BufferScale::MAX,
                None,
            ),
            ("a size that is none", (0.0, 300.0), 1.0, None),
            ("a size that is not finite", (f64::NAN, 300.0), 1.0, None),
        ] {
            let scale = BufferScale::try_new(scale).expect("a scale");
            let physical = metrics(Size::new(width, height), scale)
                .map(|metrics| (metrics.physical().width, metrics.physical().height));
            assert_eq!(physical, expected, "{case}");
        }
    }
}
