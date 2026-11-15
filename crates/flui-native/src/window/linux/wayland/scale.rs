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
//! stretches them. A configure of any `int` asks for a content of any size,
//! and the renderer configures no surface past [`MAX_CONTENT_EXTENT`]
//! device pixels on a side: so the scale is lowered further until the
//! content fits ([`BufferScale::fitted`]), and the window still draws, at
//! fewer pixels the viewport stretches. A window's metrics are
//! [`metrics`]'s, which publishes none it cannot draw: a content past the
//! extent with no viewport to stretch it, or one that is no size.

#![forbid(unsafe_code)]

use flui_foundation::geometry::{DevicePixelRatio, Size, WindowMetrics};
use flui_platform::MAX_SURFACE_EXTENT;

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

    /// The scale as a device pixel ratio.
    pub(crate) const fn ratio(self) -> DevicePixelRatio {
        self.0
    }

    /// This scale, lowered where `content` logical pixels drawn at it would
    /// pass [`MAX_CONTENT_EXTENT`] device pixels on a side, so they fit;
    /// this scale where the content is no size.
    pub(crate) fn fitted(self, content: Size<f64>) -> Self {
        let longest = content.width.max(content.height);
        if !(longest.is_finite() && longest > 0.0) {
            return self;
        }
        let fits = f64::from(MAX_CONTENT_EXTENT) / longest;
        if self.get() <= fits {
            return self;
        }
        DevicePixelRatio::try_new(fits).map_or(self, Self)
    }
}

/// The widest or tallest a window's content is drawn, in device pixels:
/// the largest surface a renderer draws ([`MAX_SURFACE_EXTENT`]), signed
/// as window metrics count device pixels.
pub(crate) const MAX_CONTENT_EXTENT: i32 = MAX_SURFACE_EXTENT.cast_signed();

/// The metrics of a window whose content is `content` logical pixels
/// drawn at `scale`, which [`buffer_scale`] fitted to it. `None` where it
/// has no drawable area: an extent that is no size, or one past
/// [`MAX_CONTENT_EXTENT`] in device pixels, which a window with no
/// viewport cannot draw at a scale below one; nothing is published and
/// nothing drawn until a configure gives a size a surface can have. A side
/// that the scale would round below one device pixel (a window far longer
/// than it is wide, lowered to fit) is drawn one pixel across, so the
/// window still draws.
pub(crate) fn metrics(content: Size<f64>, scale: BufferScale) -> Option<WindowMetrics> {
    let least = 1.0 / scale.get();
    let side = |logical: f64| {
        if logical > 0.0 {
            logical.max(least)
        } else {
            logical
        }
    };
    let content = Size::new(side(content.width), side(content.height));
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

/// The scale a window's buffers are drawn at: the compositor's, fitted so
/// that `drawn`, the logical size of everything drawn at it (the content,
/// and the frame around it where the window draws one), fits the largest
/// surface.
pub(crate) fn buffer_scale(viewported: bool, told: ScaleTold, drawn: Size<f64>) -> BufferScale {
    if !viewported {
        return BufferScale::ONE;
    }
    told.fractional
        .and_then(over_120)
        .or_else(|| told.preferred.filter(|scale| *scale > 0).map(f64::from))
        .or(told.output)
        .and_then(BufferScale::try_new)
        .unwrap_or(BufferScale::ONE)
        .fitted(drawn)
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
            assert_eq!(
                buffer_scale(viewported, told, Size::new(400.0, 300.0)).get(),
                expected,
                "{case}"
            );
        }
    }

    /// A window publishes what the renderer can draw, whatever the
    /// compositor says. Told a scale in the hundreds, a 400 by 300 window is
    /// drawn at the largest scale, not 176 800 pixels across. A configure
    /// whose content at the compositor's scale would pass the largest
    /// surface is drawn at the scale that fits it, so the window still
    /// draws and publishes metrics the renderer accepts. With no viewport
    /// to stretch it, a content past the largest surface publishes none, and
    /// no product of a size and a scale overflows or is published
    /// non-finite.
    #[test]
    fn a_window_publishes_what_a_surface_can_be() {
        let fractional = |numerator| ScaleTold {
            fractional: Some(numerator),
            preferred: None,
            output: None,
        };
        let published = |viewported, numerator, (width, height): (f64, f64)| {
            let content = Size::new(width, height);
            metrics(
                content,
                buffer_scale(viewported, fractional(numerator), content),
            )
            .map(|metrics| {
                (
                    metrics.physical().width,
                    metrics.physical().height,
                    metrics.scale().get(),
                )
            })
        };
        let extent = f64::from(MAX_CONTENT_EXTENT);
        for (case, viewported, numerator, content, expected) in [
            (
                "a scale in the hundreds",
                true,
                442 * 120,
                (400.0, 300.0),
                Some((3_200, 2_400, BufferScale::MAX)),
            ),
            (
                "a window at one",
                true,
                120,
                (400.0, 300.0),
                Some((400, 300, 1.0)),
            ),
            (
                "a fractional scale",
                true,
                180,
                (400.0, 300.0),
                Some((600, 450, 1.5)),
            ),
            (
                "the widest content at the largest scale",
                true,
                960,
                (extent / BufferScale::MAX, 300.0),
                Some((MAX_CONTENT_EXTENT, 2_400, BufferScale::MAX)),
            ),
            (
                "a configure past the largest surface at the largest scale",
                true,
                960,
                (4_096.0, 1_024.0),
                Some((MAX_CONTENT_EXTENT, 4_096, 4.0)),
            ),
            (
                "a configure past the largest surface at one",
                true,
                120,
                (32_768.0, 100.0),
                Some((MAX_CONTENT_EXTENT, 50, 0.5)),
            ),
            (
                "a configure past the largest surface with no viewport",
                false,
                120,
                (extent + 1.0, 300.0),
                None,
            ),
            (
                "the largest configure at the largest scale",
                true,
                960,
                (f64::from(i32::MAX), f64::from(i32::MAX)),
                Some((
                    MAX_CONTENT_EXTENT,
                    MAX_CONTENT_EXTENT,
                    extent / f64::from(i32::MAX),
                )),
            ),
            (
                "a sliver far longer than the cap's ratio",
                true,
                120,
                (1e6, 1.0),
                Some((MAX_CONTENT_EXTENT, 1, extent / 1e6)),
            ),
            ("a size that is none", true, 120, (0.0, 300.0), None),
            (
                "a size that is not finite",
                true,
                120,
                (f64::NAN, 300.0),
                None,
            ),
        ] {
            assert_eq!(
                published(viewported, numerator, content),
                expected,
                "{case}"
            );
        }
    }
}
