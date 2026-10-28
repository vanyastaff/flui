//! The X11 host's input on the harness's X server: XTest moves the pointer,
//! presses its buttons, turns its wheel and types, as a user's devices do,
//! and the window's handler hears what the host decoded from XInput 2 and
//! XKB. With `native-capture`, the pointer session journals a native
//! recording that replays to what the window delivered and holds against its
//! golden.

use std::rc::Rc;
use std::time::Duration;

use cursor_icon::CursorIcon;
use flui_native::Platform as _;
use flui_native::testing::x11::run_for;
use flui_platform::keyboard::{Code, Key, KeyEvent, KeyState, Modifiers, NamedKey};
use flui_platform::pointer::{ButtonChange, PointerEvent};
use flui_platform::{PlatformInput, UnsupportedReason, WindowCommand};
use flui_testing::window_commands::AdmitAndApply as _;
use flui_testing::window_conformance::WindowFixture as _;
use flui_testing::window_handler::RecordingWindowHandler;
use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{
    BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT, ConnectionExt as _, KEY_PRESS_EVENT,
    KEY_RELEASE_EVENT, MOTION_NOTIFY_EVENT,
};
use x11rb::protocol::xtest::ConnectionExt as _;

use crate::cases::X11Fixture;

pub(crate) const CASES: &[(&str, fn())] = &[
    (
        "xtest_pointer_buttons_and_wheel_reach_the_window",
        xtest_pointer_buttons_and_wheel_reach_the_window,
    ),
    (
        "xtest_keys_reach_the_focused_window",
        xtest_keys_reach_the_focused_window,
    ),
    (
        "a_window_shows_the_cursor_it_asks_for",
        a_window_shows_the_cursor_it_asks_for,
    ),
];

/// Evdev key codes as the X server numbers them (the kernel's plus 8).
mod keycode {
    pub(super) const ESCAPE: u8 = 9;
    pub(super) const CONTROL_L: u8 = 37;
    pub(super) const A: u8 = 38;
    pub(super) const SHIFT_L: u8 = 50;
    pub(super) const F10: u8 = 76;
    pub(super) const MENU: u8 = 135;
}

/// One XTest request at a time, each followed by turns of the host's loop.
struct Xtest<'a> {
    fixture: &'a X11Fixture,
    /// The window's content origin on the root window.
    origin: (i16, i16),
}

impl<'a> Xtest<'a> {
    fn new(fixture: &'a X11Fixture) -> Self {
        let origin = fixture
            .server
            .translate_coordinates(fixture.xid, fixture.root(), 0, 0)
            .expect("request")
            .reply()
            .expect("the content's place");
        Self {
            fixture,
            origin: (origin.dst_x, origin.dst_y),
        }
    }

    fn send(&self, kind: u8, detail: u8, at: (i16, i16)) {
        self.fixture
            .server
            .xtest_fake_input(kind, detail, 0, self.fixture.root(), at.0, at.1, 0)
            .expect("request");
        self.fixture.server.flush().expect("flush");
        run_for(&self.fixture.platform, Duration::from_millis(40)).expect("the loop turns");
    }

    /// Move the pointer to `(x, y)` of the window's content.
    fn move_to(&self, x: i16, y: i16) {
        self.send(
            MOTION_NOTIFY_EVENT,
            0,
            (self.origin.0 + x, self.origin.1 + y),
        );
    }

    /// Move the pointer to `(x, y)` of the root window.
    fn move_to_root(&self, x: i16, y: i16) {
        self.send(MOTION_NOTIFY_EVENT, 0, (x, y));
    }

    fn click(&self, button: u8) {
        self.send(BUTTON_PRESS_EVENT, button, (0, 0));
        self.send(BUTTON_RELEASE_EVENT, button, (0, 0));
    }

    fn key(&self, keycode: u8, pressed: bool) {
        let kind = if pressed {
            KEY_PRESS_EVENT
        } else {
            KEY_RELEASE_EVENT
        };
        self.send(kind, keycode, (0, 0));
    }

    fn tap(&self, keycode: u8) {
        self.key(keycode, true);
        self.key(keycode, false);
    }
}

