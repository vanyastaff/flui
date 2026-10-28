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
    (
        "losing_the_focus_ends_a_dead_key_and_releases_held_keys",
        losing_the_focus_ends_a_dead_key_and_releases_held_keys,
    ),
    (
        "a_window_follows_its_focus_when_a_handler_panics_at_its_loss",
        a_window_follows_its_focus_when_a_handler_panics_at_its_loss,
    ),
];

/// `dead_acute` (`keysymdef.h`).
const DEAD_ACUTE: u32 = 0xfe51;
/// A key code the server's evdev keymap leaves free, which a case maps.
const FREE_KEYCODE: u8 = 247;
/// `e`.
const KEYCODE_E: u8 = 26;

/// Give `keycode` the one keysym `keysym` in the server's core keymap; XKB
/// tells every client its keymap changed.
fn map_key(fixture: &X11Fixture, keycode: u8, keysym: u32) {
    fixture
        .server
        .change_keyboard_mapping(1, keycode, 1, &[keysym])
        .expect("request");
    fixture.server.flush().expect("flush");
    run_for(&fixture.platform, Duration::from_millis(100)).expect("the loop turns");
}

/// Move the focus off every window, to the root, as another client may.
///
/// The focus goes to a window of the test's own, mapped for it, rather than
/// to the root: openbox gives the focus back to the last active window when
/// it lands on the root ("focus fallback"), at a moment the case cannot
/// control, and a case that reads the focus would race it.
fn focus_elsewhere(fixture: &X11Fixture) -> u32 {
    use x11rb::protocol::xproto::{CreateWindowAux, InputFocus, MapState, WindowClass};
    let server = &fixture.server;
    let elsewhere = server.generate_id().expect("an id");
    server
        .create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            elsewhere,
            fixture.root(),
            900,
            700,
            50,
            50,
            0,
            WindowClass::INPUT_OUTPUT,
            x11rb::COPY_FROM_PARENT,
            &CreateWindowAux::new(),
        )
        .expect("request");
    server.map_window(elsewhere).expect("request");
    server.flush().expect("flush");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while server
        .get_window_attributes(elsewhere)
        .expect("request")
        .reply()
        .expect("its attributes")
        .map_state
        != MapState::VIEWABLE
    {
        assert!(
            std::time::Instant::now() < deadline,
            "the window manager maps the other window"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    server
        .set_input_focus(InputFocus::PARENT, elsewhere, x11rb::CURRENT_TIME)
        .expect("request");
    server.flush().expect("flush");
    elsewhere
}

fn key_downs(handler: &RecordingWindowHandler) -> Vec<KeyEvent> {
    handler
        .inputs()
        .into_iter()
        .filter_map(|input| match input {
            PlatformInput::Keyboard(key) => Some(key),
            _ => None,
        })
        .collect()
}

/// A dead key pressed before the window loses the focus does not compose
/// with the key pressed after it comes back; and a key held as the focus goes
/// is released in the window it was pressed in.
fn losing_the_focus_ends_a_dead_key_and_releases_held_keys() {
    let handler = RecordingWindowHandler::new();
    let mut fixture = X11Fixture::open_with("dead keys", Rc::clone(&handler));
    map_key(&fixture, FREE_KEYCODE, DEAD_ACUTE);
    let activate = |fixture: &mut X11Fixture| {
        fixture
            .window
            .admit_and_apply(WindowCommand::Activate)
            .expect("the window activates");
        fixture.settle();
        assert!(fixture.window.state().focused, "the window has the focus");
    };
    activate(&mut fixture);
    let xtest = Xtest::new(&fixture);
    // Without a focus change the two compose: compose is there.
    xtest.tap(FREE_KEYCODE);
    xtest.tap(KEYCODE_E);
    let composed: Vec<Key> = key_downs(&handler)
        .into_iter()
        .filter(|key| key.state() == KeyState::Down)
        .map(|key| key.key)
        .collect();
    assert_eq!(
        composed,
        [Key::Named(NamedKey::Dead), Key::character("é")],
        "the dead key composes with e"
    );
    handler.take_calls();
    // The dead key, then the focus goes while Shift is held.
    xtest.tap(FREE_KEYCODE);
    xtest.key(keycode::SHIFT_L, true);
    focus_elsewhere(&fixture);
    run_for(&fixture.platform, Duration::from_millis(100)).expect("the loop turns");
    xtest.key(keycode::SHIFT_L, false);
    let released: Vec<(KeyState, Key)> = key_downs(&handler)
        .into_iter()
        .map(|key| (key.state(), key.key))
        .collect();
    assert_eq!(
        released.last(),
        Some(&(KeyState::Up, Key::Named(NamedKey::Shift))),
        "the held Shift is released as the focus goes: {released:?}"
    );
    handler.take_calls();
    activate(&mut fixture);
    let xtest = Xtest::new(&fixture);
    xtest.tap(KEYCODE_E);
    let after: Vec<Key> = key_downs(&handler)
        .into_iter()
        .filter(|key| key.state() == KeyState::Down)
        .map(|key| key.key)
        .collect();
    assert_eq!(
        after,
        [Key::character("e")],
        "the dead key pressed before the focus went is gone"
    );
    map_key(&fixture, FREE_KEYCODE, 0);
}

/// A handler that panics at a key's release: the one the focus loss
/// delivers for a key held as the focus goes.
struct PanicsAtTheRelease(Rc<RecordingWindowHandler>);

impl flui_platform::WindowHandler for PanicsAtTheRelease {
    fn input(&self, event: PlatformInput) -> flui_platform::DispatchEventResult {
        if let PlatformInput::Keyboard(key) = &event
            && key.state() == KeyState::Up
        {
            panic!("the handler panics at the release");
        }
        self.0.input(event)
    }

    fn state_changed(&self) {
        self.0.state_changed();
    }

    fn frame_requested(&self, request: flui_platform::FrameRequest) -> flui_platform::FrameOutcome {
        self.0.frame_requested(request)
    }

    fn close_requested(&self) -> flui_platform::CloseDecision {
        self.0.close_requested()
    }

    fn closed(&self) {
        self.0.closed();
    }

    fn surface_changed(&self, change: flui_platform::SurfaceChange) {
        self.0.surface_changed(change);
    }
}

/// The window hears its focus going even when the handler panics at what the
/// input delivers for that loss first (the release of a key held as the focus
/// goes); the panic is raised after.
fn a_window_follows_its_focus_when_a_handler_panics_at_its_loss() {
    park_pointer();
    let mut fixture = X11Fixture::open_with_handler(
        "focus and a panic",
        Rc::new(PanicsAtTheRelease(RecordingWindowHandler::new())),
    );
    fixture
        .window
        .admit_and_apply(WindowCommand::Activate)
        .expect("the window activates");
    fixture.settle();
    assert!(fixture.window.state().focused, "the window has the focus");
    let xtest = Xtest::new(&fixture);
    xtest.key(keycode::A, true);
    focus_elsewhere(&fixture);
    let turned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_for(&fixture.platform, Duration::from_millis(100))
    }));
    assert!(turned.is_err(), "the handler's panic is raised");
    assert!(
        !fixture.window.state().focused,
        "the window heard its focus go"
    );
    fixture
        .server
        .xtest_fake_input(KEY_RELEASE_EVENT, keycode::A, 0, fixture.root(), 0, 0, 0)
        .expect("request");
    fixture.server.flush().expect("flush");
}

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
    assert_eq!(fixture.platform.keyboard_layout(), "English (US)");
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
