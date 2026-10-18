//! The rules of the Windows display source that need no Win32 call: how a rate
//! the OS reports becomes a [`RefreshPeriod`], and which sampled display a
//! window's nearest monitor is.
//!
//! They live apart from `platforms::windows::display` so every host runs their
//! tests.

use flui_platform_api::{DisplayId, DisplayKey, RefreshPeriod};

/// The period of the rational rate `QueryDisplayConfig` reports for a target
/// (`DISPLAYCONFIG_RATIONAL`), kept exact: 60000/1001 stays 16.683 ms. `None`
/// for a zero numerator or denominator (a path that carries no rate) and for a
/// rate outside 1 to 1000 Hz.
pub(crate) fn rational_refresh_period(numerator: u32, denominator: u32) -> Option<RefreshPeriod> {
    RefreshPeriod::from_ratio(numerator, denominator).ok()
}

/// The period of `dmDisplayFrequency`, the whole-hertz rate
/// `EnumDisplaySettingsW` reports. Zero and one are the documented "hardware
/// default" values, not a cadence, so they are unknown.
pub(crate) fn frequency_period(hertz: u32) -> Option<RefreshPeriod> {
    match hertz {
        0 | 1 => None,
        hertz => RefreshPeriod::from_ratio(hertz, 1).ok(),
    }
}

/// One monitor of a sample: the handle `EnumDisplayMonitors` reported for it
/// and the identity of its display. The handle is only meaningful inside the
/// sample that produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SampledMonitor {
    pub(crate) handle: isize,
    pub(crate) key: DisplayKey,
}

/// The display `nearest` names, found by the native key of the sampled monitor
/// with that handle and issued through `mint`. `None` when the sample holds no
/// such monitor or `mint` refuses its key.
pub(crate) fn display_of_monitor(
    nearest: isize,
    sample: &[SampledMonitor],
    mint: impl FnOnce(&DisplayKey) -> Option<DisplayId>,
) -> Option<DisplayId> {
    let monitor = sample.iter().find(|monitor| monitor.handle == nearest)?;
    mint(&monitor.key)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use flui_platform_api::{DisplayKey, DisplayRegistry, RefreshPeriod};

    use super::{SampledMonitor, display_of_monitor, frequency_period, rational_refresh_period};

    fn target(target_id: u32) -> DisplayKey {
        DisplayKey::WindowsTarget {
            adapter_luid: 7,
            target_id,
        }
    }

    fn within_a_nanosecond(a: Duration, b: Duration) -> bool {
        a.abs_diff(b) <= Duration::from_nanos(1)
    }

    fn windows_zero_or_one_hz_is_unknown() {
        assert_eq!(frequency_period(0), None);
        assert_eq!(frequency_period(1), None, "1 Hz is the hardware default");
        assert_eq!(
            frequency_period(2).map(RefreshPeriod::get),
            Some(Duration::from_millis(500))
        );
        assert_eq!(
            frequency_period(60).map(RefreshPeriod::get),
            Some(Duration::from_nanos(16_666_667))
        );
        assert_eq!(frequency_period(u32::MAX), None, "beyond 1000 Hz");
    }

    fn rational_refresh_converts_exactly() {
        let ntsc = rational_refresh_period(60_000, 1001).expect("59.94 Hz");
        assert!(
            within_a_nanosecond(ntsc.get(), Duration::from_nanos(16_683_333)),
            "59.94 Hz is {:?}, not 1/60 s",
            ntsc.get()
        );
        for (hertz, nanos) in [(144, 6_944_444), (165, 6_060_606)] {
            let period = rational_refresh_period(hertz, 1).expect("whole rate");
            assert!(
                within_a_nanosecond(period.get(), Duration::from_nanos(nanos)),
                "{hertz} Hz is {:?}",
                period.get()
            );
        }
        assert_eq!(rational_refresh_period(60, 0), None, "zero denominator");
        assert_eq!(rational_refresh_period(0, 1), None, "zero numerator");
        assert_eq!(rational_refresh_period(0, 0), None, "a path with no rate");
    }

    fn nearest_monitor_resolves_by_native_key() {
        let registry = DisplayRegistry::new();
        let mint = |key: &DisplayKey| registry.mint(key).ok();
        // Two mirrored outputs: bounds would not tell them apart, the key does.
        // The handles are arbitrary and unordered.
        let sample = [
            SampledMonitor {
                handle: 0x4_0001,
                key: target(4352),
            },
            SampledMonitor {
                handle: 0x2_0007,
                key: target(4353),
            },
        ];
        let first = registry.mint(&target(4352)).expect("id");
        let second = registry.mint(&target(4353)).expect("id");
        assert_ne!(first, second);
        assert_eq!(display_of_monitor(0x4_0001, &sample, mint), Some(first));
        assert_eq!(display_of_monitor(0x2_0007, &sample, mint), Some(second));
        assert_eq!(
            display_of_monitor(0x9_9999, &sample, mint),
            None,
            "a handle outside the sample is an unknown display, not the primary"
        );
        assert_eq!(display_of_monitor(0x4_0001, &[], mint), None);
        assert_eq!(
            display_of_monitor(0x4_0001, &sample, |_| None),
            None,
            "a refused key leaves the window without a display"
        );
    }

    #[test]
    fn os_display_classification() {
        crate::table_test::run_table(
            "os_display_classification",
            &[
                (
                    "windows_zero_or_one_hz_is_unknown",
                    windows_zero_or_one_hz_is_unknown as fn(),
                ),
                (
                    "rational_refresh_converts_exactly",
                    rational_refresh_converts_exactly as fn(),
                ),
                (
                    "nearest_monitor_resolves_by_native_key",
                    nearest_monitor_resolves_by_native_key as fn(),
                ),
            ],
        );
    }
}
