//! The rules of the Android display source that need no JVM: what a sample of a
//! `Display` becomes in the snapshot, how a reported refresh rate becomes a
//! period, how long the idle loop waits for it, and what rate a window asks for.

use std::time::Duration;

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform_api::{DisplayId, DisplayInfo, FrameRateHint, RefreshPeriod};

/// How long the loop waits for an event when nothing asks for a frame and the
/// display's period is unknown: one frame of a 60 Hz display, rounded down.
const FALLBACK_IDLE_WAIT: Duration = Duration::from_millis(16);

/// What one `android.view.Display` reported, before it has an identity.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawDisplay {
    /// `Display.getDisplayId`.
    pub(crate) display_id: i32,
    /// `Display.getName`.
    pub(crate) name: String,
    /// The real size in pixels (`getRealMetrics`), in the current rotation.
    pub(crate) width: i32,
    /// See `width`.
    pub(crate) height: i32,
    /// `DisplayMetrics.densityDpi`.
    pub(crate) density_dpi: i32,
    /// The rate of the current mode, in hertz (`Display.getMode`, or
    /// `getRefreshRate` when the mode says nothing).
    pub(crate) refresh_hz: f32,
    /// The highest rate among `getSupportedModes`.
    pub(crate) max_refresh_hz: Option<f32>,
}

impl RawDisplay {
    /// `Display.DEFAULT_DISPLAY`, the built-in screen.
    pub(crate) const DEFAULT_DISPLAY_ID: i32 = 0;

    /// The snapshot entry for this display under `id`; `None` for a display
    /// that reports no size or density, which nothing could be laid out on.
    pub(crate) fn info(&self, id: DisplayId) -> Option<DisplayInfo> {
        if self.width <= 0 || self.height <= 0 || self.density_dpi <= 0 {
            return None;
        }
        let bounds = Bounds::new(Point::new(0, 0), Size::new(self.width, self.height));
        let mut info = DisplayInfo::new(id, &self.name, bounds, scale_factor(self.density_dpi))
            .with_primary(self.display_id == Self::DEFAULT_DISPLAY_ID);
        info.refresh_period = refresh_period(self.refresh_hz);
        Some(info)
    }
}

/// Device pixels per logical pixel: Android's density is `densityDpi / 160`.
fn scale_factor(density_dpi: i32) -> f64 {
    f64::from(density_dpi) / 160.0
}

/// The period of a rate Android reports. A value that is zero, negative, not a
/// number or outside 1 Hz to 1 kHz describes no cadence and is unknown.
pub(crate) fn refresh_period(hz: f32) -> Option<RefreshPeriod> {
    RefreshPeriod::from_hz(f64::from(hz)).ok()
}

/// The longest the loop blocks waiting for an event when it has nothing to
/// render: the display's period, so that wall-clock deadlines and a frame the
/// system will not announce are noticed within a frame, and 16 ms when the
/// period is unknown.
pub(crate) fn idle_wait(period: Option<RefreshPeriod>) -> Duration {
    period.map_or(FALLBACK_IDLE_WAIT, RefreshPeriod::get)
}

/// How long the loop blocks when it has nothing to render: [`idle_wait`], cut
/// short by the time left to the next wall-clock deadline (`None` when there is
/// none), so a deadline nearer than one frame is not served a frame late.
///
/// The loop keeps waking once per period even when the next deadline is far,
/// instead of blocking until it: the same iteration samples preferences every
/// 500 ms, settles the safe area and consumes redraw requests that set a flag
/// without waking the looper.
pub(crate) fn idle_wait_until(
    period: Option<RefreshPeriod>,
    until_deadline: Option<Duration>,
) -> Duration {
    until_deadline.map_or_else(|| idle_wait(period), |left| idle_wait(period).min(left))
}

/// The highest valid rate among `rates`.
pub(crate) fn max_refresh_hz(rates: impl IntoIterator<Item = f32>) -> Option<f32> {
    rates
        .into_iter()
        .filter(|hz| refresh_period(*hz).is_some())
        .max_by(f32::total_cmp)
}

/// The rate to pass to `ANativeWindow_setFrameRate` for `hint`: the display's
/// highest rate for [`FrameRateHint::High`], `0.0` ("no preference") for the
/// default. `None` when a high rate is wanted and the display's highest is not
/// known, so no request is made rather than a guessed one.
pub(crate) fn requested_frame_rate(hint: FrameRateHint, max_hz: Option<f32>) -> Option<f32> {
    match hint {
        FrameRateHint::High => max_hz,
        _ => Some(0.0),
    }
}

#[cfg(test)]
mod tests {
    use flui_platform_api::{DisplayKey, DisplayRegistry};

    use super::*;

    /// The period of `hz` to within a microsecond, which is all a float rate
    /// carries.
    fn micros(hz: f32) -> Option<u128> {
        refresh_period(hz).map(|period| period.get().as_micros())
    }

