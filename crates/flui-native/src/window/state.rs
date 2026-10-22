//! The window state every backend keeps: the snapshot it last published and
//! the rule for when it is read from the OS again (ADR-0237), built like
//! the display source (`display::source`), and the one rule that decides a
//! window's mode from what its window system says of it (`mode_of`).

#![forbid(unsafe_code)]

use std::sync::atomic::{AtomicU64, Ordering};

use flui_platform::WindowState;
use parking_lot::Mutex;

/// What a window system says of a window, as the three facts its mode is
/// decided from. Each platform reads them from its own report (winit's
/// queries, AppKit's style mask and flags, the X11 window manager's
/// properties, the Win32 window's tracked mode) and [`mode_of`] decides.
#[cfg(any(
    windows,
    target_os = "macos",
    all(target_os = "linux", feature = "winit-backend"),
    test
))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ModeFlags {
    /// The window is minimized: iconic, in the Dock or on the taskbar.
    pub(crate) minimized: bool,
    /// The window covers its monitor without a frame.
    pub(crate) fullscreen: bool,
    /// The window is maximized.
    pub(crate) maximized: bool,
}

/// The mode of a window with these flags. A minimized window is hidden
/// whatever else is set; a fullscreen one often keeps its maximized flag, so
/// fullscreen comes before maximized.
#[cfg(any(
    windows,
    target_os = "macos",
    all(target_os = "linux", feature = "winit-backend"),
    test
))]
pub(crate) const fn mode_of(flags: ModeFlags) -> flui_platform::WindowMode {
    use flui_platform::WindowMode;
    if flags.minimized {
        WindowMode::Minimized
    } else if flags.fullscreen {
        WindowMode::Fullscreen
    } else if flags.maximized {
        WindowMode::Maximized
    } else {
        WindowMode::Normal
    }
}

/// What a read of the cell answered.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Published {
    /// The snapshot now published.
    pub(crate) state: WindowState,
    /// Whether this call published a higher revision. A backend raises
    /// `on_state_changed` exactly when this is `true`, after the call returned
    /// and so outside the cell's lock.
    pub(crate) changed: bool,
}

struct Slot {
    state: WindowState,
    /// The invalidations counted when the published state was read: the
    /// snapshot is current while no invalidation has come since.
    read_after: u64,
    /// The revision space was used up: nothing is published any more.
    frozen: bool,
}

/// One window's last published [`WindowState`] and its staleness.
///
/// A backend marks the cell stale on every OS event that may change the window
/// (`WM_SIZE`, `windowDidResize`, a winit `Resized`, `fullscreenchange`, or a
/// command it just issued). The next [`state_with`](Self::state_with) reads the
/// OS again, with no lock held across the read, and publishes the sample; a
/// sample equal to the published one keeps its revision.
///
/// Threads may read at once. A sample begun before an invalidation that
/// another read has already published past is discarded, so once the read
/// after a change has published, no slower read puts the window from before
/// the change back. Reads begun before the same invalidation may still publish
/// in either order until then: the samples cannot be ordered without a lock
/// held across them, and a backend's sample may wait on its owner thread.
pub(crate) struct WindowStateCell {
    last: Mutex<Slot>,
    /// Counts every invalidation, from any thread. A read takes the count
    /// before it reads the OS, so an invalidation that lands during the read
    /// leaves the cell stale. At one per nanosecond it would take centuries to
    /// wrap.
    invalidations: AtomicU64,
}

impl crate::owner::notices::StaleOnWake for WindowStateCell {
    fn mark_stale(&self) {
        self.invalidate();
    }
}

impl WindowStateCell {
    /// Whether the next read asks the operating system again.
    #[cfg(test)]
    pub(crate) fn is_stale(&self) -> bool {
        self.last.lock().read_after != self.invalidations.load(Ordering::Acquire)
    }

    /// A cell publishing `initial`, stale: the first `state_with` reads the OS.
    pub(crate) fn new(initial: WindowState) -> Self {
        Self {
            last: Mutex::new(Slot {
                state: initial,
                read_after: 0,
                frozen: false,
            }),
            invalidations: AtomicU64::new(1),
        }
    }

