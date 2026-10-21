//! The refresh period the macOS display source hands to the display snapshot.
//!
//! The AppKit backend read a current-mode rate (CoreGraphics, in hertz) before
//! the display snapshot existed. This function is the one place that rate
//! becomes a [`RefreshPeriod`], so the snapshot carries what
//! `PlatformWindow::refresh_period` answered for the same display and a 120 Hz
//! panel does not fall back to 60 Hz.

use flui_platform::RefreshPeriod;

/// The period of a CoreGraphics refresh rate in hertz; `None` for a rate that
/// does not describe a cadence (zero — CoreGraphics reports 0 for modes it
/// cannot describe — negative, NaN, infinite, or outside 1 to 1000 Hz).
#[cfg(any(target_os = "macos", test))]
pub(crate) fn core_graphics_refresh_period(hz: f64) -> Option<RefreshPeriod> {
    RefreshPeriod::from_hz(hz).ok()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use flui_platform::RefreshPeriod;

    use super::core_graphics_refresh_period;

    /// What `PlatformWindow::refresh_period` answered before the display
    /// snapshot, for the macOS backend.
    fn legacy_macos(hz: f64) -> Option<Duration> {
        if !hz.is_finite() || hz <= 0.0 {
            return None;
        }
        Some(Duration::from_secs_f64(1.0 / hz))
    }

    fn within_a_nanosecond(a: Duration, b: Duration) -> bool {
        a.abs_diff(b) <= Duration::from_nanos(1)
    }

    #[test]
    fn macos_shim_keeps_the_period_it_reports_today() {
        // Common panel rates, including the fractional NTSC ones.
        for millihertz in [
            24_000, 30_000, 48_000, 50_000, 59_940, 60_000, 75_000, 90_000, 100_000, 119_880,
            120_000, 144_000, 165_000, 240_000, 360_000,
        ] {
            let hz = f64::from(millihertz) / 1000.0;
            let shim = core_graphics_refresh_period(hz).map(RefreshPeriod::get);
            let legacy = legacy_macos(hz);
            assert!(
                shim.zip(legacy)
                    .is_some_and(|(shim, legacy)| within_a_nanosecond(shim, legacy)),
                "macOS {hz} Hz: shim {shim:?}, legacy {legacy:?}"
            );
        }
        // What the backend reports no cadence for stays unknown.
        for hz in [0.0, -60.0, f64::NAN, f64::INFINITY] {
            assert_eq!(core_graphics_refresh_period(hz), None, "{hz}");
        }
    }
}
