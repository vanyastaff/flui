//! The Windows displays: the monitors the system reports ([`source`]), what a
//! sample of them holds and answers with no Win32 call ([`sample`]), and the
//! system's display broadcasts, which mark the sample stale (`broadcasts`).
//!
//! The Win32 loop holds the monitors through its `HostDisplays` hook and the
//! broadcast receiver tells [`DisplayBroadcasts`] what the system broadcast;
//! the host's table makes both. The winit host on Windows reads the same
//! monitors through its own loop's hook.
//!
//! `sample` is the pure half, which every host compiles and tests; `source`
//! and `broadcasts` call Win32 and exist only on Windows.

#[cfg(all(windows, test))]
mod message_tests;

#[cfg(windows)]
pub(crate) use broadcasts::DisplayBroadcasts;
#[cfg(windows)]
pub(crate) use source::WindowsDisplays;

#[cfg(any(windows, test))]
#[forbid(unsafe_code)]
pub(crate) mod sample {
    //! The rules of the Windows display source that need no Win32 call: how a rate
    //! the OS reports becomes a [`RefreshPeriod`], which sampled display a
    //! window's nearest monitor is, and from which build the display query is
    //! virtual-mode aware. Every host runs their tests.

    use flui_platform::{DisplayId, DisplayKey, RefreshPeriod};

    use crate::version::gate::Gate;
    use crate::version::gate::windows::WindowsBuild;

    /// `QDC_VIRTUAL_MODE_AWARE`, which `QueryDisplayConfig` accepts from Windows 10
    /// 1903 (build 18362). Without it the active paths are queried without
    /// virtual-mode awareness.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct QdcVirtualModeAware;

    impl Gate for QdcVirtualModeAware {
        type Version = WindowsBuild;
        const SINCE: u64 = 18362;
    }

    /// The period of the rational rate `QueryDisplayConfig` reports for a target
    /// (`DISPLAYCONFIG_RATIONAL`), kept exact: 60000/1001 stays 16.683 ms. `None`
    /// for a zero numerator or denominator (a path that carries no rate) and for a
    /// rate outside 1 to 1000 Hz.
    pub(crate) fn rational_refresh_period(
        numerator: u32,
        denominator: u32,
    ) -> Option<RefreshPeriod> {
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
    /// with that handle and issued through `mint`.
    ///
    /// A sample that lacks the monitor (one plugged in since) answers `None`, never
    /// another display, and runs `on_miss` once so the caller can arrange a fresh
    /// sample; this function samples nothing itself, because it runs while the host
    /// derives window displays from a snapshot it has already accepted. `None` too
    /// when `mint` refuses the key.
    pub(crate) fn resolve_display(
        nearest: isize,
        sample: &[SampledMonitor],
        on_miss: impl FnOnce(),
        mint: impl FnOnce(&DisplayKey) -> Option<DisplayId>,
    ) -> Option<DisplayId> {
        let Some(monitor) = sample.iter().find(|monitor| monitor.handle == nearest) else {
            on_miss();
            return None;
        };
        mint(&monitor.key)
    }

    #[cfg(test)]
    mod tests {
        use std::time::Duration;

        use flui_platform::{DisplayKey, DisplayRegistry, RefreshPeriod};

        use super::{
            QdcVirtualModeAware, SampledMonitor, WindowsBuild, frequency_period,
            rational_refresh_period, resolve_display,
        };

        /// The display query's gate, the row of `version::gate`'s table for the
        /// marker declared here: no proof before build 18362, one from it on, and
        /// none for a build the system could not report.
        #[test]
        fn virtual_mode_gate_table() {
            for (build, aware) in [
                (WindowsBuild::from_number(18361), false),
                (WindowsBuild::from_number(18362), true),
                (WindowsBuild::UNKNOWN, false),
            ] {
                assert_eq!(
                    build.since::<QdcVirtualModeAware>().is_some(),
                    aware,
                    "{build}"
                );
            }
        }

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
            // Two outputs with distinct targets; the handles are arbitrary and not
            // in sample order.
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
            let resolve = |nearest, sample: &[SampledMonitor]| {
                resolve_display(nearest, sample, || panic!("a hit asked for a sample"), mint)
            };
            assert_eq!(resolve(0x4_0001, &sample), Some(first));
            assert_eq!(resolve(0x2_0007, &sample), Some(second));
            assert_eq!(
                resolve_display(0x4_0001, &sample, || (), |_| None),
                None,
                "a refused key leaves the window without a display"
            );
        }

        fn a_monitor_missing_from_the_sample_is_unknown_not_the_primary() {
            let registry = DisplayRegistry::new();
            let sample = [SampledMonitor {
                handle: 0x4_0001,
                key: target(4352),
            }];
            let misses = std::cell::Cell::new(0);
            let answer = resolve_display(
                0x9_9999,
                &sample,
                || misses.set(misses.get() + 1),
                |key| registry.mint(key).ok(),
            );
            assert_eq!(answer, None, "a handle outside the sample named a display");
            assert_eq!(misses.get(), 1, "a miss asks for one fresh sample");
            let empty = resolve_display(0x4_0001, &[], || (), |key| registry.mint(key).ok());
            assert_eq!(empty, None);
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
                    (
                        "a_monitor_missing_from_the_sample_is_unknown_not_the_primary",
                        a_monitor_missing_from_the_sample_is_unknown_not_the_primary as fn(),
                    ),
                ],
            );
        }
    }
}

