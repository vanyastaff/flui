//! What the window procedure records of a Win32 window, which its wrappers
//! and its owner-thread context share without a lock.

use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform::WindowOptions;

use super::proc::with_window_context;
use super::window::WindowsWindow;
use crate::WindowsPlatform;
use crate::host_window::HostWindow;
use crate::platform::Platform as _;

/// A read of the window made while the window procedure updates its record
/// (what code an update reaches would see) answers the record as last set and
/// holds up nothing; the update lands once it returns. Run on a thread of its
/// own and awaited with a bound, since a lock held across the update would
/// leave the nested read waiting on its own thread forever.
#[test]
fn a_read_inside_a_record_update_sees_the_last_record_set() {
    let (answer, answered) = mpsc::channel::<(Bounds<f64>, Bounds<f64>, Bounds<f64>)>();
    std::thread::spawn(move || {
        let platform = WindowsPlatform::try_new().expect("native platform");
        let window: Rc<dyn HostWindow> = platform
            .open_window(
                WindowOptions::default().title("record").visible(false),
                flui_testing::window_handler::RecordingWindowHandler::new(),
            )
            .expect("a hidden window opens");
        let wrapper = window
            .as_any()
            .downcast_ref::<WindowsWindow>()
            .expect("Win32 backend");
        let before = wrapper.bounds();
        let moved = Bounds {
            origin: Point::new(before.origin.x + 7.0, before.origin.y + 11.0),
            size: Size::new(before.size.width + 13.0, before.size.height + 17.0),
        };
        let inside = std::cell::Cell::new(None);
        with_window_context(wrapper.hwnd(), "record update", |ctx| {
            ctx.update_window_state(|record| {
                record.bounds = moved;
                inside.set(Some(wrapper.bounds()));
            });
        })
        .expect("the window's context");
        let inside = inside.get().expect("the update ran");
        let _ = answer.send((before, inside, wrapper.bounds()));
    });
    let (before, inside, after) = answered
        .recv_timeout(Duration::from_secs(10))
        .expect("a read inside the record's update returned");
    assert_eq!(
        inside, before,
        "the read inside the update sees the last record set"
    );
    assert_ne!(after, before, "the update landed once it returned");
    assert_eq!(after.size.width, before.size.width + 13.0);
}
