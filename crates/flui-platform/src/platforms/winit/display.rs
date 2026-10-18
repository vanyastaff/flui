//! The winit backend's display source.
//!
//! winit enumerates monitors only through a live event loop or a window, so
//! this source holds the last list it was given and turns it into a
//! [`DisplaySnapshot`]. It is re-sampled at every event-loop turn that can see
//! a change; the window names its display through `current_monitor()`, matched
//! to the list by monitor identity.
//!
//! The monitor key is the identifier winit exposes for the OS (`native_id`),
//! which is a label on Windows (the GDI device name) — a stopgap that the
//! native per-OS display sources replace.

use std::sync::Arc;

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform_api::{DisplayId, DisplayInfo, DisplayKey, DisplayRegistry, DisplaySnapshot};
use parking_lot::Mutex;
use winit::monitor::MonitorHandle;

use crate::shared::display_period::winit_refresh_period;

/// The monitors last sampled, their ids and the snapshot built from them.
pub(super) struct WinitDisplaySource {
    registry: DisplayRegistry,
    sampled: Mutex<Sampled>,
}

struct Sampled {
    snapshot: DisplaySnapshot,
    monitors: Vec<(DisplayId, MonitorHandle)>,
}

impl WinitDisplaySource {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            registry: DisplayRegistry::new(),
            sampled: Mutex::new(Sampled {
                snapshot: DisplaySnapshot::empty(),
                monitors: Vec::new(),
            }),
        })
    }

    /// Replace the sampled list with `monitors`. Returns whether the snapshot
    /// changed. A monitor with no id (the id space is used up) is left out, and
    /// a revision that cannot advance keeps the previous sample.
    pub(super) fn resample(
        &self,
        monitors: impl IntoIterator<Item = MonitorHandle>,
        primary: Option<&MonitorHandle>,
    ) -> bool {
        let mut keys = Vec::new();
        let mut handles = Vec::new();
        let mut displays = Vec::new();
        let monitors = super::platform::map_monitor_displays(
            monitors.into_iter(),
            primary.cloned(),
            |monitor, _, is_primary| (monitor, is_primary),
        );
        for (monitor, is_primary) in monitors {
            let key = unique_key(monitor_key(&monitor), &keys);
            let Ok(id) = self.registry.mint(&key) else {
                tracing::warn!("display ids exhausted; a monitor is left out of the snapshot");
                continue;
            };
            let position = monitor.position();
            let size = monitor.size();
            displays.push(display_info(
                id,
                monitor.name(),
                (position.x, position.y),
                (size.width, size.height),
                monitor.scale_factor(),
                monitor.refresh_rate_millihertz(),
                is_primary,
            ));
            handles.push((id, monitor));
            keys.push(key);
        }
        let mut sampled = self.sampled.lock();
        let Ok(snapshot) = sampled.snapshot.successor(displays) else {
            tracing::warn!("display revisions exhausted; keeping the last snapshot");
            return false;
        };
        let changed = snapshot != sampled.snapshot;
        sampled.snapshot = snapshot;
        sampled.monitors = handles;
        changed
    }

    pub(super) fn snapshot(&self) -> DisplaySnapshot {
        self.sampled.lock().snapshot.clone()
    }

    /// The display `monitor` is, by monitor identity; `None` when the last
    /// sample does not hold it.
    pub(super) fn id_of(&self, monitor: &MonitorHandle) -> Option<DisplayId> {
        self.sampled
            .lock()
            .monitors
            .iter()
            .find(|(_, known)| known == monitor)
            .map(|(id, _)| *id)
    }
}

/// One display from the parts winit reports for a monitor.
fn display_info(
    id: DisplayId,
    name: Option<String>,
    position: (i32, i32),
    size: (u32, u32),
    scale_factor: f64,
    refresh_millihertz: Option<u32>,
    is_primary: bool,
) -> DisplayInfo {
    let bounds = Bounds::new(
        Point::new(position.0, position.1),
        Size::new(
            i32::try_from(size.0).unwrap_or(i32::MAX),
            i32::try_from(size.1).unwrap_or(i32::MAX),
        ),
    );
    let mut info = DisplayInfo::new(
        id,
        name.unwrap_or_else(|| format!("Display {}", id.get())),
        bounds,
        scale_factor,
    )
    .with_primary(is_primary);
    info.refresh_period = winit_refresh_period(refresh_millihertz);
    info
}

/// A key not yet taken in this sample: two monitors that report one label stay
/// two displays.
fn unique_key(key: DisplayKey, taken: &[DisplayKey]) -> DisplayKey {
    let DisplayKey::Backend(label) = &key else {
        return key;
    };
    let mut candidate = key.clone();
    let mut ordinal = 1_u32;
    while taken.contains(&candidate) {
        candidate = DisplayKey::Backend(format!("{label}#{ordinal}"));
        ordinal += 1;
    }
    candidate
}

#[cfg(windows)]
fn monitor_key(monitor: &MonitorHandle) -> DisplayKey {
    use winit::platform::windows::MonitorHandleExtWindows;
    DisplayKey::Backend(monitor.native_id())
}

#[cfg(target_os = "macos")]
fn monitor_key(monitor: &MonitorHandle) -> DisplayKey {
    use winit::platform::macos::MonitorHandleExtMacOS;
    DisplayKey::Backend(format!("cgdisplay:{}", monitor.native_id()))
}

#[cfg(target_os = "linux")]
fn monitor_key(monitor: &MonitorHandle) -> DisplayKey {
    // Both Linux extension traits read the same inner identifier: the RandR
    // output on X11, the `wl_output` global name on Wayland.
    use winit::platform::x11::MonitorHandleExtX11;
    DisplayKey::Backend(format!("output:{}", monitor.native_id()))
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn monitor_key(monitor: &MonitorHandle) -> DisplayKey {
    let position = monitor.position();
    DisplayKey::Backend(format!(
        "{}@{},{}",
        monitor.name().unwrap_or_default(),
        position.x,
        position.y
    ))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use flui_platform_api::{DisplayKey, DisplayRegistry, RefreshPeriod};

    use super::{display_info, unique_key};

    fn id() -> flui_platform_api::DisplayId {
        DisplayRegistry::new()
            .mint(&DisplayKey::Injected(1))
            .expect("id")
    }

    #[test]
    fn monitor_parts_become_a_display_with_the_period_winit_reported() {
        let info = display_info(
            id(),
            Some("Panel".into()),
            (-1920, 0),
            (1920, 1080),
            1.5,
            Some(144_000),
            true,
        );
        assert_eq!(info.name, "Panel");
        assert_eq!(info.bounds.origin.x, -1920);
        assert_eq!(info.bounds.size.width, 1920);
        assert_eq!(info.scale_factor, 1.5);
        assert!(info.is_primary);
        assert_eq!(
            info.refresh_period.map(RefreshPeriod::get),
            Some(Duration::from_nanos(6_944_444))
        );
        let unreported = display_info(id(), None, (0, 0), (800, 600), 1.0, None, false);
        assert_eq!(unreported.refresh_period, None);
        assert_eq!(unreported.name, "Display 1");
    }

    #[test]
    fn monitors_sharing_a_label_keep_distinct_keys() {
        let label = DisplayKey::Backend("DISPLAY1".into());
        let second = unique_key(label.clone(), std::slice::from_ref(&label));
        assert_ne!(second, label);
        let third = unique_key(label.clone(), &[label.clone(), second.clone()]);
        assert_ne!(third, label);
        assert_ne!(third, second);
        assert_eq!(unique_key(label.clone(), &[]), label);
    }
}
