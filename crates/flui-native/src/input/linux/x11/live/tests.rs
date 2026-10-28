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
use crate::owner::linux::x11::{InputTarget, LoopInput, WindowRef, WindowScale, X11Connection};
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
    let input = X11Input::new(&connection);
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
