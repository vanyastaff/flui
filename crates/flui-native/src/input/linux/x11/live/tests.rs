//! The live input on the X server `DISPLAY` names; each case passes as
//! untested where none answers.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Weak};

use cursor_icon::CursorIcon;
use flui_platform::DispatchEventResult;
use flui_platform::input_capabilities::InputCapabilities;
use flui_platform::{
    CloseDecision, FrameOutcome, FrameRequest, PlatformInput, SurfaceChange, WindowHandler,
    WindowId,
};

use super::X11Input;
use crate::owner::linux::x11::{
    InputTarget, KeysAsTyped, LoopInput, WindowRef, WindowScale, X11Connection,
};
use crate::owner::window_delivery::WindowDelivery;

/// A handler whose drop sets a window's cursor through the input, as a
/// renderer or a view that resets the cursor as it goes may.
struct ResetsTheCursor {
    input: Weak<X11Input>,
    window: WindowRef,
    dropped: Rc<Cell<bool>>,
}

impl WindowHandler for ResetsTheCursor {
    fn input(&self, _event: PlatformInput) -> DispatchEventResult {
        DispatchEventResult::default()
    }

    fn state_changed(&self) {}

    fn frame_requested(&self, _request: FrameRequest) -> FrameOutcome {
        FrameOutcome::Declined
    }

    fn close_requested(&self) -> CloseDecision {
        CloseDecision::Close
    }

    fn closed(&self) {}

    fn surface_changed(&self, _change: SurfaceChange) {}
}

impl Drop for ResetsTheCursor {
    fn drop(&mut self) {
        if let Some(input) = self.input.upgrade() {
            // Refused or not, it must not find the input borrowed.
            drop(input.set_cursor(self.window, CursorIcon::Default));
        }
        self.dropped.set(true);
    }
}

fn target(input: &Arc<X11Input>, window: WindowRef, dropped: &Rc<Cell<bool>>) -> InputTarget {
    let handler = Rc::new(ResetsTheCursor {
        input: Arc::downgrade(input),
        window,
        dropped: Rc::clone(dropped),
    });
    InputTarget {
        window,
        delivery: Arc::new(WindowDelivery::new(handler, InputCapabilities::UNAVAILABLE)),
        scale: WindowScale::new(1.0),
    }
}

/// A window's entry its X id's next window replaces goes with no borrow of
/// the input held: the handler it lets go of may set a cursor as it drops.
#[test]
fn a_handler_dropped_with_its_entry_may_set_a_cursor() {
    let Ok(connection) = X11Connection::connect() else {
        eprintln!("no X server answers: nothing to run");
        return;
    };
    let input = X11Input::new(&connection, Arc::new(KeysAsTyped));
    let xid = connection.root();
    let dropped = Rc::new(Cell::new(false));
    let first = WindowRef {
        xid,
        id: WindowId(1),
    };
    let later = WindowRef {
        xid,
        id: WindowId(2),
    };
    input.window_opened(target(&input, first, &dropped));
    input.window_opened(target(&input, later, &Rc::new(Cell::new(false))));
    assert!(
        dropped.get(),
        "the first window's handler went with its entry"
    );
}

/// The windows a pointer press reached, as the text input heard them.
#[derive(Debug, Default)]
struct PressesHeard {
    windows: std::sync::Mutex<Vec<x11rb::protocol::xproto::Window>>,
}

impl crate::owner::linux::x11::LoopText for PressesHeard {
    fn window_opened(
        &self,
        _: WindowRef,
        _: WindowScale,
        _: Option<Arc<dyn crate::owner::linux::x11::X11WindowText>>,
    ) {
    }

    fn window_closed(&self, _: WindowRef) {}

    fn event(&self, _: &x11rb::protocol::Event) {}

    fn key(&self, key: crate::owner::linux::x11::X11Key) {
        (key.deliver)();
    }

    fn pointer_pressed(&self, xid: x11rb::protocol::xproto::Window) {
        self.windows
            .lock()
            .expect("BUG: the list is not poisoned")
            .push(xid);
    }

    fn deliver(&self, _: web_time::Instant) {}

    fn deadline(&self) -> Option<web_time::Instant> {
        None
    }

    fn release(&self) {}
}

/// A button press is told to the text input before the window hears it, so
/// the keys the window holds are typed first.
#[test]
fn a_button_press_reaches_the_text_input_first() {
    let Ok(connection) = X11Connection::connect() else {
        eprintln!("no X server answers: nothing to run");
        return;
    };
    let heard = Arc::new(PressesHeard::default());
    let input = X11Input::new(&connection, Arc::clone(&heard) as _);
    let xid = connection.root();
    let press = x11rb::protocol::xinput::ButtonPressEvent {
        event: xid,
        detail: 1,
        ..Default::default()
    };
    input.event(&x11rb::protocol::Event::XinputButtonPress(press.clone()));
    input.event(&x11rb::protocol::Event::XinputButtonRelease(press));
    assert_eq!(
        *heard.windows.lock().expect("BUG: the list is not poisoned"),
        [xid],
        "the press is told, the release is not"
    );
}