/// Put the pointer in the screen's far corner, where no window opens, before
/// a window opens.
fn park_pointer() {
    let (server, screen) =
        x11rb::rust_connection::RustConnection::connect(None).expect("the X server answers");
    let root = server.setup().roots[screen].root;
    server
        .xtest_fake_input(MOTION_NOTIFY_EVENT, 0, 0, root, 1270, 1010, 0)
        .expect("request");
    // A round trip: the server has moved the pointer.
    server
        .get_input_focus()
        .expect("request")
        .reply()
        .expect("the focus");
}

/// `event` in the terms the cases compare: what it is, where, which button,
/// the Control it carries.
fn line(event: &PointerEvent) -> Option<String> {
    let control = |modifiers: Modifiers| {
        if modifiers.contains(Modifiers::CONTROL) {
            " ctrl"
        } else {
            ""
        }
    };
    Some(match event {
        PointerEvent::Enter(_) => "enter".to_owned(),
        PointerEvent::Leave(_) => "leave".to_owned(),
        PointerEvent::Move(movement) => {
            let at = movement.current().position.get();
            format!("move ({}, {})", at.x, at.y)
        }
        PointerEvent::Down(press) | PointerEvent::ButtonChange(ButtonChange::Pressed(press)) => {
            format!("down {:?}{}", press.button(), control(press.modifiers))
        }
        PointerEvent::Up(release) | PointerEvent::ButtonChange(ButtonChange::Released(release)) => {
            format!("up {:?}", release.button())
        }
        PointerEvent::Scroll(scroll) => format!(
            "scroll ({}, {}) {:?}",
            scroll.delta.x(),
            scroll.delta.y(),
            scroll.precision
        ),
        _ => return None,
    })
}

fn pointer_events(handler: &RecordingWindowHandler) -> Vec<PointerEvent> {
    handler
        .inputs()
        .into_iter()
        .filter_map(|input| match input {
            PlatformInput::Pointer(event) => Some(event),
            _ => None,
        })
        .collect()
}

/// The pointer session: in, a left click with a drag, a right click, the
/// wheel down and up, a Control-held click, and out.
fn pointer_session(fixture: &X11Fixture) {
    let xtest = Xtest::new(fixture);
    xtest.move_to(50, 40);
    xtest.send(BUTTON_PRESS_EVENT, 1, (0, 0));
    xtest.move_to(60, 40);
    xtest.send(BUTTON_RELEASE_EVENT, 1, (0, 0));
    xtest.click(3);
    xtest.click(5);
    xtest.click(4);
    xtest.key(keycode::CONTROL_L, true);
    xtest.click(1);
    xtest.key(keycode::CONTROL_L, false);
    xtest.move_to_root(1270, 1010);
}

/// XTest's pointer reaches the window as XInput 2 decodes it: an enter, a
/// press and its drag, the right button, a whole detent of the wheel each
/// way (XTest's pointer has no scroll valuator, so the legacy wheel buttons
/// are read), the modifiers held, and a leave.
fn xtest_pointer_buttons_and_wheel_reach_the_window() {
    // The platform is built inside the capture, which journals the registry
    // its input makes.
    let session = || {
        park_pointer();
        let handler = RecordingWindowHandler::new();
        let mut fixture = X11Fixture::open_with("xtest pointer", Rc::clone(&handler));
        fixture
            .window
            .admit_and_apply(WindowCommand::Activate)
            .expect("the window activates");
        fixture.settle();
        pointer_session(&fixture);
        let delivered = pointer_events(&handler);
        drop(fixture);
        delivered
    };
    #[cfg(all(debug_assertions, feature = "native-capture"))]
    let (delivered, recordings) = {
        use crate::native_recording::capture::record;
        use crate::native_recording::codec::Backend;
        record(Backend::X11, session)
    };
    #[cfg(not(all(debug_assertions, feature = "native-capture")))]
    let delivered = session();

    let lines: Vec<String> = delivered.iter().filter_map(line).collect();
    let expected = [
        "enter",
        "move (50, 40)",
        "down PRIMARY",
        "move (60, 40)",
        "up PRIMARY",
        "down SECONDARY",
        "up SECONDARY",
        "scroll (0, 1) Notched",
        "scroll (0, -1) Notched",
        "down PRIMARY ctrl",
        "up PRIMARY",
        "leave",
    ];
    assert_eq!(lines, expected, "{delivered:#?}");

    #[cfg(all(debug_assertions, feature = "native-capture"))]
    {
        let [recording] = recordings.as_slice() else {
            panic!("one input, one registry: {recordings:?}");
        };
        // The devices were announced before the window opened, to no one.
        let contact = |event: &&PointerEvent| {
            !matches!(
                event,
                PointerEvent::DeviceAdded(_) | PointerEvent::DeviceRemoved(_)
            )
        };
        let replayed: Vec<String> = recording
            .replay()
            .iter()
            .filter_map(PlatformInput::as_pointer)
            .filter(contact)
            .map(|event| format!("{event:?}"))
            .collect();
        let delivered: Vec<String> = delivered
            .iter()
            .filter(contact)
            .map(|event| format!("{event:?}"))
            .collect();
        assert_eq!(
            replayed, delivered,
            "the journal replays to what the window delivered"
        );
        crate::native_recording::capture::hold_against_golden(
            "x11_xtest_mouse_and_wheel",
            recording,
        );
    }
}

