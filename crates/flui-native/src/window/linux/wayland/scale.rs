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
//! and the renderer configures no surface past [`MAX_SURFACE_EXTENT`]
//! device pixels on a side: so the scale is lowered further until the
//! content fits ([`BufferScale::fitted`]), and the window still draws, at
//! fewer pixels the viewport stretches. A window's metrics are
//! [`metrics`]'s, which publishes none it cannot draw: a content past the
//! extent with no viewport to stretch it, or one that is no size.

#![forbid(unsafe_code)]

use flui_foundation::geometry::{DevicePixelRatio, Size, WindowMetrics};
use flui_platform::MAX_SURFACE_EXTENT;

use crate::display::scale::{RefusedScale, ScaleStreak};

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

    /// The scale a buffer is drawn at where the compositor names `told`,
    /// lowered to [`Self::MAX`] past it.
    pub(crate) fn of(told: DevicePixelRatio) -> Self {
        if told.get() > Self::MAX {
            Self::LARGEST
        } else {
            Self(told)
        }
    }

    /// [`Self::MAX`] as a buffer scale.
    const LARGEST: Self = Self(match DevicePixelRatio::try_new(Self::MAX) {
        Some(ratio) => ratio,
        None => panic!("BUG: the largest buffer scale is a ratio a screen can have"),
    });

    /// Device pixels per logical pixel.
    pub(crate) const fn get(self) -> f64 {
        self.0.get()
    }

    /// The scale as a device pixel ratio.
    pub(crate) const fn ratio(self) -> DevicePixelRatio {
        self.0
    }

    /// This scale, lowered where `content` logical pixels drawn at it would
    /// pass [`MAX_SURFACE_EXTENT`] device pixels on a side, so they fit, but
    /// never below [`DevicePixelRatio::MIN`]: a content too long to fit at it
    /// does not fit at all, and [`metrics`] publishes none; this scale where
    /// the content is no size.
    pub(crate) fn fitted(self, content: Size<f64>) -> Self {
        let longest = content.width.max(content.height);
        if !(longest.is_finite() && longest > 0.0) {
            return self;
        }
        let fits = f64::from(MAX_SURFACE_EXTENT) / longest;
        if self.get() <= fits {
            return self;
        }
        DevicePixelRatio::try_new(fits.max(DevicePixelRatio::MIN.get())).map_or(self, Self)
    }
}

/// The metrics of a window whose content is `content` logical pixels
/// drawn at `scale`, which [`buffer_scale`] fitted to it. `None` where it
/// has no drawable area: an extent that is no size, or one past
/// [`MAX_SURFACE_EXTENT`] in device pixels, which a window with no
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
    let fits = |side: i32| u32::try_from(side).is_ok_and(|side| side <= MAX_SURFACE_EXTENT);
    (fits(physical.width) && fits(physical.height)).then_some(metrics)
}

/// One scale the compositor tells a window after it opens, kept by the hosts'
/// one rule (`display::scale::ScaleStreak::check`): a value that is a ratio a
/// screen can have replaces the one held, and one that is not is refused and
/// changes nothing, so the window keeps the scale it had, as the Win32 window
/// does at a `WM_DPICHANGED` it refuses. `T` is the number as the protocol
/// carries it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ToldScale<T> {
    held: Option<(T, DevicePixelRatio)>,
    streak: ScaleStreak,
}

impl<T> Default for ToldScale<T> {
    fn default() -> Self {
        Self {
            held: None,
            streak: ScaleStreak::default(),
        }
    }
}

impl<T: Copy> ToldScale<T> {
    /// The compositor told `raw`, which is the ratio `ratio`; `what` names the
    /// event. The refusal to report, with nothing borrowed, when this one
    /// starts a run of them.
    #[must_use = "a refused scale is reported, with nothing borrowed"]
    pub(crate) fn tell(&mut self, raw: T, ratio: f64, what: &'static str) -> Option<RefusedScale> {
        match self.streak.check(ratio, what) {
            Ok(ratio) => {
                self.held = Some((raw, ratio));
                None
            }
            Err(refused) => refused,
        }
    }