// Direct Win32 calls have no safe wrapper; every `unsafe` block carries its
// own `// SAFETY:` comment stating the invariant that makes it sound.
#[cfg(windows)]
#[expect(unsafe_code)]
pub(crate) mod source {
    //! Windows display source.
    //!
    //! Samples the monitors with `EnumDisplayMonitors` and turns them into a
    //! [`DisplaySnapshot`]. [`WindowsDisplays`] keeps the last sample and answers
    //! from it until something invalidates it: `WM_DISPLAYCHANGE` or a work-area
    //! change on the owner's broadcast receiver (`DisplayBroadcasts`), or a
    //! window's `WM_DPICHANGED`. The host asks for the snapshot on every owner
    //! turn, so a sample that costs several OS queries is taken when the displays
    //! may have changed, not per turn.
    //!
    //! # Identity
    //!
    //! A monitor is keyed by its display target from
    //! `QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS)`: the adapter LUID with the target
    //! id ([`DisplayKey::WindowsTarget`]). The GDI device name (`\\.\DISPLAYn`) only
    //! pairs a monitor with its path inside one sample, because Windows reuses that
    //! name when the topology changes and a new output would inherit the id of the
    //! one that left. A monitor with no path (a remote session, a driver without
    //! paths, a failed query) is keyed by its monitor interface path
    //! ([`DisplayKey::WindowsInterface`]); one with neither is left out of the
    //! snapshot rather than keyed by a label. A monitor whose path appears in one
    //! sample and not in the next changes key, and so id, between them.
    //!
    //! # Refresh period
    //!
    //! The period is the target's `refreshRate` rational from the same path, kept
    //! exact. A monitor whose path carries no rate (or has no path) falls back to
    //! `EnumDisplaySettingsW`'s whole-hertz `dmDisplayFrequency`, where zero and
    //! one mean unknown.
    //!
    //! # A window's display
    //!
    //! `MonitorFromWindow(MONITOR_DEFAULTTONEAREST)` names a monitor handle, which
    //! is only meaningful inside the sample that listed it. The window's display is
    //! the one whose sampled monitor has that handle, found through its native key.

    use std::num::NonZeroIsize;

