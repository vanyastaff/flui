//! How a window's inner-size limits, kept in logical pixels, become the track
//! limits Win32 asks for in `WM_GETMINMAXINFO`, which are outer sizes in
//! physical pixels at the window's current DPI.
//!
//! The rules need no Win32 call, so every host runs their tests: the frame the
//! limits are measured with is an input (the backend reads it from
//! `AdjustWindowRectExForDpi`), and the message's rectangle is a plain value.

use flui_foundation::geometry::Size;

/// The DPI Windows reports for a window it could not measure.
const BASE_DPI: u32 = 96;

/// A size in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PixelSize {
    pub(crate) width: i32,
    pub(crate) height: i32,
}

impl PixelSize {
    pub(crate) const fn new(width: i32, height: i32) -> Self {
        Self { width, height }
    }

    fn component_max(self, other: Self) -> Self {
        Self::new(self.width.max(other.width), self.height.max(other.height))
    }

    fn component_min(self, other: Self) -> Self {
        Self::new(self.width.min(other.width), self.height.min(other.height))
    }
}

/// The outer-size limits of one window at one DPI, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TrackLimits {
    pub(crate) min: Option<PixelSize>,
    pub(crate) max: Option<PixelSize>,
}

impl TrackLimits {
    /// `size` held within the limits: raised to the minimum, then cut to the
    /// maximum (which is never below the minimum).
    pub(crate) fn clamp(self, size: PixelSize) -> PixelSize {
        let raised = self.min.map_or(size, |min| size.component_max(min));
        self.max.map_or(raised, |max| raised.component_min(max))
    }
}

/// The inner-size limits as outer sizes at `dpi`.
///
/// `frame` is the width and height the window's non-client area adds to its
/// client area at that DPI. A minimum rounds up and a maximum rounds down, so
/// the client area never ends outside the logical limits; a limit that is not
/// finite is no limit. When rounding leaves the maximum below the minimum (a
/// fractional limit with both equal), the window is pinned to the minimum.
/// A `dpi` of zero, which Windows reports for a handle it cannot measure, is
/// the base DPI.
pub(crate) fn track_limits(
    min: Option<Size<f64>>,
    max: Option<Size<f64>>,
    dpi: u32,
    frame: PixelSize,
) -> TrackLimits {
    let scale = f64::from(if dpi == 0 { BASE_DPI } else { dpi }) / f64::from(BASE_DPI);
    let outer = |size: Size<f64>, round: fn(f64) -> f64| {
        let component = |logical: f64, frame: i32| {
            (logical.is_finite() && logical > 0.0)
                .then(|| round(logical * scale))
                // `as` saturates at the `i32` range, which the frame then
                // adds to without wrapping.
                .map(|device| (device as i32).saturating_add(frame))
        };
        Some(PixelSize::new(
            component(size.width, frame.width)?,
            component(size.height, frame.height)?,
        ))
    };
    let min = min.and_then(|size| outer(size, f64::ceil));
    let max = max
        .and_then(|size| outer(size, f64::floor))
        .map(|max| min.map_or(max, |min| max.component_max(min)));
    TrackLimits { min, max }
}

/// The rectangle `WM_GETMINMAXINFO` carries, as the OS filled it in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MinMaxInfo {
    /// The size of the window when maximized.
    pub(crate) max_size: PixelSize,
    /// The smallest size the user can drag the window to.
    pub(crate) min_track: PixelSize,
    /// The largest size the user can drag the window to.
    pub(crate) max_track: PixelSize,
}

/// `info` with the window's limits applied.
///
/// The minimum can only raise the OS minimum, never lower it below what the
/// frame needs. The maximum caps both the drag size and the maximized size, so
/// a window with a maximum does not grow past it by maximizing. A fullscreen
/// window covers its monitor whatever its limits say; the caller gives it none.
pub(crate) fn constrain(info: MinMaxInfo, limits: TrackLimits) -> MinMaxInfo {
    let mut constrained = info;
    if let Some(min) = limits.min {
        constrained.min_track = info.min_track.component_max(min);
    }
    if let Some(max) = limits.max {
        // A maximum below the smallest size the window can have is that size.
        let max = max.component_max(constrained.min_track);
        constrained.max_track = max;
        constrained.max_size = info.max_size.component_min(max);
    }
    constrained
}

