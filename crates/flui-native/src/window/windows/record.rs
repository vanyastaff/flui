//! What the window procedure records of a Win32 window, which the window's
//! wrappers and its owner-thread context share.
//!
//! The record is `Copy` and lives in an `Rc<Cell<_>>`: a read copies it out,
//! an update takes the record and returns the next one, and neither holds
//! anything while code runs. Both sides are on the owner thread (the wrapper
//! and the context are neither `Send` nor `Sync`), so a read made while an
//! update runs is the only re-entry there is, and it sees the record as last
//! set, where a lock would have left it waiting on its own thread.
//!
//! The pure half of the Win32 window: it names no OS type, so every host runs
//! its tests.

#![forbid(unsafe_code)]
#![cfg_attr(
    not(windows),
    allow(
        dead_code,
        reason = "off Windows the record is compiled for its tests alone"
    )
)]

use std::cell::Cell;
use std::rc::Weak;

use flui_foundation::geometry::{Bounds, DevicePixelRatio};

/// What the window procedure last recorded of a window: its bounds, scale,
/// visibility and focus.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct WindowRecord {
    /// The client area's bounds, in logical pixels.
    pub(crate) bounds: Bounds<f64>,
    /// The window's scale (its DPI over 96).
    pub(crate) scale_factor: DevicePixelRatio,
    /// Whether the window is shown.
    pub(crate) visible: bool,
    /// Whether the window has the keyboard focus.
    pub(crate) focused: bool,
}

/// Record what the window procedure learned of a window, while any wrapper of
/// it lives: `change` takes the record and returns the next one. It is given
/// no access to the cell, and holds nothing: a read of the record it makes
/// through another handle sees the record as last set, and a write it made
/// that way would be replaced by what it returns.
pub(crate) fn update(
    record: &Weak<Cell<WindowRecord>>,
    change: impl FnOnce(WindowRecord) -> WindowRecord,
) {
    if let Some(record) = record.upgrade() {
        record.update(change);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use flui_foundation::geometry::{Bounds, DevicePixelRatio, Point, Size};

    use super::{WindowRecord, update};

    fn record(width: f64) -> WindowRecord {
        WindowRecord {
            bounds: Bounds {
                origin: Point::new(10.0, 20.0),
                size: Size::new(width, 300.0),
            },
            scale_factor: DevicePixelRatio::try_new(1.5).expect("a screen's ratio"),
            visible: true,
            focused: false,
        }
    }

    /// A read of the record made while an update runs (what the code an
    /// update reaches would see) answers the record as last set and holds up
    /// nothing; the update lands once it returns, and an update of a window
    /// no wrapper holds any more records nothing.
    ///
    /// Red-check: share the record as `Arc<Mutex<_>>` and hold the lock across
    /// the update, and the nested read waits on its own thread forever.
    #[test]
    fn a_read_inside_a_record_update_sees_the_last_record_set() {
        let shared = Rc::new(Cell::new(record(400.0)));
        let held = Rc::downgrade(&shared);
        let mut inside = None;
        update(&held, |last| {
            inside = Some(shared.get());
            WindowRecord {
                bounds: Bounds {
                    size: Size::new(640.0, 300.0),
                    ..last.bounds
                },
                ..last
            }
        });
        assert_eq!(
            inside,
            Some(record(400.0)),
            "the read sees the last record set"
        );
        assert_eq!(
            shared.get(),
            record(640.0),
            "the update landed once it returned"
        );

        drop(shared);
        let mut ran = false;
        update(&held, |last| {
            ran = true;
            last
        });
        assert!(!ran, "a window no wrapper holds records nothing");
    }
}