    fn android_refresh_value_table() {
        for (hz, expected) in [
            (60.0, Some(16_666)),
            (90.0, Some(11_111)),
            (120.0, Some(8_333)),
            (1000.0, Some(1_000)),
            (1.0, Some(1_000_000)),
            // An old driver reports nothing, or something that is not a rate.
            (0.0, None),
            (-60.0, None),
            (f32::NAN, None),
            (f32::INFINITY, None),
            // Outside 1 ms to 1 s there is no cadence to follow.
            (0.5, None),
            (5000.0, None),
        ] {
            assert_eq!(micros(hz), expected, "a display reporting {hz} Hz");
        }
    }

    fn a_display_without_size_or_density_is_not_described() {
        let registry = DisplayRegistry::new();
        let id = registry.mint(&DisplayKey::AndroidId(0)).expect("id");
        let healthy = RawDisplay {
            display_id: 0,
            name: "Built-in".into(),
            width: 1080,
            height: 2400,
            density_dpi: 420,
            refresh_hz: 120.0,
            max_refresh_hz: Some(120.0),
        };
        let info = healthy.info(id).expect("a healthy display");
        assert_eq!(info.scale_factor, 2.625, "420 dpi is 2.625 logical pixels");
        assert!(info.is_primary, "display 0 is the built-in screen");
        assert_eq!(info.bounds.size, Size::new(1080, 2400));
        assert_eq!(
            info.refresh_period.map(|period| period.get().as_micros()),
            Some(8_333)
        );
        for broken in [
            RawDisplay {
                width: 0,
                ..healthy.clone()
            },
            RawDisplay {
                height: -1,
                ..healthy.clone()
            },
            RawDisplay {
                density_dpi: 0,
                ..healthy.clone()
            },
        ] {
            assert_eq!(broken.info(id), None, "{broken:?}");
        }
        let external = RawDisplay {
            display_id: 2,
            refresh_hz: 0.0,
            ..healthy
        };
        let info = external.info(id).expect("an external display");
        assert!(!info.is_primary);
        assert_eq!(info.refresh_period, None, "an unknown rate stays unknown");
    }

    fn highest_rate_ignores_values_that_are_not_rates() {
        assert_eq!(max_refresh_hz([60.0, 120.0, 90.0]), Some(120.0));
        assert_eq!(max_refresh_hz([60.0, f32::NAN, 0.0, 1e9]), Some(60.0));
        assert_eq!(max_refresh_hz([f32::NAN, -1.0]), None);
        assert_eq!(max_refresh_hz([]), None);
    }

    fn the_requested_rate_follows_the_hint() {
        assert_eq!(
            requested_frame_rate(FrameRateHint::High, Some(120.0)),
            Some(120.0)
        );
        assert_eq!(
            requested_frame_rate(FrameRateHint::High, None),
            None,
            "no request is guessed when the highest rate is unknown"
        );
        assert_eq!(
            requested_frame_rate(FrameRateHint::Default, Some(120.0)),
            Some(0.0),
            "the default withdraws the request"
        );
        assert_eq!(
            requested_frame_rate(FrameRateHint::Default, None),
            Some(0.0)
        );
    }

    #[test]
    fn android_display_table() {
        crate::table_test::run_table(
            "android_display_table",
            &[
                (
                    "android_refresh_value_table",
                    android_refresh_value_table as fn(),
                ),
                (
                    "a_display_without_size_or_density_is_not_described",
                    a_display_without_size_or_density_is_not_described as fn(),
                ),
                (
                    "highest_rate_ignores_values_that_are_not_rates",
                    highest_rate_ignores_values_that_are_not_rates as fn(),
                ),
                (
                    "the_requested_rate_follows_the_hint",
                    the_requested_rate_follows_the_hint as fn(),
                ),
            ],
        );
    }

    #[test]
    fn idle_wait_is_cut_short_by_the_wake_deadline() {
        let period = refresh_period(60.0);
        let wait = |left: Option<Duration>| idle_wait_until(period, left);
        assert_eq!(wait(None).as_micros(), 16_666, "no deadline: one period");
        assert_eq!(
            wait(Some(Duration::from_millis(3))),
            Duration::from_millis(3),
            "a deadline nearer than a frame is not served a frame late"
        );
        assert_eq!(wait(Some(Duration::ZERO)), Duration::ZERO, "already due");
        assert_eq!(
            wait(Some(Duration::from_secs(5))).as_micros(),
            16_666,
            "a far deadline does not stretch the wait"
        );
    }

    /// The idle wait is the period the display runs at, not a fixed 16 ms.
    #[test]
    fn idle_wait_follows_the_real_period() {
        let wait = |hz: f32| idle_wait(refresh_period(hz));
        assert_eq!(
            wait(120.0).as_micros(),
            8_333,
            "a 120 Hz display was polled every 16 ms"
        );
        assert_eq!(wait(90.0).as_micros(), 11_111);
        assert_eq!(wait(60.0).as_micros(), 16_666);
        assert_eq!(
            idle_wait(None),
            Duration::from_millis(16),
            "an unknown period keeps the 60 Hz cadence"
        );
        assert_eq!(wait(0.0), Duration::from_millis(16));
    }
}