#[cfg(test)]
mod tests {
    use flui_foundation::geometry::Size;

    use super::{MinMaxInfo, PixelSize, constrain, track_limits};

    const FRAME: PixelSize = PixelSize::new(16, 39);

    fn px(width: i32, height: i32) -> PixelSize {
        PixelSize::new(width, height)
    }

    fn os_default() -> MinMaxInfo {
        MinMaxInfo {
            max_size: px(1936, 1056),
            min_track: px(136, 60),
            max_track: px(3872, 2112),
        }
    }

    fn base_dpi_adds_the_frame_to_whole_limits() {
        let limits = track_limits(
            Some(Size::new(300.0, 200.0)),
            Some(Size::new(900.0, 700.0)),
            96,
            FRAME,
        );
        assert_eq!(limits.min, Some(px(316, 239)));
        assert_eq!(limits.max, Some(px(916, 739)));
    }

    fn a_scaled_minimum_rounds_up_and_a_maximum_rounds_down() {
        // 150 %: 100.3 logical is 150.45 device pixels.
        let limits = track_limits(
            Some(Size::new(100.3, 100.3)),
            Some(Size::new(500.3, 500.3)),
            144,
            px(0, 0),
        );
        assert_eq!(
            limits.min,
            Some(px(151, 151)),
            "the client never ends below the minimum"
        );
        assert_eq!(
            limits.max,
            Some(px(750, 750)),
            "the client never ends above the maximum"
        );
    }

    fn equal_fractional_limits_pin_the_window_to_the_minimum() {
        let size = Size::new(100.5, 100.5);
        let limits = track_limits(Some(size), Some(size), 96, px(0, 0));
        assert_eq!(limits.min, Some(px(101, 101)));
        assert_eq!(
            limits.max,
            Some(px(101, 101)),
            "rounding must not leave the maximum below the minimum"
        );
    }