    use flui_foundation::geometry::{Bounds, Point, Size};
    use flui_platform::{DisplayId, DisplayInfo, DisplayKey, DisplaySnapshot};
    use parking_lot::Mutex;
    use windows::Win32::{
        Devices::Display::{
            DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_MODE_INFO,
            DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME, DisplayConfigGetDeviceInfo,
            GetDisplayConfigBufferSizes, QDC_ONLY_ACTIVE_PATHS, QDC_VIRTUAL_MODE_AWARE,
            QUERY_DISPLAY_CONFIG_FLAGS, QueryDisplayConfig,
        },
        Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, FALSE, HWND, LPARAM, RECT, TRUE},
        Graphics::Gdi::{
            DEVMODEW, DISPLAY_DEVICE_ACTIVE, DISPLAY_DEVICEW, ENUM_CURRENT_SETTINGS,
            EnumDisplayDevicesW, EnumDisplayMonitors, EnumDisplaySettingsW, GetMonitorInfoW, HDC,
            HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFOEXW, MonitorFromWindow,
        },
        UI::{
            HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
            WindowsAndMessaging::EDD_GET_DEVICE_INTERFACE_NAME,
        },
    };
    // BOOL moved from Win32::Foundation to windows_core in windows 0.62
    use windows::core::{BOOL, PCWSTR};

    use super::sample::{
        QdcVirtualModeAware, SampledMonitor, frequency_period, rational_refresh_period,
        resolve_display,
    };
    use crate::display::source::DisplaySource;
    use crate::error::PlatformError;
    use crate::owner::panic_boundary::{AfterPanic, os_entry};
    use crate::version::gate::Since;
    use crate::version::gate::windows::WindowsBuild;

    /// One platform instance's monitors: the last sample, and whether anything has
    /// invalidated it since (both kept by the [`DisplaySource`]).
    pub struct WindowsDisplays {
        source: DisplaySource,
        build: WindowsBuild,
        /// The handle-to-key pairs of the last sample, which resolve a window's
        /// nearest monitor.
        monitors: Mutex<Vec<SampledMonitor>>,
        #[cfg(test)]
        samples: std::sync::atomic::AtomicUsize,
    }

    impl std::fmt::Debug for WindowsDisplays {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("WindowsDisplays").finish_non_exhaustive()
        }
    }

    impl crate::owner::notices::StaleOnWake for WindowsDisplays {
        fn mark_stale(&self) {
            self.source.invalidate();
        }
    }

    impl crate::owner::windows::HostDisplays for WindowsDisplays {
        /// The last sample, or a fresh one when none was taken or something
        /// invalidated it; a revision space that is used up is retried on the next
        /// call.
        fn snapshot(&self) -> Result<DisplaySnapshot, PlatformError> {
            self.source.snapshot_with_aux(
                || Ok(self.sample()),
                |monitors| *self.monitors.lock() = monitors,
            )
        }

        /// Touches one flag and nothing else, so a window procedure or a foreign
        /// thread may call it at any point.
        fn invalidate(&self) {
            self.source.invalidate();
        }

        /// The one the window's nearest monitor is in the last sample, by native
        /// key.
        ///
        /// Reads the last sample and never takes one: the host derives window
        /// displays from a snapshot it has accepted, and a sample here would publish
        /// a newer revision mid-derivation. `None` for a window with no monitor, a
        /// monitor the sample lacks (a monitor plugged in since), or an output with
        /// no id. A monitor the sample lacks invalidates the snapshot once per
        /// revision, so the next owner turn samples it and the repeated miss costs
        /// nothing.
        fn display_of_window(&self, hwnd: HWND) -> Option<DisplayId> {
            let nearest = crate::owner::windows::HostDisplays::nearest_monitor(self, hwnd)?.get();
            resolve_display(
                nearest,
                &self.monitors.lock(),
                || self.source.note_miss(nearest.cast_unsigned() as u64),
                |key| self.source.mint(key),
            )
        }

        fn nearest_monitor(&self, hwnd: HWND) -> Option<NonZeroIsize> {
            // SAFETY: `MonitorFromWindow` takes the handle by value and dereferences
            // nothing; a destroyed or foreign handle yields a handle or null, never a
            // fault.
            let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
            NonZeroIsize::new(monitor.0 as isize)
        }

        #[cfg(test)]
        fn sample_count(&self) -> usize {
            self.samples.load(std::sync::atomic::Ordering::Relaxed)
        }
    }

    /// The winit host's displays on Windows are these monitors. winit enumerates
    /// monitors only through a live event loop, and keys them by a reusable GDI
    /// device name; this source needs no event loop, keys monitors by display
    /// target, reads the refresh period from `QueryDisplayConfig` and names a
    /// window's display by `MonitorFromWindow`. winit's own monitor list is not
    /// read. The source is invalidated by `WM_DISPLAYCHANGE` and work-area changes
    /// on the loop's broadcast receiver (created on resume) and sampled again where
    /// winit reports a move, a scale change or a resume.
    #[cfg(feature = "winit-backend")]
    impl crate::owner::winit::HostDisplays for WindowsDisplays {
        fn snapshot(&self) -> Result<DisplaySnapshot, PlatformError> {
            crate::owner::windows::HostDisplays::snapshot(self)
        }

        /// The display `window` is on, by the monitor nearest its native window.
        fn id_of_window(&self, window: &winit::window::Window) -> Option<DisplayId> {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};

            let RawWindowHandle::Win32(handle) = window.window_handle().ok()?.as_raw() else {
                return None;
            };
            crate::owner::windows::HostDisplays::display_of_window(
                self,
                HWND(handle.hwnd.get() as *mut _),
            )
        }

        /// Sample the monitors now, which needs neither the event loop nor the
        /// signal: whether the snapshot differs from the one the last sample
        /// published. A revision that cannot advance keeps the previous sample.
        fn resample(
            &self,
            _event_loop: &winit::event_loop::ActiveEventLoop,
            _signal: Option<&std::sync::Arc<crate::owner::signal::OwnerSignal>>,
        ) -> bool {
            let before = self.source.published().revision();
            self.source.invalidate();
            match crate::owner::windows::HostDisplays::snapshot(self) {
                Ok(snapshot) => snapshot.revision() != before,
                Err(error) => {
                    tracing::warn!(
                        %error,
                        "display revisions exhausted; keeping the last snapshot"
                    );
                    false
                }
            }
        }

        fn broadcasts(
            self: std::sync::Arc<Self>,
            signal: &std::sync::Arc<crate::owner::signal::OwnerSignal>,
        ) -> std::rc::Rc<dyn crate::owner::windows::broadcast::Subscriber> {
            let displays: std::sync::Arc<dyn crate::owner::windows::HostDisplays> = self;
            std::rc::Rc::new(super::DisplayBroadcasts::new(&displays, signal))
        }
    }

    impl WindowsDisplays {
        /// A source that has not sampled yet. Reads the OS build once.
        pub(crate) fn new() -> Self {
            Self {
                source: DisplaySource::new(),
                build: WindowsBuild::read(),
                monitors: Mutex::new(Vec::new()),
                #[cfg(test)]
                samples: std::sync::atomic::AtomicUsize::new(0),
            }
        }

        /// Read the monitors now: their displays, and the handles that resolve
        /// them, which are committed only once the displays are published.
        fn sample(&self) -> (Vec<DisplayInfo>, Vec<SampledMonitor>) {
            #[cfg(test)]
            self.samples
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.read()
        }

        /// Enumerate the monitors now: their displays and the handle-to-key pairs
        /// that resolve a window's nearest monitor.
        fn read(&self) -> (Vec<DisplayInfo>, Vec<SampledMonitor>) {
            let paths = query_source_paths(display_config_flags(self.build)).unwrap_or_default();
            let mut displays = Vec::new();
            let mut sampled = Vec::new();
            for monitor in enumerate_monitors() {
                let path = paths
                    .iter()
                    .find(|path| path.gdi_name == monitor.device_name);
                let Some(key) = monitor_key(&monitor.device_name, &paths, || {
                    monitor_interface_path(&monitor.device_name)
                }) else {
                    tracing::warn!(
                        monitor = %monitor.device_name,
                        "a monitor with no display target or interface path is left out of the display snapshot"
                    );
                    continue;
                };
                let Some(id) = self.source.mint(&key) else {
                    continue;
                };
                let mut display = DisplayInfo::new(
                    id,
                    monitor.device_name.clone(),
                    monitor.bounds,
                    monitor.scale_factor,
                )
                .with_usable_bounds(monitor.usable_bounds)
                .with_primary(monitor.is_primary);
                // The monitor's interface path names this monitor on this machine
                // across restarts, so a window remembered on it finds it again.
                if let Some(interface) = monitor_interface_path(&monitor.device_name) {
                    display = display.with_persistent_key(interface);
                }
                display.refresh_period = path
                    .and_then(|path| rational_refresh_period(path.refresh.0, path.refresh.1))
                    .or_else(|| frequency_period(current_mode_frequency(&monitor.device_name)));
                displays.push(display);
                sampled.push(SampledMonitor {
                    handle: monitor.handle,
                    key,
                });
            }
            (displays, sampled)
        }
    }

    /// The `dmDisplayFrequency` of the current mode of the adapter output
    /// `gdi_name`, `0` when the query fails.
    fn current_mode_frequency(gdi_name: &str) -> u32 {
        let name: Vec<u16> = gdi_name.encode_utf16().chain(Some(0)).collect();
        let mut mode = DEVMODEW {
            dmSize: std::mem::size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };
        // SAFETY: `name` is a NUL-terminated UTF-16 string that outlives the call,
        // and `mode` is a `DEVMODEW` with `dmSize` set to its size, which is what
        // `EnumDisplaySettingsW` requires.
        let found = unsafe {
            EnumDisplaySettingsW(PCWSTR(name.as_ptr()), ENUM_CURRENT_SETTINGS, &raw mut mode)
        };
        if found.as_bool() {
            mode.dmDisplayFrequency
        } else {
            0
        }
    }

    /// The display target behind one GDI source, as an active
    /// `QueryDisplayConfig` path reports it.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct SourcePath {
        /// The source's `viewGdiDeviceName`, the name `GetMonitorInfoW` reports.
        gdi_name: String,
        adapter_luid: u64,
        target_id: u32,
        /// The target's `refreshRate`: numerator and denominator, in hertz.
        refresh: (u32, u32),
    }

    /// The identity of the monitor named `gdi_name`: its path's target when a path
    /// names it, otherwise the interface path `interface_path` finds, otherwise
    /// nothing. `interface_path` runs only when no path matches.
    fn monitor_key(
        gdi_name: &str,
        paths: &[SourcePath],
        interface_path: impl FnOnce() -> Option<String>,
    ) -> Option<DisplayKey> {
        if let Some(path) = paths.iter().find(|path| path.gdi_name == gdi_name) {
            return Some(DisplayKey::WindowsTarget {
                adapter_luid: path.adapter_luid,
                target_id: path.target_id,
            });
        }
        interface_path().map(DisplayKey::WindowsInterface)
    }

    /// The `QueryDisplayConfig` flags for the active paths on `build`:
    /// virtual-mode aware where the build has it.
    fn display_config_flags(build: WindowsBuild) -> QUERY_DISPLAY_CONFIG_FLAGS {
        match build.since::<QdcVirtualModeAware>() {
            Some(proof) => QDC_ONLY_ACTIVE_PATHS | QUERY_DISPLAY_CONFIG_FLAGS::from(proof),
            None => QDC_ONLY_ACTIVE_PATHS,
        }
    }

    impl From<Since<QdcVirtualModeAware>> for QUERY_DISPLAY_CONFIG_FLAGS {
        fn from(_proof: Since<QdcVirtualModeAware>) -> Self {
            QDC_VIRTUAL_MODE_AWARE
        }
    }

    /// The attempts `QueryDisplayConfig` gets when the topology grows between
    /// sizing the buffers and filling them.
    const QUERY_ATTEMPTS: usize = 4;

    /// The active display paths with their GDI source names, or `None` when the
    /// query fails (a remote session answers with no paths or an error).
    fn query_source_paths(flags: QUERY_DISPLAY_CONFIG_FLAGS) -> Option<Vec<SourcePath>> {
        for _ in 0..QUERY_ATTEMPTS {
            let mut path_count = 0u32;
            let mut mode_count = 0u32;
            // SAFETY: both out-pointers are valid `u32`s for the call's duration.
            let sized = unsafe {
                GetDisplayConfigBufferSizes(flags, &raw mut path_count, &raw mut mode_count)
            };
            if sized != ERROR_SUCCESS {
                return None;
            }
            let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
            let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
            // SAFETY: `paths` and `modes` hold exactly `path_count` and
            // `mode_count` initialized elements, which is what the counts passed in
            // promise; both counts are valid in/out `u32`s. The flags select active
            // paths, which take no topology id, so the optional pointer stays `None`.
            let queried = unsafe {
                QueryDisplayConfig(
                    flags,
                    &raw mut path_count,
                    paths.as_mut_ptr(),
                    &raw mut mode_count,
                    modes.as_mut_ptr(),
                    None,
                )
            };
            if queried == ERROR_INSUFFICIENT_BUFFER {
                continue;
            }
            if queried != ERROR_SUCCESS {
                return None;
            }
            // The call set `path_count` to the paths it wrote.
            paths.truncate(path_count as usize);
            return Some(paths.iter().filter_map(source_path).collect());
        }
        None
    }

    /// Name the GDI source of `path` and pair it with the path's target.
    fn source_path(path: &DISPLAYCONFIG_PATH_INFO) -> Option<SourcePath> {
        let mut request = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
        request.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
        request.header.size = std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
        request.header.adapterId = path.sourceInfo.adapterId;
        request.header.id = path.sourceInfo.id;
        // SAFETY: `request` is a `DISPLAYCONFIG_SOURCE_DEVICE_NAME` whose header
        // carries the matching request type and the struct's own size, so the OS
        // writes no further than the struct; it outlives the call.
        let status = unsafe { DisplayConfigGetDeviceInfo(&raw mut request.header) };
        if status != 0 {
            return None;
        }
        let target = &path.targetInfo;
        Some(SourcePath {
            gdi_name: wide_to_string(&request.viewGdiDeviceName),
            adapter_luid: (u64::from(target.adapterId.HighPart.cast_unsigned()) << 32)
                | u64::from(target.adapterId.LowPart),
            target_id: target.id,
            refresh: (target.refreshRate.Numerator, target.refreshRate.Denominator),
        })
    }

    /// The interface path of the active monitor attached to the adapter output
    /// `gdi_name`, from `EnumDisplayDevicesW`.
    fn monitor_interface_path(gdi_name: &str) -> Option<String> {
        /// More monitors than one adapter output can carry.
        const MAX_MONITORS_PER_OUTPUT: u32 = 16;
        let adapter: Vec<u16> = gdi_name.encode_utf16().chain(Some(0)).collect();
        for index in 0..MAX_MONITORS_PER_OUTPUT {
            let mut device = DISPLAY_DEVICEW {
                cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
                ..Default::default()
            };
            // SAFETY: `adapter` is a NUL-terminated UTF-16 string that outlives the
            // call, and `device` is a `DISPLAY_DEVICEW` with `cb` set to its size.
            let found = unsafe {
                EnumDisplayDevicesW(
                    PCWSTR(adapter.as_ptr()),
                    index,
                    &raw mut device,
                    EDD_GET_DEVICE_INTERFACE_NAME,
                )
            };
            if !found.as_bool() {
                return None;
            }
            if device.StateFlags.0 & DISPLAY_DEVICE_ACTIVE.0 != 0 {
                let path = wide_to_string(&device.DeviceID);
                return (!path.is_empty()).then_some(path);
            }
        }
        None
    }

    /// The text of a NUL-terminated UTF-16 buffer.
    fn wide_to_string(buffer: &[u16]) -> String {
        let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..end])
    }

    /// One monitor as `EnumDisplayMonitors` and `GetMonitorInfoW` describe it.
    struct Monitor {
        handle: isize,
        device_name: String,
        bounds: Bounds<i32>,
        usable_bounds: Bounds<i32>,
        scale_factor: f64,
        is_primary: bool,
    }

    impl Monitor {
        fn new(hmonitor: HMONITOR) -> Option<Self> {
            // SAFETY: `hmonitor` is a valid monitor handle the OS handed to
            // `EnumDisplayMonitors`'s callback. `MONITORINFOEXW` is zeroed with its
            // `cbSize` set before `GetMonitorInfoW`, which is that API's contract;
            // `GetDpiForMonitor` takes a valid HMONITOR and two valid out-pointers.
            unsafe {
                let mut monitor_info: MONITORINFOEXW = std::mem::zeroed();
                monitor_info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
                if !GetMonitorInfoW(hmonitor, (&raw mut monitor_info.monitorInfo).cast()).as_bool()
                {
                    return None;
                }
                let rc = monitor_info.monitorInfo.rcMonitor;
                let rc_work = monitor_info.monitorInfo.rcWork;

                let mut dpi_x = 96u32;
                let mut dpi_y = 96u32;
                let _ =
                    GetDpiForMonitor(hmonitor, MDT_EFFECTIVE_DPI, &raw mut dpi_x, &raw mut dpi_y);

                let device_name = wide_to_string(&monitor_info.szDevice);
                Some(Self {
                    handle: hmonitor.0 as isize,
                    device_name,
                    bounds: Bounds {
                        origin: Point::new(rc.left, rc.top),
                        size: Size::new(rc.right - rc.left, rc.bottom - rc.top),
                    },
                    usable_bounds: Bounds {
                        origin: Point::new(rc_work.left, rc_work.top),
                        size: Size::new(rc_work.right - rc_work.left, rc_work.bottom - rc_work.top),
                    },
                    scale_factor: f64::from(dpi_x) / 96.0,
                    // MONITORINFOF_PRIMARY = 1
                    is_primary: (monitor_info.monitorInfo.dwFlags & 1) != 0,
                })
            }
        }
    }

    fn enumerate_monitors() -> Vec<Monitor> {
        // Callback for EnumDisplayMonitors.
        //
        // # Safety
        // The caller (`EnumDisplayMonitors`) guarantees `hmonitor` is a valid
        // monitor handle and `lparam` carries the pointer passed below: a `*mut
        // Vec<Monitor>` live and unaliased for the whole enumeration.
        unsafe extern "system" fn enum_proc(
            hmonitor: HMONITOR,
            _hdc: HDC,
            _rect: *mut RECT,
            lparam: LPARAM,
        ) -> BOOL {
            // A panic ends the enumeration with the monitors read so far.
            os_entry(
                "the monitor enumeration",
                AfterPanic::Answer(|| FALSE),
                || {
                    // SAFETY: `lparam.0` is the address of `monitors` below, cast to
                    // `isize`. `EnumDisplayMonitors` blocks until every callback
                    // returns, so the vector is live and no other reference to it
                    // exists concurrently.
                    let monitors = unsafe {
                        &mut *std::ptr::with_exposed_provenance_mut::<Vec<Monitor>>(
                            lparam.0 as usize,
                        )
                    };
                    if let Some(monitor) = Monitor::new(hmonitor) {
                        monitors.push(monitor);
                    }
                    TRUE
                },
            )
        }

        let mut monitors: Vec<Monitor> = Vec::new();
        // SAFETY: `enum_proc` runs on this thread before `EnumDisplayMonitors`
        // returns, so `monitors` is live and exclusively accessible during all
        // callbacks; the pointer is passed through `LPARAM` and recovered there.
        unsafe {
            let _ = EnumDisplayMonitors(
                None,
                None,
                Some(enum_proc),
                LPARAM((&raw mut monitors).expose_provenance() as isize),
            );
        }
        monitors
    }

    // The key mapping is a private pure function, and its failure rows need
    // samples no live host can produce (a reused GDI name, a monitor without a
    // path), so they live beside it.
    #[cfg(test)]
    mod tests {
        use flui_platform::DisplayRegistry;

        use super::*;

        const DISPLAY2: &str = r"\\.\DISPLAY2";
        const INTERFACE: &str = r"\\?\DISPLAY#GSM5B09#4&2b1f&0&UID4352#{e6f07b5f}";

        fn path(gdi_name: &str, adapter_luid: u64, target_id: u32) -> SourcePath {
            SourcePath {
                gdi_name: gdi_name.to_owned(),
                adapter_luid,
                target_id,
                refresh: (0, 0),
            }
        }

        fn no_interface() -> Option<String> {
            None
        }

        fn a_reused_gdi_name_on_another_output_gets_a_new_id() {
            let registry = DisplayRegistry::new();
            // The monitor on DISPLAY2 is unplugged and another output takes the
            // same GDI name.
            let before = monitor_key(DISPLAY2, &[path(DISPLAY2, 7, 4352)], no_interface)
                .expect("a monitor with a path is keyed");
            let after = monitor_key(DISPLAY2, &[path(DISPLAY2, 7, 4353)], no_interface)
                .expect("a monitor with a path is keyed");
            assert_ne!(
                registry.mint(&before),
                registry.mint(&after),
                "a new output inherited the id of the one that left"
            );
        }

        fn the_same_output_keeps_its_id_across_samples() {
            let registry = DisplayRegistry::new();
            let paths = [path(r"\\.\DISPLAY1", 7, 4352), path(DISPLAY2, 7, 4353)];
            let sample = || {
                [r"\\.\DISPLAY1", DISPLAY2].map(|name| {
                    let key = monitor_key(name, &paths, no_interface).expect("keyed");
                    registry.mint(&key).expect("id")
                })
            };
            let first = sample();
            assert_ne!(first[0], first[1], "two outputs share an id");
            assert_eq!(first, sample(), "resampling changed an id");
        }

        fn a_monitor_without_a_config_path_uses_its_interface_path() {
            let registry = DisplayRegistry::new();
            let other_output = [path(r"\\.\DISPLAY1", 7, 4352)];
            let key = monitor_key(DISPLAY2, &other_output, || Some(INTERFACE.to_owned()));
            assert_eq!(
                key,
                Some(DisplayKey::WindowsInterface(INTERFACE.to_owned()))
            );
            let key = key.expect("keyed");
            assert_eq!(
                registry.mint(&key),
                registry.mint(&key),
                "an interface-keyed monitor churned its id between samples"
            );
            assert_eq!(
                monitor_key(DISPLAY2, &[], no_interface),
                None,
                "a monitor with neither a path nor an interface was keyed by its GDI name"
            );
            assert_eq!(
                monitor_key(DISPLAY2, &[path(DISPLAY2, 1, 2)], || panic!(
                    "the interface path is only the fallback"
                )),
                Some(DisplayKey::WindowsTarget {
                    adapter_luid: 1,
                    target_id: 2
                })
            );
        }

        #[test]
        fn monitor_key_table() {
            crate::table_test::run_table(
                "monitor_key_table",
                &[
                    (
                        "a_reused_gdi_name_on_another_output_gets_a_new_id",
                        a_reused_gdi_name_on_another_output_gets_a_new_id as fn(),
                    ),
                    (
                        "the_same_output_keeps_its_id_across_samples",
                        the_same_output_keeps_its_id_across_samples as fn(),
                    ),
                    (
                        "a_monitor_without_a_config_path_uses_its_interface_path",
                        a_monitor_without_a_config_path_uses_its_interface_path as fn(),
                    ),
                ],
            );
        }

        /// The display query asks for virtual-mode awareness from build 18362 on,
        /// and an unknown build unlocks nothing.
        #[test]
        fn windows_build_below_18362_omits_the_virtual_mode_flag() {
            assert_eq!(
                display_config_flags(WindowsBuild::from_number(18361)),
                QDC_ONLY_ACTIVE_PATHS
            );
            assert_eq!(
                display_config_flags(WindowsBuild::UNKNOWN),
                QDC_ONLY_ACTIVE_PATHS,
                "an unknown build unlocks nothing"
            );
            assert_eq!(
                display_config_flags(WindowsBuild::from_number(18362)),
                QDC_ONLY_ACTIVE_PATHS | QDC_VIRTUAL_MODE_AWARE
            );
        }
    }
}

