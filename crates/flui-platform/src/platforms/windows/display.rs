//! Windows display source.
//!
//! Samples the monitors with `EnumDisplayMonitors` and turns them into a
//! [`DisplaySnapshot`]. [`WindowsDisplays`] keeps the last sample and answers
//! from it until something invalidates it: `WM_DISPLAYCHANGE` on the hidden
//! settings receiver, a work-area change, or a window's `WM_DPICHANGED`. The
//! host asks for the snapshot on every owner turn, so a sample that costs
//! several OS queries is taken when the displays may have changed, not per turn.
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

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform_api::{DisplayId, DisplayInfo, DisplayKey, DisplaySnapshot};
use parking_lot::Mutex;
use windows::Win32::{
    Devices::Display::{
        DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_MODE_INFO,
        DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME, DisplayConfigGetDeviceInfo,
        GetDisplayConfigBufferSizes, QUERY_DISPLAY_CONFIG_FLAGS, QueryDisplayConfig,
    },
    Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, HWND, LPARAM, RECT, TRUE},
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

use super::os_version::WindowsBuild;
use crate::error::PlatformError;
use crate::shared::{
    display_source::DisplaySource,
    windows_display::{SampledMonitor, frequency_period, rational_refresh_period, resolve_display},
};

/// One platform instance's monitors: the last sample, and whether anything has
/// invalidated it since (both kept by the [`DisplaySource`]).
pub(crate) struct WindowsDisplays {
    source: DisplaySource,
    build: WindowsBuild,
    /// The handle-to-key pairs of the last sample, which resolve a window's
    /// nearest monitor.
    monitors: Mutex<Vec<SampledMonitor>>,
    #[cfg(test)]
    samples: std::sync::atomic::AtomicUsize,
}

impl crate::shared::system_notice::StaleOnWake for WindowsDisplays {
    fn mark_stale(&self) {
        self.invalidate();
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

    /// Record that the displays may have changed. Touches one flag and nothing
    /// else, so a window procedure or a foreign thread may call it at any point;
    /// the next [`snapshot`](Self::snapshot) samples again.
    pub(crate) fn invalidate(&self) {
        self.source.invalidate();
    }

    /// The snapshot: the last sample, or a fresh one when none was taken or
    /// something invalidated it.
    ///
    /// # Errors
    /// [`PlatformError::Displays`] when the snapshot revision space is used up;
    /// the next call samples again.
    pub(crate) fn snapshot(&self) -> Result<DisplaySnapshot, PlatformError> {
        self.source.snapshot_with_aux(
            || Ok(self.sample()),
            |monitors| *self.monitors.lock() = monitors,
        )
    }

    /// Sample now and report whether the snapshot differs from the one the last
    /// sample published.
    ///
    /// # Errors
    /// As [`snapshot`](Self::snapshot).
    #[cfg(feature = "winit-backend")]
    pub(crate) fn resample_changed(&self) -> Result<bool, PlatformError> {
        let before = self.source.published().revision();
        self.invalidate();
        Ok(self.snapshot()?.revision() != before)
    }

    /// The display `hwnd` is on: the one the window's nearest monitor is in the
    /// last sample, by native key.
    ///
    /// Reads the last sample and never takes one: the host derives window
    /// displays from a snapshot it has accepted, and a sample here would publish
    /// a newer revision mid-derivation. `None` for a window with no monitor, a
    /// monitor the sample lacks (a monitor plugged in since), or an output with
    /// no id. A monitor the sample lacks invalidates the snapshot once per
    /// revision, so the next owner turn samples it and the repeated miss costs
    /// nothing.
    pub(crate) fn display_of_window(&self, hwnd: HWND) -> Option<DisplayId> {
        let nearest = nearest_monitor(hwnd);
        if nearest == 0 {
            return None;
        }
        resolve_display(
            nearest,
            &self.monitors.lock(),
            || self.source.note_miss(nearest.cast_unsigned() as u64),
            |key| self.source.mint(key),
        )
    }

    #[cfg(test)]
    pub(crate) fn sample_count(&self) -> usize {
        self.samples.load(std::sync::atomic::Ordering::Relaxed)
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
        let paths = query_source_paths(self.build.display_config_flags()).unwrap_or_default();
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

/// The handle of the monitor nearest `hwnd`, `0` for none.
pub(crate) fn nearest_monitor(hwnd: HWND) -> isize {
    // SAFETY: `MonitorFromWindow` takes the handle by value and dereferences
    // nothing; a destroyed or foreign handle yields a handle or null, never a
    // fault.
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    monitor.0 as isize
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
        let sized =
            unsafe { GetDisplayConfigBufferSizes(flags, &raw mut path_count, &raw mut mode_count) };
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
            if !GetMonitorInfoW(hmonitor, (&raw mut monitor_info.monitorInfo).cast()).as_bool() {
                return None;
            }
            let rc = monitor_info.monitorInfo.rcMonitor;
            let rc_work = monitor_info.monitorInfo.rcWork;

            let mut dpi_x = 96u32;
            let mut dpi_y = 96u32;
            let _ = GetDpiForMonitor(hmonitor, MDT_EFFECTIVE_DPI, &raw mut dpi_x, &raw mut dpi_y);

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
        // SAFETY: `lparam.0` is the address of `monitors` below, cast to
        // `isize`. `EnumDisplayMonitors` blocks until every callback returns, so
        // the vector is live and no other reference to it exists concurrently.
        let monitors = unsafe {
            &mut *std::ptr::with_exposed_provenance_mut::<Vec<Monitor>>(lparam.0 as usize)
        };
        if let Some(monitor) = Monitor::new(hmonitor) {
            monitors.push(monitor);
        }
        TRUE
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
    use flui_platform_api::DisplayRegistry;

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
}