    /// The ratio held: the last one told that was a ratio.
    pub(crate) fn ratio(&self) -> Option<DevicePixelRatio> {
        self.held.map(|(_, ratio)| ratio)
    }

    /// The number held, as the protocol carried it.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn raw(&self) -> Option<T> {
        self.held.map(|(raw, _)| raw)
    }
}

/// What the compositor said of the scale a surface is drawn at, each a ratio
/// a screen can have by its type.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct ScaleTold {
    /// `wp_fractional_scale_v1.preferred_scale`.
    pub(crate) fractional: Option<DevicePixelRatio>,
    /// `wl_surface.preferred_buffer_scale`.
    pub(crate) preferred: Option<DevicePixelRatio>,
    /// The scale of the output the surface is on.
    pub(crate) output: Option<DevicePixelRatio>,
}

/// The scale a window's buffers are drawn at: the compositor's (the
/// fractional scale, else the preferred one, else the output's), fitted so
/// that `drawn`, the logical size of everything drawn at it (the content,
/// and the frame around it where the window draws one), fits the largest
/// surface.
pub(crate) fn buffer_scale(viewported: bool, told: ScaleTold, drawn: Size<f64>) -> BufferScale {
    if !viewported {
        return BufferScale::ONE;
    }
    told.fractional
        .or(told.preferred)
        .or(told.output)
        .map_or(BufferScale::ONE, BufferScale::of)
        .fitted(drawn)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a window holds after the compositor told it `fractional` (each a
    /// numerator over 120) and `preferred` in turn, on an output of scale
    /// `output`: each event through the hosts' one rule, as the window's
    /// handlers send it.
    fn told(fractional: &[u32], preferred: &[i32], output: Option<f64>) -> ScaleTold {
        let mut held_fractional = ToldScale::default();
        for &numerator in fractional {
            let _ = held_fractional.tell(numerator, f64::from(numerator) / 120.0, "test");
        }
        let mut held_preferred = ToldScale::default();
        for &factor in preferred {
            let _ = held_preferred.tell(factor, f64::from(factor), "test");
        }
        ScaleTold {
            fractional: held_fractional.ratio(),
            preferred: held_preferred.ratio(),
            output: output.map(|scale| DevicePixelRatio::try_new(scale).expect("a ratio")),
        }
    }

    /// The fractional scale wins over the whole ones, and each falls back
    /// to the next where the compositor said nothing usable. A scale the
    /// compositor tells that no screen has is refused and the window keeps
    /// the one it held, so its buffer scale does not change.
    #[test]
    fn a_buffer_is_drawn_at_the_scale_the_compositor_prefers() {
        for (case, viewported, told, expected) in [
            ("nothing said", true, told(&[], &[], None), 1.0),
            (
                "a fractional scale",
                true,
                told(&[180], &[2], Some(2.0)),
                1.5,
            ),
            (
                "a whole preferred scale",
                true,
                told(&[], &[2], Some(1.0)),
                2.0,
            ),
            ("the output's", true, told(&[], &[], Some(3.0)), 3.0),
            ("a zero fraction", true, told(&[0], &[2], None), 2.0),
            (
                "a zero preferred scale",
                true,
                told(&[], &[0], Some(1.25)),
                1.25,
            ),
            ("no viewporter", false, told(&[180], &[2], Some(2.0)), 1.0),
            ("a numerator over 120", true, told(&[150], &[], None), 1.25),
            // A compositor names any scale. Past the largest buffer scale
            // and within the range a screen can have, the buffer is drawn at
            // the largest; past the range it is refused, as one the
            // compositor did not say.
            (
                "a fraction past the buffer cap",
                true,
                told(&[30 * 120], &[], None),
                BufferScale::MAX,
            ),
            (
                "a fraction below the smallest falls back to the preferred",
                true,
                told(&[1], &[2], Some(3.0)),
                2.0,
            ),
            (
                "a fraction in the hundreds falls back to the output's",
                true,
                told(&[442 * 120], &[], Some(3.0)),
                3.0,
            ),
            (
                "the largest fraction the protocol carries",
                true,
                told(&[u32::MAX], &[], None),
                1.0,
            ),
            (
                "the largest whole scale the protocol carries",
                true,
                told(&[], &[i32::MAX], None),
                1.0,
            ),
            (
                "a refused fraction keeps the one held",
                true,
                told(&[180, 0], &[2], Some(3.0)),
                1.5,
            ),
            (
                "a fraction past the range keeps the one held",
                true,
                told(&[180, u32::MAX], &[2], Some(3.0)),
                1.5,
            ),
            (
                "a refused preferred scale keeps the one held",
                true,
                told(&[], &[2, 0], Some(3.0)),
                2.0,
            ),
            (
                "a scale told after a refusal is taken",
                true,
                told(&[180, 0, 240], &[], None),
                2.0,
            ),
        ] {
            assert_eq!(
                buffer_scale(viewported, told, Size::new(400.0, 300.0)).get(),
                expected,
                "{case}"
            );
        }
    }

    /// A refused scale is reported once per run of them, and a scale taken
    /// ends the run.
    #[test]
    fn a_run_of_refused_told_scales_is_reported_once() {
        let mut held = ToldScale::default();
        let refused: Vec<bool> = [180, 0, 0, u32::MAX, 240, 0]
            .into_iter()
            .map(|numerator| {
                held.tell(numerator, f64::from(numerator) / 120.0, "test")
                    .is_some()
            })
            .collect();
        assert_eq!(refused, [false, true, false, false, false, true]);
        assert_eq!(held.raw(), Some(240), "the last scale taken is held");
    }

    /// A window publishes what the renderer can draw, whatever the
    /// compositor says. Told a scale past the buffer cap, a 400 by 300 window is
    /// drawn at the largest buffer scale, not 12 000 pixels across. A configure
    /// whose content at the compositor's scale would pass the largest
    /// surface is drawn at the scale that fits it, so the window still
    /// draws and publishes metrics the renderer accepts. With no viewport
    /// to stretch it, a content past the largest surface publishes none, and
    /// no product of a size and a scale overflows or is published
    /// non-finite.
    #[test]
    fn a_window_publishes_what_a_surface_can_be() {
        let fractional = |numerator| told(&[numerator], &[], None);
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
        let extent = f64::from(MAX_SURFACE_EXTENT);
        let max = MAX_SURFACE_EXTENT.cast_signed();
        for (case, viewported, numerator, content, expected) in [
            (
                "a scale past the buffer cap",
                true,
                30 * 120,
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
                Some((max, 2_400, BufferScale::MAX)),
            ),
            (
                "a configure past the largest surface at the largest scale",
                true,
                960,
                (4_096.0, 1_024.0),
                Some((max, 4_096, 4.0)),
            ),
            (
                "a configure past the largest surface at one",
                true,
                120,
                (32_768.0, 100.0),
                Some((max, 50, 0.5)),
            ),
            (
                "a configure past the largest surface with no viewport",
                false,
                120,
                (extent + 1.0, 300.0),
                None,
            ),
            (
                "the longest content the smallest ratio fits",
                true,
                120,
                (extent / DevicePixelRatio::MIN.get(), 1.0),
                Some((max, 1, DevicePixelRatio::MIN.get())),
            ),
            (
                "a configure too long to fit at the smallest ratio",
                true,
                120,
                (1e6, 1.0),
                None,
            ),
            (
                "the largest configure at the largest scale",
                true,
                960,
                (f64::from(i32::MAX), f64::from(i32::MAX)),
                None,
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
