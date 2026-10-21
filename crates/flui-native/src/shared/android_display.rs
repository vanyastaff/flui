//! The rules of the Android display source that need no JVM: what a sample of a
//! `Display` becomes in the snapshot, how a reported refresh rate becomes a
//! period, how long the event loop may block, and what rate a window asks for.

use std::time::Duration;

use web_time::Instant;

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform_api::{DisplayId, DisplayInfo, FrameRateHint, RefreshPeriod};

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

/// What one turn of the Android event loop knows when it chooses how long
/// `poll_events` may block.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LoopTurn {
    /// The turn has a frame to deliver: a redraw request or a due deadline,
    /// while resumed.
    pub(crate) should_render: bool,
    /// The Activity is resumed, so a frame can be delivered at all.
    pub(crate) resumed: bool,
    /// The earliest wall-clock deadline the wake hook reported.
    pub(crate) wake_deadline: Option<Instant>,
    /// When the preferences that have no change notification are read next.
    pub(crate) next_preference_sample: Instant,
}

/// The longest wait `ALooper_pollOnce` takes: its timeout is `i32` milliseconds,
/// and `android-activity` converts with `as i32`, so a longer one wraps negative
/// and waits forever.
const LOOPER_MAX_WAIT: Duration = Duration::from_millis(i32::MAX as u64);

/// How long `poll_events` may block; `None` blocks until the looper is woken.
///
/// Every producer of work wakes the looper (a window redraw request, the owner
/// signal, the Java view's event queue, the inset reply, the display watch), so
/// the loop wakes on no timer of its own:
///
/// - a turn with a frame to deliver does not block;
/// - a resumed turn blocks until the next deadline or preference sample;
/// - a paused turn blocks until a deadline that is still ahead, and with none
///   until it is woken. A deadline already behind was offered to the owner this
///   turn and is left waiting for a frame, which nothing delivers before the
///   Activity resumes; preferences are read again on `Resume`.
pub(crate) fn poll_timeout(turn: LoopTurn, now: Instant) -> Option<Duration> {
    if turn.should_render {
        return Some(Duration::ZERO);
    }
    let until = |deadline: Instant| deadline.saturating_duration_since(now).min(LOOPER_MAX_WAIT);
    if turn.resumed {
        let next = turn
            .wake_deadline
            .map_or(turn.next_preference_sample, |deadline| {
                deadline.min(turn.next_preference_sample)
            });
        Some(until(next))
    } else {
        turn.wake_deadline
            .filter(|deadline| *deadline > now)
            .map(until)
    }
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
/// default. A high rate wanted while the display's highest is not known is
/// `0.0` too: no rate is guessed, and a high request made earlier, when the rate
/// was known, is withdrawn rather than left in force.
pub(crate) fn requested_frame_rate(hint: FrameRateHint, max_hz: Option<f32>) -> f32 {
    match hint {
        FrameRateHint::High => max_hz.unwrap_or(0.0),
        _ => 0.0,
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
            120.0
        );
        assert_eq!(
            requested_frame_rate(FrameRateHint::High, None),
            0.0,
            "a highest rate that became unknown withdraws the earlier high request \
             instead of leaving it in force"
        );
        assert_eq!(
            requested_frame_rate(FrameRateHint::Default, Some(120.0)),
            0.0,
            "the default withdraws the request"
        );
        assert_eq!(requested_frame_rate(FrameRateHint::Default, None), 0.0);
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

    /// A paused Activity cannot deliver a frame, so the loop blocks: no timer
    /// of its own, no past deadline turning the poll into a busy loop, and only
    /// a deadline still ahead (a timer the owner serves without a frame) bounds
    /// the wait.
    #[test]
    fn a_paused_loop_blocks_until_it_is_woken() {
        let now = Instant::now();
        let ms = Duration::from_millis;
        let turn = |should_render, resumed, wake_deadline| LoopTurn {
            should_render,
            resumed,
            wake_deadline,
            next_preference_sample: now + ms(500),
        };
        for (case, turn, expected) in [
            (
                "a frame to deliver",
                turn(true, true, None),
                Some(Duration::ZERO),
            ),
            ("resumed and idle", turn(false, true, None), Some(ms(500))),
            (
                "resumed, a deadline before the next sample",
                turn(false, true, Some(now + ms(3))),
                Some(ms(3)),
            ),
            (
                "resumed, a deadline after the next sample",
                turn(false, true, Some(now + ms(5_000))),
                Some(ms(500)),
            ),
            ("paused and idle", turn(false, false, None), None),
            (
                "paused, a deadline that needs a frame is already behind",
                turn(false, false, now.checked_sub(ms(40))),
                None,
            ),
            (
                "paused, a deadline exactly due",
                turn(false, false, Some(now)),
                None,
            ),
            (
                "paused, a timer still ahead",
                turn(false, false, Some(now + ms(250))),
                Some(ms(250)),
            ),
            (
                "paused, a timer further ahead than the looper can wait",
                turn(false, false, Some(now + Duration::from_hours(30 * 24))),
                Some(LOOPER_MAX_WAIT),
            ),
        ] {
            assert_eq!(poll_timeout(turn, now), expected, "{case}");
        }
    }
}