    fn a_limit_that_is_not_a_size_is_no_limit() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -5.0] {
            let limits = track_limits(
                Some(Size::new(bad, 100.0)),
                Some(Size::new(100.0, bad)),
                96,
                FRAME,
            );
            assert_eq!((limits.min, limits.max), (None, None), "{bad}");
        }
    }

    fn a_huge_limit_saturates_instead_of_wrapping() {
        let limits = track_limits(None, Some(Size::new(1.0e300, 1.0e300)), 192, FRAME);
        assert_eq!(limits.max, Some(px(i32::MAX, i32::MAX)));
        let limits = track_limits(Some(Size::new(f64::MAX, 10.0)), None, 96, FRAME);
        assert_eq!(limits.min, Some(px(i32::MAX, 49)));
    }

    fn an_unmeasured_window_is_at_the_base_dpi() {
        let at = |dpi| track_limits(Some(Size::new(100.0, 100.0)), None, dpi, px(0, 0));
        assert_eq!(at(0), at(96));
        assert_ne!(at(96), at(192));
    }

    fn the_minimum_only_raises_the_os_minimum() {
        let raised = constrain(
            os_default(),
            track_limits(Some(Size::new(400.0, 300.0)), None, 96, FRAME),
        );
        assert_eq!(raised.min_track, px(416, 339));
        assert_eq!(raised.max_track, os_default().max_track);
        let below = constrain(
            os_default(),
            track_limits(Some(Size::new(20.0, 10.0)), None, 96, FRAME),
        );
        assert_eq!(
            below.min_track,
            os_default().min_track,
            "a limit below the system minimum cannot lower it"
        );
    }

    fn the_maximum_caps_dragging_and_maximizing() {
        let capped = constrain(
            os_default(),
            track_limits(None, Some(Size::new(800.0, 600.0)), 96, FRAME),
        );
        assert_eq!(capped.max_track, px(816, 639));
        assert_eq!(capped.max_size, px(816, 639));
        let roomy = constrain(
            os_default(),
            track_limits(None, Some(Size::new(5000.0, 5000.0)), 96, FRAME),
        );
        assert_eq!(roomy.max_track, px(5016, 5039));
        assert_eq!(
            roomy.max_size,
            os_default().max_size,
            "a maximum never makes maximizing larger than the monitor allows"
        );
    }

    fn a_size_is_held_within_the_limits() {
        let limits = track_limits(
            Some(Size::new(400.0, 300.0)),
            Some(Size::new(800.0, 600.0)),
            96,
            FRAME,
        );
        assert_eq!(limits.clamp(px(100, 100)), px(416, 339));
        assert_eq!(limits.clamp(px(5000, 5000)), px(816, 639));
        assert_eq!(limits.clamp(px(500, 400)), px(500, 400));
        let none = track_limits(None, None, 96, FRAME);
        assert_eq!(none.clamp(px(1, 1)), px(1, 1));
    }

    fn a_maximum_below_the_smallest_size_is_that_size() {
        let capped = constrain(
            os_default(),
            track_limits(None, Some(Size::new(1.0, 1.0)), 96, px(0, 0)),
        );
        assert_eq!(capped.max_track, os_default().min_track);
        assert_eq!(capped.max_size, os_default().min_track);
        let raised = constrain(
            os_default(),
            track_limits(
                Some(Size::new(400.0, 300.0)),
                Some(Size::new(1.0, 1.0)),
                96,
                px(0, 0),
            ),
        );
        assert!(raised.max_track.width >= raised.min_track.width);
        assert!(raised.max_track.height >= raised.min_track.height);
    }

    fn no_limits_keep_the_os_rectangle() {
        let none = track_limits(None, None, 96, FRAME);
        assert_eq!(constrain(os_default(), none), os_default());
    }

    #[test]
    fn window_limits_in_device_pixels() {
        crate::table_test::run_table(
            "window_limits_in_device_pixels",
            &[
                (
                    "base_dpi_adds_the_frame_to_whole_limits",
                    base_dpi_adds_the_frame_to_whole_limits as fn(),
                ),
                (
                    "a_scaled_minimum_rounds_up_and_a_maximum_rounds_down",
                    a_scaled_minimum_rounds_up_and_a_maximum_rounds_down as fn(),
                ),
                (
                    "equal_fractional_limits_pin_the_window_to_the_minimum",
                    equal_fractional_limits_pin_the_window_to_the_minimum as fn(),
                ),
                (
                    "a_limit_that_is_not_a_size_is_no_limit",
                    a_limit_that_is_not_a_size_is_no_limit as fn(),
                ),
                (
                    "a_huge_limit_saturates_instead_of_wrapping",
                    a_huge_limit_saturates_instead_of_wrapping as fn(),
                ),
                (
                    "an_unmeasured_window_is_at_the_base_dpi",
                    an_unmeasured_window_is_at_the_base_dpi as fn(),
                ),
                (
                    "the_minimum_only_raises_the_os_minimum",
                    the_minimum_only_raises_the_os_minimum as fn(),
                ),
                (
                    "the_maximum_caps_dragging_and_maximizing",
                    the_maximum_caps_dragging_and_maximizing as fn(),
                ),
                (
                    "a_size_is_held_within_the_limits",
                    a_size_is_held_within_the_limits as fn(),
                ),
                (
                    "a_maximum_below_the_smallest_size_is_that_size",
                    a_maximum_below_the_smallest_size_is_that_size as fn(),
                ),
                (
                    "no_limits_keep_the_os_rectangle",
                    no_limits_keep_the_os_rectangle as fn(),
                ),
            ],
        );
    }
}
