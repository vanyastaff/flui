//! The live input on the X server `DISPLAY` names; each case passes as
//! untested where none answers.

use std::cell::Cell;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use cursor_icon::CursorIcon;
use flui_platform::DispatchEventResult;
use flui_platform::input::capabilities::InputCapabilities;
use flui_platform::{
    CloseDecision, Cursor, FrameOutcome, FrameRequest, PlatformInput, SurfaceChange, WindowHandler,
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
            drop(input.set_cursor(self.window, &Cursor::Icon(CursorIcon::Default)));
        }
        self.dropped.set(true);
    }
}

fn target(input: &Rc<X11Input>, window: WindowRef, dropped: &Rc<Cell<bool>>) -> InputTarget {
    let handler = Rc::new(ResetsTheCursor {
        input: Rc::downgrade(input),
        window,
        dropped: Rc::clone(dropped),
    });
    InputTarget {
        window,
        delivery: Arc::new(WindowDelivery::new(handler, InputCapabilities::UNAVAILABLE)),
        scale: WindowScale::new(flui_foundation::geometry::DevicePixelRatio::ONE),
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
    let input = X11Input::new(&connection, Rc::new(KeysAsTyped));
    let xid = connection.root();
    let dropped = Rc::new(Cell::new(false));
    let first = WindowRef {
        xid,
        id: WindowId::try_from_raw(1).expect("a nonzero window id"),
    };
    let later = WindowRef {
        xid,
        id: WindowId::try_from_raw(2).expect("a nonzero window id"),
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
    let heard = Rc::new(PressesHeard::default());
    let input = X11Input::new(&connection, Rc::clone(&heard) as _);
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

/// A change of the server's keymap marks the keymap and reads nothing: it is
/// read once, before the next key or as the turn's events end, so a burst of
/// changes costs what one costs. Counted in the requests the connection
/// makes, each of which takes the next sequence number.
#[test]
fn a_burst_of_keymap_changes_reads_the_keymap_once() {
    use x11rb::protocol::Event;
    use x11rb::protocol::xinput::KeyPressEvent;
    use x11rb::protocol::xkb::MapNotifyEvent;
    use x11rb::protocol::xproto::ConnectionExt as _;

    /// An event of a turn.
    #[derive(Clone, Copy)]
    enum Heard {
        /// The server's keymap changed.
        Change,
        /// A key was pressed, in no window of the input's.
        Key,
    }

    let Ok(connection) = X11Connection::connect() else {
        eprintln!("no X server answers: nothing to run");
        return;
    };
    let input = X11Input::new(&connection, Rc::new(KeysAsTyped));
    let probes = Cell::new(0);
    // The requests made so far, these probes aside.
    let requests_made = || {
        let cookie = connection.xcb().get_input_focus().expect("request");
        let sequence = cookie.sequence_number();
        cookie.reply().expect("the server answers");
        probes.set(probes.get() + 1);
        sequence - probes.get()
    };
    let hear = |heard: Heard| {
        let event = match heard {
            Heard::Change => Event::XkbMapNotify(MapNotifyEvent::default()),
            Heard::Key => Event::XinputKeyPress(KeyPressEvent::default()),
        };
        input.event(&event);
    };

    let before = requests_made();
    hear(Heard::Change);
    assert_eq!(requests_made(), before, "a change reads nothing");
    input.settle();
    let one_read = requests_made() - before;
    if one_read == 0 {
        eprintln!("the X server has no XKB keymap to read: nothing to run");
        return;
    }

    use Heard::{Change, Key};
    // (case, the events of a turn, the reads of the keymap before the events
    //  end, the reads in all)
    let table: [(&str, Vec<Heard>, u64, u64); 3] = [
        ("a burst of changes", vec![Change; 1_000], 0, 1),
        (
            "keys after a burst of changes",
            vec![Change, Change, Key, Key],
            1,
            1,
        ),
        ("changes around a key", vec![Change, Key, Change], 1, 2),
    ];
    for (case, events, before_the_end, in_all) in table {
        let before = requests_made();
        events.into_iter().for_each(hear);
        assert_eq!(
            requests_made() - before,
            before_the_end * one_read,
            "{case}: read before the events end"
        );
        input.settle();
        assert_eq!(
            requests_made() - before,
            in_all * one_read,
            "{case}: read in all"
        );
    }
}

/// A window's mask reaches every event it selects: XInput 2.4's swipe end
/// (type 32) sets the first bit of a second word, which a single 32-bit word
/// cannot hold.
#[test]
fn an_event_mask_takes_a_word_for_each_thirty_two_types() {
    use x11rb::protocol::xinput::{self, XIEventMask};
    assert_eq!(
        super::event_mask(&[xinput::MOTION_EVENT, xinput::GESTURE_SWIPE_END_EVENT]),
        [
            XIEventMask::from(1_u32 << xinput::MOTION_EVENT),
            XIEventMask::from(1_u32),
        ]
    );
    assert_eq!(super::event_mask(&[]), Vec::<XIEventMask>::new());
}