#[cfg(windows)]
mod broadcasts {
    //! The displays' part of the system's broadcasts, which the owner's broadcast
    //! receiver tells its subscribers.

    use std::cell::Cell;
    use std::sync::{Arc, Weak};

    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_SETWORKAREA, WM_DISPLAYCHANGE, WM_SETTINGCHANGE,
    };

    use crate::owner::signal::OwnerSignal;
    use crate::owner::windows::HostDisplays;
    use crate::owner::windows::broadcast::Subscriber;

    /// The displays' part of the system's broadcasts: `WM_DISPLAYCHANGE` (a
    /// monitor was attached, removed or switched mode) and a work-area change
    /// (`WM_SETTINGCHANGE` with `SPI_SETWORKAREA`: the taskbar moved or was
    /// resized, which a display's usable bounds carry). Each marks the monitors
    /// stale and wakes the owner, which samples them on its next turn; nothing is
    /// sampled inside the message, which can arrive inside a COM call of ours.
    ///
    /// It only observes: Windows reads no result from either notification, so
    /// the subscriber answers neither and every subscriber after it hears the
    /// same message, whatever order the host lists them in.
    ///
    /// It admits wakes of its own accord until the receiver detaches it, so a
    /// message the receiver still hears while it is destroyed, or in a window the
    /// operating system kept after a failed destruction, wakes nobody. The mark
    /// stays: it is a flag the next sample clears.
    pub(crate) struct DisplayBroadcasts {
        displays: Arc<dyn HostDisplays>,
        signal: Weak<OwnerSignal>,
        /// Cleared when the receiver detaches this subscriber. The receiver runs
        /// on the owner thread only.
        attached: Cell<bool>,
    }

    impl DisplayBroadcasts {
        /// The subscriber that marks `displays` stale and wakes the owner behind
        /// `signal`.
        pub(crate) fn new(displays: &Arc<dyn HostDisplays>, signal: &Arc<OwnerSignal>) -> Self {
            Self {
                displays: Arc::clone(displays),
                signal: Arc::downgrade(signal),
                attached: Cell::new(true),
            }
        }

        /// The monitors may have changed: mark them stale and wake the owner. A
        /// wake that cannot be posted leaves the signal's debt for the next turn,
        /// and the mark for the next sample.
        fn notify(&self) {
            self.displays.invalidate();
            if self.attached.get()
                && let Some(signal) = self.signal.upgrade()
                && signal.accepting()
            {
                let _ = signal.wake();
            }
        }
    }

    impl Subscriber for DisplayBroadcasts {
        fn detach(&self, _receiver: HWND) {
            self.attached.set(false);
        }

        fn message(
            &self,
            _hwnd: HWND,
            message: u32,
            wparam: WPARAM,
            _lparam: LPARAM,
        ) -> Option<LRESULT> {
            let work_area = message == WM_SETTINGCHANGE && wparam.0 == SPI_SETWORKAREA.0 as usize;
            if message == WM_DISPLAYCHANGE || work_area {
                self.notify();
            }
            None
        }
    }

    #[cfg(test)]
    mod tests {
        use std::rc::Rc;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        use windows::Win32::UI::WindowsAndMessaging::{
            SPI_SETWORKAREA, WM_DISPLAYCHANGE, WM_SETTINGCHANGE,
        };

        use super::DisplayBroadcasts;
        use crate::display::windows::WindowsDisplays;
        use crate::owner::signal::OwnerSignal;
        use crate::owner::windows::HostDisplays;
        use crate::owner::windows::broadcast::{Receiver, Subscriber};

        /// An owner signal that counts its wakes.
        fn counting_signal() -> (Arc<OwnerSignal>, Arc<AtomicUsize>) {
            let wakes = Arc::new(AtomicUsize::new(0));
            let signal = OwnerSignal::new({
                let wakes = Arc::clone(&wakes);
                Arc::new(move || {
                    wakes.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
            });
            signal.start().expect("the owner signal starts");
            (signal, wakes)
        }

        /// Whether `displays` samples again on its next snapshot, which costs it
        /// that sample.
        fn samples_again(displays: &Arc<dyn HostDisplays>) -> bool {
            let before = displays.sample_count();
            displays.snapshot().expect("a snapshot");
            displays.sample_count() > before
        }

        /// Whom the subscriber wakes: a display change and a work-area change mark
        /// the monitors stale and wake the owner, any other setting is not the
        /// displays'; once detached, it still marks the monitors stale but wakes
        /// nobody. It answers none of them. The owner's turn runs after each wake,
        /// so the next one is posted again.
        #[test]
        fn display_broadcasts_mark_the_monitors_stale_and_wake_until_detached() {
            let (signal, wakes) = counting_signal();
            let displays: Arc<dyn HostDisplays> = Arc::new(WindowsDisplays::new());
            let subscriber = Rc::new(DisplayBroadcasts::new(&displays, &signal));
            let receiver =
                Receiver::new([Rc::clone(&subscriber) as Rc<dyn Subscriber>]).expect("a receiver");
            displays.snapshot().expect("the first sample");

            receiver.send_with_for_test(WM_DISPLAYCHANGE, 32, 0);
            assert!(samples_again(&displays), "a display change marked nothing");
            assert_eq!(wakes.load(Ordering::SeqCst), 1);
            signal.drive();

            receiver.send_with_for_test(WM_SETTINGCHANGE, SPI_SETWORKAREA.0 as usize, 0);
            assert!(
                samples_again(&displays),
                "a work-area change marked nothing"
            );
            assert_eq!(wakes.load(Ordering::SeqCst), 2);
            signal.drive();

            receiver.send_with_for_test(WM_SETTINGCHANGE, 0, 0);
            assert!(
                !samples_again(&displays),
                "another setting is not the displays'"
            );
            assert_eq!(wakes.load(Ordering::SeqCst), 2);

            let receiver_window = receiver.hwnd_for_test();
            drop(receiver);
            let answer = subscriber.message(
                receiver_window,
                WM_DISPLAYCHANGE,
                windows::Win32::Foundation::WPARAM(32),
                windows::Win32::Foundation::LPARAM(0),
            );
            assert_eq!(answer, None, "the subscriber answered a display change");
            assert!(
                samples_again(&displays),
                "a detached subscriber still marks the monitors"
            );
            assert_eq!(
                wakes.load(Ordering::SeqCst),
                2,
                "a detached subscriber woke the owner"
            );
        }

        /// The winit loop's receiver takes its subscriber from the displays'
        /// winit hook, which gives the same one: a display change marks the
        /// monitors stale and wakes the owner.
        #[cfg(feature = "winit-backend")]
        #[test]
        fn the_winit_loop_s_displays_hear_a_display_change() {
            let (signal, wakes) = counting_signal();
            let displays = Arc::new(WindowsDisplays::new());
            let sampled: Arc<dyn HostDisplays> = Arc::clone(&displays) as _;
            let subscriber = crate::owner::winit::HostDisplays::broadcasts(displays, &signal);
            let receiver = Receiver::new([subscriber]).expect("a receiver");
            sampled.snapshot().expect("the first sample");

            receiver.send_with_for_test(WM_DISPLAYCHANGE, 32, 0);
            assert!(samples_again(&sampled), "a display change marked nothing");
            assert_eq!(
                wakes.load(Ordering::SeqCst),
                1,
                "a display change did not wake the owner"
            );
        }
    }
}