    /// Record that the window may have changed. One atomic increment, so a
    /// window procedure, a notification block or a foreign thread may call it
    /// at any point.
    pub(crate) fn invalidate(&self) {
        self.invalidations.fetch_add(1, Ordering::Release);
    }

    /// The last published snapshot, without reading the OS.
    #[cfg(windows)]
    pub(crate) fn published(&self) -> WindowState {
        self.last.lock().state.clone()
    }

    /// The snapshot: the last published one while nothing has invalidated the
    /// cell, otherwise the result of `sample` published.
    ///
    /// `sample` runs with no lock held, so it may call the OS.
    pub(crate) fn state_with(&self, sample: impl FnOnce() -> WindowState) -> Published {
        let invalidations = self.invalidations.load(Ordering::Acquire);
        {
            let slot = self.last.lock();
            if slot.read_after == invalidations {
                return Published {
                    state: slot.state.clone(),
                    changed: false,
                };
            }
        }
        self.publish(sample(), invalidations)
    }

    /// Publish a sample read after `invalidations`: the previous snapshot when
    /// nothing differs or a read begun later has published already, otherwise
    /// the next revision. At the last revision the cell freezes on its final
    /// snapshot, with one diagnostic, and publishes nothing again.
    fn publish(&self, sample: WindowState, invalidations: u64) -> Published {
        let mut slot = self.last.lock();
        if slot.read_after > invalidations {
            return Published {
                state: slot.state.clone(),
                changed: false,
            };
        }
        slot.read_after = invalidations;
        if slot.frozen {
            return Published {
                state: slot.state.clone(),
                changed: false,
            };
        }
        match slot.state.successor(sample) {
            Ok(next) => {
                let changed = next.revision != slot.state.revision;
                if changed {
                    slot.state = next.clone();
                }
                Published {
                    state: next,
                    changed,
                }
            }
            Err(error) => {
                slot.frozen = true;
                tracing::warn!(%error, "a window stops publishing state changes");
                Published {
                    state: slot.state.clone(),
                    changed: false,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use flui_foundation::geometry::Size;
    use flui_platform::{WindowGeometry, WindowRevision, WindowState};

    use super::WindowStateCell;

    fn state(width: f64) -> WindowState {
        WindowState::new(
            WindowGeometry::new(None, Size::new(width, 100.0)),
            Size::new(width, 100.0),
        )
    }

    #[test]
    fn invalidation_during_a_sample_stays_stale() {
        let cell = WindowStateCell::new(state(1.0));
        let samples = Cell::new(0);
        let _ = cell.state_with(|| {
            samples.set(samples.get() + 1);
            // The OS changes while the window is being read.
            cell.invalidate();
            state(2.0)
        });
        let _ = cell.state_with(|| {
            samples.set(samples.get() + 1);
            state(2.0)
        });
        assert_eq!(samples.get(), 2, "the next call reads again");
        let _ = cell.state_with(|| {
            samples.set(samples.get() + 1);
            state(2.0)
        });
        assert_eq!(samples.get(), 2, "and then the snapshot is kept");
    }

    /// Two threads read at once: the one that began first finishes last, after
    /// the window changed and the other read published the change.
    #[test]
    fn a_read_overtaken_by_a_later_one_publishes_nothing() {
        let cell = WindowStateCell::new(state(1.0));
        let _ = cell.state_with(|| state(1.0));
        cell.invalidate();
        let overtaken = cell.state_with(|| {
            // Read before the change, still to be published.
            let older = state(1.0);
            cell.invalidate();
            let newer = cell.state_with(|| state(2.0));
            assert!(newer.changed);
            older
        });
        assert!(!overtaken.changed, "the older read publishes no revision");
        assert_eq!(overtaken.state.content_size, Size::new(2.0, 100.0));
        let kept = cell.state_with(|| panic!("the newer snapshot is current"));
        assert_eq!(kept.state, overtaken.state);
    }

    #[test]
    fn an_exhausted_revision_freezes_the_cell() {
        let mut last = state(1.0);
        last.revision = WindowRevision::LAST;
        let cell = WindowStateCell::new(last.clone());
        for attempt in 0..3 {
            cell.invalidate();
            let published = cell.state_with(|| state(2.0 + f64::from(attempt)));
            assert!(!published.changed);
            assert_eq!(published.state, last, "attempt {attempt}");
        }
        assert!(
            !cell.is_stale(),
            "a frozen cell is current until invalidated"
        );
        cell.invalidate();
        assert_eq!(
            cell.state_with(|| state(1.0)).state,
            last,
            "a frozen cell stays frozen even for a sample equal to its state"
        );
    }
}

/// The table of the one mode rule, which every window system's reading of its
/// own report is held to: a row's flags, put as that system reports them and
/// read back by the system's own reading, decide the row's mode.
#[cfg(test)]
mod mapping {
    use flui_foundation::geometry::{Bounds, Point, Size};
    use flui_platform::WindowMode::{self, Fullscreen, Maximized, Minimized, Normal};

    use super::{ModeFlags, mode_of};
    use crate::window::apple::geometry::mode::WindowFlags;
    use crate::window::linux::x11::mode::{NetWmState, WmState, mode_flags};
    use crate::window::windows::mode::NativeMode;

    const fn flags(minimized: bool, fullscreen: bool, maximized: bool) -> ModeFlags {
        ModeFlags {
            minimized,
            fullscreen,
            maximized,
        }
    }

    /// (case, what the window system says, mode)
    const ROWS: [(&str, ModeFlags, WindowMode); 8] = [
        ("nothing set", flags(false, false, false), Normal),
        ("maximized", flags(false, false, true), Maximized),
        ("fullscreen", flags(false, true, false), Fullscreen),
        ("minimized", flags(true, false, false), Minimized),
        (
            "fullscreen that kept its maximized flag",
            flags(false, true, true),
            Fullscreen,
        ),
        (
            "minimized from fullscreen",
            flags(true, true, false),
            Minimized,
        ),
        (
            "minimized from maximized",
            flags(true, false, true),
            Minimized,
        ),
        ("all three", flags(true, true, true), Minimized),
    ];

    /// The properties an X11 window manager keeps for such a window:
    /// `WM_STATE` iconic and `_NET_WM_STATE_HIDDEN` for a minimized one, the
    /// fullscreen atom, and both maximized atoms.
    fn x11(said: ModeFlags) -> ModeFlags {
        let wm_state = if said.minimized {
            WmState::Iconic
        } else {
            WmState::Normal
        };
        mode_flags(
            Some(wm_state),
            NetWmState {
                hidden: said.minimized,
                maximized_horz: said.maximized,
                maximized_vert: said.maximized,
                fullscreen: said.fullscreen,
            },
        )
    }

    /// AppKit's flags for such a window: AppKit leaves full screen before it
    /// miniaturizes a window, and a maximized window is a zoomed resizable one.
    fn appkit(said: ModeFlags) -> ModeFlags {
        WindowFlags {
            fullscreen: said.fullscreen && !said.minimized,
            miniaturized: said.minimized,
            zoomed: said.maximized,
            resizable: true,
        }
        .mode_flags()
    }

    /// The mode a Win32 window tracks for such a window: a fullscreen window
    /// keeps the rectangle it returns to, also while it is minimized, and a
    /// fullscreen window's maximized flag is its own business.
    fn win32(said: ModeFlags) -> ModeFlags {
        let back = Bounds::new(Point::new(10, 20), Size::new(800, 600));
        let mode = if said.minimized {
            NativeMode::Minimized {
                from_fullscreen: said.fullscreen.then_some(back),
            }
        } else if said.fullscreen {
            NativeMode::Fullscreen {
                restore_bounds: back,
            }
        } else if said.maximized {
            NativeMode::Maximized
        } else {
            NativeMode::Normal
        };
        mode.flags()
    }

    /// A window system's reading of its own report of a window.
    type Reading = fn(ModeFlags) -> ModeFlags;

    /// Every window system decides a row's mode alike; winit's three queries
    /// are the flags as they stand.
    #[test]
    fn every_window_system_decides_the_mode_alike() {
        let readings: [(&str, Reading); 4] = [
            ("winit", |said| said),
            ("X11", x11),
            ("AppKit", appkit),
            ("Win32", win32),
        ];
        for (case, said, mode) in ROWS {
            for (system, read) in readings {
                assert_eq!(mode_of(read(said)), mode, "{system}: {case}");
            }
        }
    }
}
