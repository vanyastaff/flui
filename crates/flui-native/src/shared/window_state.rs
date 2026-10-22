//! The window state every backend keeps: the snapshot it last published and
//! the rule for when it is read from the OS again (ADR-0237), built like
//! [`DisplaySource`](crate::display::source).

use std::sync::atomic::{AtomicU64, Ordering};

use flui_platform::WindowState;
use parking_lot::Mutex;

/// The mode of a window from what the window system says of it. A minimized
/// window is hidden whatever else is set; a fullscreen one often keeps its
/// maximized flag, so fullscreen comes before maximized.
#[cfg(any(
    all(
        feature = "winit-backend",
        any(windows, target_os = "macos", target_os = "linux")
    ),
    test
))]
pub(crate) const fn mode_of(
    minimized: bool,
    fullscreen: bool,
    maximized: bool,
) -> flui_platform::WindowMode {
    use flui_platform::WindowMode;
    if minimized {
        WindowMode::Minimized
    } else if fullscreen {
        WindowMode::Fullscreen
    } else if maximized {
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

impl super::system_notice::StaleOnWake for WindowStateCell {
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
    use flui_platform::{WindowGeometry, WindowMode, WindowRevision, WindowState};

    use super::{WindowStateCell, mode_of};

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

    #[test]
    fn window_mode_precedence() {
        // (case, minimized, fullscreen, maximized, mode)
        let table = [
            ("nothing set", false, false, false, WindowMode::Normal),
            ("maximized", false, false, true, WindowMode::Maximized),
            ("fullscreen", false, true, false, WindowMode::Fullscreen),
            ("minimized", true, false, false, WindowMode::Minimized),
            (
                "fullscreen that kept its maximized flag",
                false,
                true,
                true,
                WindowMode::Fullscreen,
            ),
            (
                "minimized from fullscreen",
                true,
                true,
                false,
                WindowMode::Minimized,
            ),
            (
                "minimized from maximized",
                true,
                false,
                true,
                WindowMode::Minimized,
            ),
            ("all three", true, true, true, WindowMode::Minimized),
        ];
        for (case, minimized, fullscreen, maximized, expected) in table {
            assert_eq!(
                mode_of(minimized, fullscreen, maximized),
                expected,
                "{case}"
            );
        }
    }
}
