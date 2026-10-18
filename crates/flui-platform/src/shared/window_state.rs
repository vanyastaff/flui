//! The window state every backend keeps: the snapshot it last published and
//! the rule for when it is read from the OS again (ADR-0237), built like
//! [`DisplaySource`](super::display_source).

use std::sync::atomic::{AtomicBool, Ordering};

use flui_platform_api::WindowState;
use parking_lot::Mutex;

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
pub(crate) struct WindowStateCell {
    last: Mutex<Slot>,
    /// Set from any thread when the window may differ from `last`; cleared
    /// before a sample reads the OS, so an invalidation that lands during the
    /// read survives it.
    stale: AtomicBool,
}

impl WindowStateCell {
    /// A cell publishing `initial`, stale: the first `state_with` reads the OS.
    pub(crate) fn new(initial: WindowState) -> Self {
        Self {
            last: Mutex::new(Slot {
                state: initial,
                frozen: false,
            }),
            stale: AtomicBool::new(true),
        }
    }

    /// Record that the window may have changed. One atomic store, so a window
    /// procedure, a notification block or a foreign thread may call it at any
    /// point.
    pub(crate) fn invalidate(&self) {
        self.stale.store(true, Ordering::Release);
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
        if !self.stale.load(Ordering::Acquire) {
            return Published {
                state: self.last.lock().state.clone(),
                changed: false,
            };
        }
        self.stale.store(false, Ordering::Release);
        self.publish(sample())
    }

    /// Publish a sample: the previous snapshot when nothing differs, otherwise
    /// the next revision. At the last revision the cell freezes on its final
    /// snapshot, with one diagnostic, and publishes nothing again.
    pub(crate) fn publish(&self, sample: WindowState) -> Published {
        let mut slot = self.last.lock();
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
    use flui_platform_api::{WindowGeometry, WindowRevision, WindowState};

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

    #[test]
    fn an_exhausted_revision_freezes_the_cell() {
        let mut last = state(1.0);
        last.revision = WindowRevision::last();
        let cell = WindowStateCell::new(last.clone());
        for attempt in 0..3 {
            let published = cell.publish(state(2.0 + f64::from(attempt)));
            assert!(!published.changed);
            assert_eq!(published.state, last, "attempt {attempt}");
        }
        assert_eq!(
            cell.publish(state(1.0)).state,
            last,
            "a frozen cell stays frozen even for a sample equal to its state"
        );
    }
}