/// XTest's keys reach the focused window through XKB: a letter, the same
/// letter with Shift held, and the keys the default bindings name (Escape,
/// F10, the menu key), with their physical keys; the layout is the server's.
fn xtest_keys_reach_the_focused_window() {
    let handler = RecordingWindowHandler::new();
    let mut fixture = X11Fixture::open_with("xtest keys", Rc::clone(&handler));
    fixture
        .window
        .admit_and_apply(WindowCommand::Activate)
        .expect("the window activates");
    fixture.settle();
    assert!(fixture.window.state().focused, "the window has the focus");
    handler.take_calls();
    let xtest = Xtest::new(&fixture);
    xtest.tap(keycode::A);
    xtest.key(keycode::SHIFT_L, true);
    xtest.tap(keycode::A);
    xtest.key(keycode::SHIFT_L, false);
    for key in [keycode::ESCAPE, keycode::F10, keycode::MENU] {
        xtest.tap(key);
    }
    let keys: Vec<KeyEvent> = handler
        .inputs()
        .into_iter()
        .filter_map(|input| match input {
            PlatformInput::Keyboard(key) => Some(key),
            _ => None,
        })
        .collect();
    let downs: Vec<(Key, Code, bool)> = keys
        .iter()
        .filter(|key| key.state() == KeyState::Down)
        .map(|key| {
            (
                key.key.clone(),
                key.code,
                key.modifiers.contains(Modifiers::SHIFT),
            )
        })
        .collect();
    assert_eq!(
        downs,
        [
            (Key::character("a"), Code::KeyA, false),
            (Key::Named(NamedKey::Shift), Code::ShiftLeft, false),
            (Key::character("A"), Code::KeyA, true),
            (Key::Named(NamedKey::Escape), Code::Escape, false),
            (Key::Named(NamedKey::F10), Code::F10, false),
            (Key::Named(NamedKey::ContextMenu), Code::ContextMenu, false),
        ],
        "{keys:#?}"
    );
    assert_eq!(
        keys.iter()
            .filter(|key| key.state() == KeyState::Up)
            .count(),
        downs.len(),
        "every key is released"
    );
    assert_eq!(fixture.platform.keyboard_layout(), "us");
}

/// A window shows the cursor it asks for, from the theme or the cursor
/// font; a closed one has none to show.
fn a_window_shows_the_cursor_it_asks_for() {
    let mut fixture = X11Fixture::open_with("cursor", RecordingWindowHandler::new());
    for cursor in [CursorIcon::Pointer, CursorIcon::Text, CursorIcon::Default] {
        fixture
            .window
            .set_cursor(cursor)
            .unwrap_or_else(|error| panic!("{cursor:?}: {error}"));
    }
    fixture
        .window
        .admit_and_apply(WindowCommand::Close)
        .expect("the window closes");
    fixture.settle();
    let refused = fixture
        .window
        .set_cursor(CursorIcon::Pointer)
        .expect_err("a closed window shows no cursor");
    assert_eq!(
        refused.unsupported().map(|unsupported| unsupported.reason),
        Some(UnsupportedReason::NoWindow)
    );
}
