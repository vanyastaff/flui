//! The X11 host's input on the harness's X server: XTest moves the pointer,
//! presses its buttons, turns its wheel and types, as a user's devices do,
//! and the window's handler hears what the host decoded from XInput 2 and
//! XKB. With `native-capture`, the pointer session journals a native
//! recording that replays to what the window delivered and holds against its
//! golden.

use std::rc::Rc;
use std::time::Duration;

use cursor_icon::CursorIcon;
use flui_native::testing::keyboard_layout;
use flui_native::testing::x11::run_for;
use flui_platform::keyboard::{Code, Key, KeyEvent, KeyPress, KeyState, Modifiers, NamedKey};
use flui_platform::pointer::{ButtonChange, PointerEvent};
use flui_platform::{Cursor, PlatformInput, UnsupportedReason, WindowCommand};
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
        "a_focused_window_holds_the_pointer_as_it_asks",
        a_focused_window_holds_the_pointer_as_it_asks,
    ),
    (
        "a_window_keeps_the_level_it_asks_for",
        a_window_keeps_the_level_it_asks_for,
    ),
    (
        "a_refused_hold_leaves_the_window_holding_none",
        a_refused_hold_leaves_the_window_holding_none,
    ),
    (
        "a_locked_pointer_stays_with_the_content_as_the_window_moves",
        a_locked_pointer_stays_with_the_content_as_the_window_moves,
    ),
    (
        "a_level_holds_through_fullscreen",
        a_level_holds_through_fullscreen,
    ),
    (
        "losing_the_focus_ends_a_dead_key_and_releases_held_keys",
        losing_the_focus_ends_a_dead_key_and_releases_held_keys,
    ),
    (
        "a_window_follows_its_focus_when_a_handler_panics_at_its_loss",
        a_window_follows_its_focus_when_a_handler_panics_at_its_loss,
    ),
    (
        "an_xtest_drag_from_the_window_drops_on_another_client",
        an_xtest_drag_from_the_window_drops_on_another_client,
    ),
    (
        "an_xtest_escape_cancels_a_drag_after_the_focus_moved",
        an_xtest_escape_cancels_a_drag_after_the_focus_moved,
    ),
    (
        "a_title_bar_of_the_applications_moves_and_resizes_through_the_window_manager",
        a_title_bar_of_the_applications_moves_and_resizes_through_the_window_manager,
    ),
    (
        "openbox_paces_an_interactive_resize_by_the_windows_counter",
        openbox_paces_an_interactive_resize_by_the_windows_counter,
    ),
    (
        "a_burst_of_motion_is_one_move_through_it",
        a_burst_of_motion_is_one_move_through_it,
    ),
    (
        "a_master_pointer_comes_and_goes_with_the_hierarchy",
        a_master_pointer_comes_and_goes_with_the_hierarchy,
    ),
    (
        "a_panicking_layout_callback_costs_no_key_of_its_turn",
        a_panicking_layout_callback_costs_no_key_of_its_turn,
    ),
    (
        "a_second_layout_callback_replaces_the_first",
        a_second_layout_callback_replaces_the_first,
    ),
];

/// A key the server sends in the turn that also carries a keyboard layout
/// change reaches the window even when the layout callback panics: the
/// callback hears the change once the turn's events are delivered, and its
/// panic leaves the loop after them.
fn a_panicking_layout_callback_costs_no_key_of_its_turn() {
    /// Puts the server's keymap back for the tests after this one.
    struct RestoreLayout;
    impl Drop for RestoreLayout {
        fn drop(&mut self) {
            let _ = std::process::Command::new("setxkbmap")
                .args(["-layout", "us"])
                .status();
        }
    }
    let handler = RecordingWindowHandler::new();
    let mut fixture = X11Fixture::open_with("layout panic", Rc::clone(&handler));
    fixture
        .window
        .admit_and_apply(WindowCommand::Activate)
        .expect("the window activates");
    fixture.settle();
    assert!(
        fixture.window.state().focus.is_focused(),
        "the window has the focus"
    );
    keyboard_layout::on_change(
        &fixture.platform,
        Box::new(|| panic!("the layout callback")),
    );
    handler.take_calls();
    let _restore = RestoreLayout;
    // The server switches the keymap; the loop does not turn before the key.
    let switched = std::process::Command::new("setxkbmap")
        .args(["-layout", "de"])
        .status()
        .expect("setxkbmap, which the X server's own xkbcomp comes with, runs");
    assert!(switched.success(), "the keymap switches");
    for kind in [KEY_PRESS_EVENT, KEY_RELEASE_EVENT] {
        fixture
            .server
            .xtest_fake_input(kind, keycode::A, 0, fixture.root(), 0, 0, 0)
            .expect("request");
    }
    // A round trip: once the server answered it, it has sent the keymap's
    // notices and both keys to the platform's connection, so one turn reads
    // them all.
    fixture
        .server
        .get_input_focus()
        .expect("request")
        .reply()
        .expect("the server answers");
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut raised = 0;
    let keys = || {
        handler
            .inputs()
            .into_iter()
            .filter_map(|input| match input {
                PlatformInput::Keyboard(key) => Some(key.state),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    while keys().len() < 2 {
        let turned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_for(&fixture.platform, Duration::from_millis(40))
        }));
        if turned.is_err() {
            raised += 1;
        }
        assert!(std::time::Instant::now() < deadline, "no key came");
    }
    assert_eq!(keyboard_layout::current(&fixture.platform), "German");
    assert_eq!(
        keys(),
        [KeyState::Down(KeyPress::First), KeyState::Up],
        "a key of the layout change's turn was lost to the callback's panic"
    );
    assert_eq!(raised, 1, "the callback's panic left the loop once");
    // The keymap goes back as the test ends: no callback panics at it.
    keyboard_layout::on_change(&fixture.platform, Box::new(|| {}));
}

/// A second keyboard layout callback replaces the first, as on every host:
/// the first is dropped at the registration, so it never hears a change
/// again.
fn a_second_layout_callback_replaces_the_first() {
    struct OnDrop(Rc<std::cell::Cell<bool>>);
    impl Drop for OnDrop {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }
    let fixture = X11Fixture::open_with("layout callbacks", RecordingWindowHandler::new());
    let dropped = Rc::new(std::cell::Cell::new(false));
    let guard = OnDrop(Rc::clone(&dropped));
    keyboard_layout::on_change(
        &fixture.platform,
        Box::new(move || {
            let _ = &guard;
        }),
    );
    assert!(!dropped.get(), "the first callback is kept until replaced");
    keyboard_layout::on_change(&fixture.platform, Box::new(|| {}));
    assert!(
        dropped.get(),
        "the first callback was kept beside the second"
    );
}

/// The motions the server sent before the loop turned reach the window as
/// one move at the last position, which carries the earlier ones: a burst
/// costs the window one move, and no position of it is lost.
fn a_burst_of_motion_is_one_move_through_it() {
    /// How many motions the server sends at once.
    const BURST: i16 = 40;

    park_pointer();
    let handler = RecordingWindowHandler::new();
    let mut fixture = X11Fixture::open_with("motion burst", Rc::clone(&handler));
    fixture
        .window
        .admit_and_apply(WindowCommand::Activate)
        .expect("the window activates");
    fixture.settle();
    let xtest = Xtest::new(&fixture);
    xtest.move_to(20, 40);
    handler.take_calls();
    for step in 1..=BURST {
        fixture
            .server
            .xtest_fake_input(
                MOTION_NOTIFY_EVENT,
                0,
                0,
                fixture.root(),
                xtest.origin.0 + 20 + step,
                xtest.origin.1 + 40,
                0,
            )
            .expect("request");
    }
    // Two round trips: the server has sent the window every motion before
    // the loop turns.
    for _ in 0..2 {
        fixture
            .server
            .get_input_focus()
            .expect("request")
            .reply()
            .expect("the focus");
    }
    run_for(&fixture.platform, Duration::from_millis(100)).expect("the loop turns");

    let moves: Vec<_> = pointer_events(&handler)
        .into_iter()
        .filter_map(|event| match event {
            PointerEvent::Move(movement) => Some(movement),
            _ => None,
        })
        .collect();
    let positions: Vec<f64> = moves
        .iter()
        .flat_map(|movement| {
            movement
                .coalesced()
                .iter()
                .chain(std::iter::once(movement.current()))
                .map(|sample| sample.position.get().x)
        })
        .collect();
    let expected: Vec<f64> = (1..=BURST).map(|step| f64::from(20 + step)).collect();
    assert_eq!(positions, expected, "every position, in order");
    assert!(
        moves.len() <= 2,
        "{} moves for a burst the loop read at once",
        moves.len()
    );
}

/// A master pointer the server adds is a mouse the window hears of, and one
/// it removes is taken away. The server reports a removed device with no
/// use, so only the flag says a master pointer went.
fn a_master_pointer_comes_and_goes_with_the_hierarchy() {
    use flui_platform::pointer::PointerKind;
    use x11rb::protocol::xinput::{
        self, ChangeMode, ConnectionExt as _, HierarchyChange, HierarchyChangeData,
        HierarchyChangeDataAddMaster, HierarchyChangeDataRemoveMaster,
    };

    let handler = RecordingWindowHandler::new();
    let fixture = X11Fixture::open_with("second master", Rc::clone(&handler));
    handler.take_calls();
    let server = &fixture.server;
    let mice = |events: Vec<PointerEvent>, added: bool| -> Vec<_> {
        events
            .into_iter()
            .filter_map(|event| match event {
                PointerEvent::DeviceAdded(change) if added => Some(change),
                PointerEvent::DeviceRemoved(change) if !added => Some(change),
                _ => None,
            })
            .filter(|change| change.kind == PointerKind::Mouse)
            .map(|change| change.device)
            .collect()
    };
    // A client says which XInput 2 it speaks before its first request.
    server
        .xinput_xi_query_version(2, 2)
        .expect("request")
        .reply()
        .expect("XInput 2");
    // The lengths are in 4-byte units: the header, then the change.
    server
        .xinput_xi_change_hierarchy(&[HierarchyChange {
            len: 3,
            data: HierarchyChangeData::AddMaster(HierarchyChangeDataAddMaster {
                send_core: true,
                enable: true,
                name: b"flui".to_vec(),
            }),
        }])
        .expect("request")
        .check()
        .expect("the server adds a master pointer");
    run_for(&fixture.platform, Duration::from_millis(100)).expect("the loop turns");
    let added = mice(pointer_events(&handler), true);
    assert_eq!(added.len(), 1, "the new master's cursor is announced");

    let master = server
        .xinput_xi_query_device(xinput::Device::ALL_MASTER)
        .expect("request")
        .reply()
        .expect("the masters")
        .infos
        .into_iter()
        .find(|info| info.name == b"flui pointer")
        .expect("the master pointer the server added")
        .deviceid;
    handler.take_calls();
    server
        .xinput_xi_change_hierarchy(&[HierarchyChange {
            len: 3,
            data: HierarchyChangeData::RemoveMaster(HierarchyChangeDataRemoveMaster {
                deviceid: master,
                return_mode: ChangeMode::FLOAT,
                return_pointer: 0,
                return_keyboard: 0,
            }),
        }])
        .expect("request")
        .check()
        .expect("the server removes the master pointer");
    run_for(&fixture.platform, Duration::from_millis(100)).expect("the loop turns");
    assert_eq!(
        mice(pointer_events(&handler), false),
        added,
        "the master that went is the one that came"
    );
}

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
        assert!(
            fixture.window.state().focus.is_focused(),
            "the window has the focus"
        );
    };
    activate(&mut fixture);
    let xtest = Xtest::new(&fixture);
    // Without a focus change the two compose: compose is there.
    xtest.tap(FREE_KEYCODE);
    xtest.tap(KEYCODE_E);
    let composed: Vec<Key> = key_downs(&handler)
        .into_iter()
        .filter(|key| matches!(key.state, KeyState::Down(_)))
        .map(|key| key.key)
        .collect();
    assert_eq!(
        composed,
        [Key::Dead(Some('\u{b4}')), Key::character("é")],
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
        .map(|key| (key.state, key.key))
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
        .filter(|key| matches!(key.state, KeyState::Down(_)))
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
            && key.state == KeyState::Up
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
    assert!(
        fixture.window.state().focus.is_focused(),
        "the window has the focus"
    );
    let xtest = Xtest::new(&fixture);
    xtest.key(keycode::A, true);
    focus_elsewhere(&fixture);
    let turned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_for(&fixture.platform, Duration::from_millis(100))
    }));
    assert!(turned.is_err(), "the handler's panic is raised");
    assert!(
        !fixture.window.state().focus.is_focused(),
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
pub(crate) fn park_pointer() {
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
            .expect("a registry's recording replays")
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
    assert!(
        fixture.window.state().focus.is_focused(),
        "the window has the focus"
    );
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
        .filter(|key| matches!(key.state, KeyState::Down(_)))
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
        keys.iter().filter(|key| key.state == KeyState::Up).count(),
        downs.len(),
        "every key is released"
    );
    assert_eq!(keyboard_layout::current(&fixture.platform), "English (US)");
}

/// A window shows the cursor it asks for, from the theme or the cursor
/// font, none, or an image made a Render cursor, the same image again
/// included; a closed one has none to show.
fn a_window_shows_the_cursor_it_asks_for() {
    let mut fixture = X11Fixture::open_with("cursor", RecordingWindowHandler::new());
    let image = flui_platform::CursorImage::try_new(16, 16, (3, 5), vec![0x80; 16 * 16 * 4])
        .expect("a 16x16 image");
    let cursors = [
        Cursor::Icon(CursorIcon::Pointer),
        Cursor::Icon(CursorIcon::Text),
        Cursor::Hidden,
        Cursor::Image(image.clone()),
        Cursor::Image(image),
        Cursor::Icon(CursorIcon::Default),
    ];
    for cursor in &cursors {
        fixture
            .window
            .set_cursor(cursor)
            .unwrap_or_else(|error| panic!("{cursor:?}: {error}"));
    }
    fixture.settle();
    fixture
        .window
        .admit_and_apply(WindowCommand::Close)
        .expect("the window closes");
    fixture.settle();
    let refused = fixture
        .window
        .set_cursor(&Cursor::Icon(CursorIcon::Pointer))
        .expect_err("a closed window shows no cursor");
    assert_eq!(
        refused.unsupported().map(|unsupported| unsupported.reason),
        Some(UnsupportedReason::NoWindow)
    );
}

/// A focused window holds the pointer as it asks, as the server's grab no
/// other client can take: confined, then locked, its motion arriving as raw
/// motion, unaccelerated, and as no move; let go, nothing holds it.
fn a_focused_window_holds_the_pointer_as_it_asks() {
    use flui_platform::CursorGrab;
    use flui_platform::pointer::MotionAcceleration;

    park_pointer();
    let handler = RecordingWindowHandler::new();
    let mut fixture = X11Fixture::open_with("grab", Rc::clone(&handler));
    fixture
        .window
        .admit_and_apply(WindowCommand::Activate)
        .expect("the window activates");
    fixture.settle();
    assert!(
        fixture.window.state().focus.is_focused(),
        "the window has the focus"
    );
    Xtest::new(&fixture).move_to(100, 100);

    fixture
        .window
        .admit_and_apply(WindowCommand::SetCursorGrab(CursorGrab::Confined))
        .expect("the window confines the pointer");
    fixture.settle();
    assert_eq!(fixture.window.state().cursor_grab, CursorGrab::Confined);
    // The grab is the server's: no other client takes the pointer. (XTest's
    // motion is not held to a grab's confinement on Xvfb, so where the
    // pointer may go is not read here.)
    assert!(fixture.pointer_grabbed_elsewhere());

    fixture
        .window
        .admit_and_apply(WindowCommand::SetCursorGrab(CursorGrab::Locked))
        .expect("the window locks the pointer");
    fixture.settle();
    assert_eq!(fixture.window.state().cursor_grab, CursorGrab::Locked);
    let before = pointer_events(&handler).len();
    // XTest's relative motion: a device's counts, before acceleration.
    Xtest::new(&fixture).send(MOTION_NOTIFY_EVENT, 1, (7, -3));
    let locked = pointer_events(&handler).split_off(before);
    assert!(
        !locked
            .iter()
            .any(|event| matches!(event, PointerEvent::Move(_))),
        "a locked pointer moves by no move: {locked:#?}"
    );
    let raw: Vec<(f64, f64, MotionAcceleration)> = locked
        .iter()
        .filter_map(|event| match event {
            PointerEvent::RawMotion(motion) => {
                Some((motion.delta.x(), motion.delta.y(), motion.acceleration))
            }
            _ => None,
        })
        .collect();
    assert_eq!(raw, [(7.0, -3.0, MotionAcceleration::None)], "{locked:#?}");

    fixture
        .window
        .admit_and_apply(WindowCommand::SetCursorGrab(CursorGrab::None))
        .expect("the window lets the pointer go");
    fixture.settle();
    assert_eq!(fixture.window.state().cursor_grab, CursorGrab::None);
    assert!(!fixture.pointer_grabbed_elsewhere());
    Xtest::new(&fixture).move_to_root(1270, 1010);
}

/// A window keeps the level it asks for, as the window manager keeps it in
/// `_NET_WM_STATE`, and its snapshot reads it back.
fn a_window_keeps_the_level_it_asks_for() {
    use flui_platform::WindowLevel;

    let mut fixture = X11Fixture::open_with("level", RecordingWindowHandler::new());
    for level in [
        WindowLevel::AlwaysOnTop,
        WindowLevel::AlwaysOnBottom,
        WindowLevel::Normal,
    ] {
        fixture
            .window
            .admit_and_apply(WindowCommand::SetLevel(level))
            .unwrap_or_else(|error| panic!("{level:?}: {error}"));
        fixture.settle();
        assert_eq!(fixture.window.state().level, level);
    }
}

/// A focused fixture window, its pointer parked away from it first.
fn focused_fixture(title: &str) -> X11Fixture {
    park_pointer();
    let mut fixture = X11Fixture::open_with(title, RecordingWindowHandler::new());
    fixture
        .window
        .admit_and_apply(WindowCommand::Activate)
        .expect("the window activates");
    fixture.settle();
    assert!(
        fixture.window.state().focus.is_focused(),
        "the window has the focus"
    );
    fixture
}

/// A hold the server refuses leaves the window holding none, and its
/// snapshot says so: another client holds the pointer, the window's hold is
/// refused, and once that client lets go the window's next hold is taken.
fn a_refused_hold_leaves_the_window_holding_none() {
    use flui_platform::CursorGrab;
    use x11rb::protocol::xproto::{EventMask, GrabMode, GrabStatus};

    let mut fixture = focused_fixture("refused grab");
    let (other, _) =
        x11rb::rust_connection::RustConnection::connect(None).expect("the X server answers");
    let taken = other
        .grab_pointer(
            false,
            fixture.root(),
            EventMask::NO_EVENT,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
            x11rb::NONE,
            x11rb::NONE,
            x11rb::CURRENT_TIME,
        )
        .expect("request")
        .reply()
        .expect("the grab's answer")
        .status;
    assert_eq!(
        taken,
        GrabStatus::SUCCESS,
        "the other client holds the pointer"
    );
    let refused = fixture
        .window
        .admit_and_apply(WindowCommand::SetCursorGrab(CursorGrab::Confined))
        .expect_err("the server refuses a second client's grab");
    eprintln!("refused: {refused}");
    assert_eq!(fixture.window.state().cursor_grab, CursorGrab::None);
    fixture.settle();
    assert_eq!(fixture.window.state().cursor_grab, CursorGrab::None);

    other.ungrab_pointer(x11rb::CURRENT_TIME).expect("request");
    other
        .get_input_focus()
        .expect("request")
        .reply()
        .expect("a round trip");
    fixture
        .window
        .admit_and_apply(WindowCommand::SetCursorGrab(CursorGrab::Confined))
        .expect("the pointer is free again");
    fixture.settle();
    assert_eq!(fixture.window.state().cursor_grab, CursorGrab::Confined);
    assert!(fixture.pointer_grabbed_elsewhere());
    fixture
        .window
        .admit_and_apply(WindowCommand::SetCursorGrab(CursorGrab::None))
        .expect("the window lets the pointer go");
    fixture.settle();
}

/// A lock holds the pointer at a point of the window's content, not of the
/// screen: as the window shrinks the point stays inside the content, and as
/// the window moves the pointer goes with it.
fn a_locked_pointer_stays_with_the_content_as_the_window_moves() {
    use flui_foundation::geometry::{Point, Size};
    use flui_platform::CursorGrab;

    let mut fixture = focused_fixture("lock point");
    Xtest::new(&fixture).move_to(300, 250);
    fixture
        .window
        .admit_and_apply(WindowCommand::SetCursorGrab(CursorGrab::Locked))
        .expect("the window locks the pointer");
    fixture.settle();
    assert_eq!(fixture.window.state().cursor_grab, CursorGrab::Locked);
    let in_window = |fixture: &X11Fixture| {
        let at = fixture
            .server
            .query_pointer(fixture.xid)
            .expect("request")
            .reply()
            .expect("the pointer");
        (at.win_x, at.win_y)
    };
    assert_eq!(in_window(&fixture), (300, 250), "held where it was");

    fixture
        .window
        .admit_and_apply(WindowCommand::SetSize(Size::new(200.0, 150.0)))
        .expect("the window resizes");
    fixture.settle();
    assert_eq!(
        in_window(&fixture),
        (199, 149),
        "held at the nearest point inside the smaller content"
    );

    // Moved by less than its size, the window still holds the pointer's old
    // place on the screen, which the server leaves alone; the lock's point
    // is the content's, so the pointer goes with the window.
    let origin = fixture.window.state().outer.position().expect("placed");
    fixture
        .window
        .admit_and_apply(WindowCommand::SetPosition(Point::new(
            origin.x + 50.0,
            origin.y + 40.0,
        )))
        .expect("the window moves");
    fixture.settle();
    assert_eq!(
        fixture.window.state().outer.position(),
        Some(Point::new(origin.x + 50.0, origin.y + 40.0)),
        "the window manager moved the window"
    );
    assert_eq!(in_window(&fixture), (199, 149), "the pointer moved with it");
    fixture
        .window
        .admit_and_apply(WindowCommand::SetCursorGrab(CursorGrab::None))
        .expect("the window lets the pointer go");
    fixture.settle();
    Xtest::new(&fixture).move_to_root(1270, 1010);
}

/// A window kept below the others keeps its level through fullscreen: the
/// window manager stacks a focused fullscreen window above every other while
/// it lasts, the snapshot publishes the level throughout, and once the window
/// leaves fullscreen it is below the others again.
fn a_level_holds_through_fullscreen() {
    use flui_platform::{WindowLevel, WindowMode};

    let mut fixture = X11Fixture::open_with("level and fullscreen", RecordingWindowHandler::new());
    let other = fixture.open_other_window();
    let stacked_above_other = |fixture: &X11Fixture| {
        let stacking = crate::cases::atom(&fixture.server, "_NET_CLIENT_LIST_STACKING");
        let windows: Vec<u32> = fixture
            .server
            .get_property(
                false,
                fixture.root(),
                stacking,
                x11rb::protocol::xproto::AtomEnum::WINDOW,
                0,
                1024,
            )
            .expect("request")
            .reply()
            .expect("the property")
            .value32()
            .map_or_default(Iterator::collect);
        let place = |xid: u32| windows.iter().position(|window| *window == xid);
        place(fixture.xid).expect("the window is stacked")
            > place(other).expect("the other window is stacked")
    };
    fixture
        .window
        .admit_and_apply(WindowCommand::SetLevel(WindowLevel::AlwaysOnBottom))
        .expect("the window is kept below");
    fixture.settle();
    assert_eq!(fixture.window.state().level, WindowLevel::AlwaysOnBottom);
    assert!(!stacked_above_other(&fixture), "below the other window");

    for command in [
        WindowCommand::Activate,
        WindowCommand::SetMode(WindowMode::Fullscreen),
    ] {
        fixture
            .window
            .admit_and_apply(command)
            .expect("the window goes fullscreen with the focus");
        fixture.settle();
    }
    let state = fixture.window.state();
    assert_eq!(state.mode, WindowMode::Fullscreen);
    assert_eq!(
        state.level,
        WindowLevel::AlwaysOnBottom,
        "the level stays published"
    );
    assert!(
        stacked_above_other(&fixture),
        "a focused fullscreen window is above the others"
    );

    fixture
        .window
        .admit_and_apply(WindowCommand::SetMode(WindowMode::Normal))
        .expect("the window leaves fullscreen");
    fixture.settle();
    let state = fixture.window.state();
    assert_eq!(state.mode, WindowMode::Normal);
    assert_eq!(state.level, WindowLevel::AlwaysOnBottom);
    assert!(
        !stacked_above_other(&fixture),
        "below the other window again"
    );
}

/// A client of its own that is an XDND target: an override-redirect window
/// at `(x, y)` of the root window, `XdndAware` in version 5.
struct DropTarget {
    connection: x11rb::rust_connection::RustConnection,
    xid: u32,
}

impl DropTarget {
    fn open(x: i16, y: i16) -> Self {
        use x11rb::protocol::xproto::{CreateWindowAux, PropMode, WindowClass};
        use x11rb::wrapper::ConnectionExt as _;
        let (connection, screen) =
            x11rb::rust_connection::RustConnection::connect(None).expect("the X server answers");
        let root = connection.setup().roots[screen].root;
        let xid = connection.generate_id().expect("an X id");
        connection
            .create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                xid,
                root,
                x,
                y,
                200,
                200,
                0,
                WindowClass::INPUT_OUTPUT,
                x11rb::COPY_FROM_PARENT,
                &CreateWindowAux::new().override_redirect(1),
            )
            .expect("request");
        let aware = crate::cases::atom(&connection, "XdndAware");
        connection
            .change_property32(
                PropMode::REPLACE,
                xid,
                aware,
                x11rb::protocol::xproto::AtomEnum::ATOM,
                &[5],
            )
            .expect("request");
        connection.map_window(xid).expect("request");
        connection
            .get_input_focus()
            .expect("request")
            .reply()
            .expect("the window is mapped");
        Self { connection, xid }
    }

    fn atom(&self, name: &str) -> u32 {
        crate::cases::atom(&self.connection, name)
    }

    /// The next client message of `kind` the target hears, turning the
    /// host's loop meanwhile.
    fn expect(&self, fixture: &X11Fixture, kind: &str) -> [u32; 5] {
        let kind = self.atom(kind);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            run_for(&fixture.platform, Duration::from_millis(10)).expect("the loop turns");
            while let Some(event) = self.connection.poll_for_event().expect("the connection") {
                if let x11rb::protocol::Event::ClientMessage(message) = event
                    && message.type_ == kind
                {
                    return message.data.as_data32();
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the target heard no {kind}"
            );
        }
    }

    fn send(&self, to: u32, kind: &str, data: [u32; 5]) {
        let event = x11rb::protocol::xproto::ClientMessageEvent::new(32, to, self.atom(kind), data);
        self.connection
            .send_event(
                false,
                to,
                x11rb::protocol::xproto::EventMask::NO_EVENT,
                event,
            )
            .expect("request");
        self.connection.flush().expect("flush");
    }

    /// Convert `XdndSelection` to `UTF8_STRING` into the target's window and
    /// read what the source wrote, turning the host's loop meanwhile.
    fn read_text(&self, fixture: &X11Fixture, time: u32) -> Option<String> {
        let selection = self.atom("XdndSelection");
        let utf8 = self.atom("UTF8_STRING");
        let property = self.atom("FLUI_TEST_DROP");
        self.connection
            .convert_selection(self.xid, selection, utf8, property, time)
            .expect("request");
        self.connection.flush().expect("flush");
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            run_for(&fixture.platform, Duration::from_millis(10)).expect("the loop turns");
            while let Some(event) = self.connection.poll_for_event().expect("the connection") {
                if let x11rb::protocol::Event::SelectionNotify(notify) = event {
                    if notify.property == x11rb::NONE {
                        return None;
                    }
                    let reply = self
                        .connection
                        .get_property(true, self.xid, property, utf8, 0, u32::MAX / 4)
                        .expect("request")
                        .reply()
                        .expect("the property");
                    return Some(String::from_utf8(reply.value).expect("UTF-8"));
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the source never answered the conversion"
            );
        }
    }
}

/// A drag the window starts while XTest holds its button runs on the
/// pointer the server moves: the target another client shows is entered and
/// told the position, the release drops there, the target reads the text
/// and finishes, and the session ends with the target's action. The press
/// the session took is ended for the window, and its next click reaches it
/// whole.
fn an_xtest_drag_from_the_window_drops_on_another_client() {
    use std::cell::RefCell;
    use std::num::NonZeroU64;

    use flui_foundation::{DragSessionId, IdMint, UiRuntimeId};
    use flui_platform::data_transfer::{
        RepresentationDescriptor, TransferActions, TransferFormat, TransferPayload,
    };
    use flui_platform::drag::{
        DragSessionKey, OsDragEnd, OsDragRequest, OsDragTerms, ProcessNonce,
    };

    park_pointer();
    let handler = RecordingWindowHandler::new();
    let fixture = X11Fixture::open_with("xtest drag source", Rc::clone(&handler));
    let target = DropTarget::open(900, 700);
    let xtest = Xtest::new(&fixture);
    xtest.move_to(50, 40);
    xtest.send(BUTTON_PRESS_EVENT, 1, (0, 0));

    let ended = Rc::new(RefCell::new(None));
    let end = Rc::clone(&ended);
    fixture
        .window
        .drag_source()
        .expect("the window has a drag source")
        .start(OsDragRequest::new(
            OsDragTerms::try_new(
                DragSessionKey::new(
                    ProcessNonce::from_raw(NonZeroU64::MIN),
                    UiRuntimeId::new(1),
                    IdMint::<DragSessionId>::new().next().expect("fresh mint"),
                ),
                vec![RepresentationDescriptor {
                    format: TransferFormat::Text,
                    declared_len: None,
                }],
                TransferActions::COPY,
            )
            .expect("one text representation under COPY"),
            None,
            Box::new(|_| Ok(TransferPayload::Text("dragged".to_owned()))),
            Box::new(move |outcome| *end.borrow_mut() = Some(outcome)),
        ))
        .expect("the session starts");

    xtest.move_to_root(950, 750);
    let enter = target.expect(&fixture, "XdndEnter");
    let source = enter[0];
    assert_eq!(enter[1] >> 24, 5, "the enter names version 5");
    let position = target.expect(&fixture, "XdndPosition");
    assert_eq!(position[0], source);
    assert_eq!(
        (position[2] >> 16, position[2] & 0xffff),
        (950, 750),
        "the server's pointer"
    );
    let copy = target.atom("XdndActionCopy");
    target.send(source, "XdndStatus", [target.xid, 1, 0, 0, copy]);

    xtest.send(BUTTON_RELEASE_EVENT, 1, (0, 0));
    let dropped = target.expect(&fixture, "XdndDrop");
    assert_eq!(dropped[0], source);
    assert_eq!(
        target.read_text(&fixture, dropped[2]).as_deref(),
        Some("dragged")
    );
    target.send(source, "XdndFinished", [target.xid, 1, copy, 0, 0]);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while ended.borrow().is_none() {
        assert!(
            std::time::Instant::now() < deadline,
            "the session never ended"
        );
        run_for(&fixture.platform, Duration::from_millis(10)).expect("the loop turns");
    }
    assert_eq!(
        *ended.borrow(),
        Some(OsDragEnd::Dropped(TransferActions::COPY))
    );

    let taken = pointer_events(&handler);
    assert!(
        taken
            .iter()
            .any(|event| matches!(event, PointerEvent::Cancel(_))),
        "the press the session took ends for the window: {taken:?}"
    );
    assert!(
        !taken
            .iter()
            .any(|event| matches!(event, PointerEvent::Up(_))),
        "the release was the session's: {taken:?}"
    );
    let before = taken.len();
    xtest.move_to(60, 40);
    xtest.click(1);
    let lines: Vec<String> = pointer_events(&handler)[before..]
        .iter()
        .filter_map(line)
        .collect();
    assert!(
        lines.ends_with(&["down PRIMARY".to_owned(), "up PRIMARY".to_owned()]),
        "the next click reaches the window: {lines:?}"
    );
}

/// Escape cancels a drag the window started after the focus went to
/// another window with the button still held, as Alt+Tab or a window that
/// raises itself moves it: the session holds the keyboard, so the focus
/// moving does not end it, and Escape still reaches it.
fn an_xtest_escape_cancels_a_drag_after_the_focus_moved() {
    use std::cell::RefCell;
    use std::num::NonZeroU64;

    use flui_foundation::{DragSessionId, IdMint, UiRuntimeId};
    use flui_platform::data_transfer::{
        RepresentationDescriptor, TransferActions, TransferFormat, TransferPayload,
    };
    use flui_platform::drag::{
        DragSessionKey, OsDragEnd, OsDragRequest, OsDragTerms, ProcessNonce,
    };

    park_pointer();
    let handler = RecordingWindowHandler::new();
    let mut fixture = X11Fixture::open_with("xtest drag and escape", Rc::clone(&handler));
    fixture
        .window
        .admit_and_apply(WindowCommand::Activate)
        .expect("the window activates");
    fixture.settle();
    let xtest = Xtest::new(&fixture);
    xtest.move_to(50, 40);
    xtest.send(BUTTON_PRESS_EVENT, 1, (0, 0));

    let ended = Rc::new(RefCell::new(None));
    let end = Rc::clone(&ended);
    fixture
        .window
        .drag_source()
        .expect("the window has a drag source")
        .start(OsDragRequest::new(
            OsDragTerms::try_new(
                DragSessionKey::new(
                    ProcessNonce::from_raw(NonZeroU64::MIN),
                    UiRuntimeId::new(1),
                    IdMint::<DragSessionId>::new().next().expect("fresh mint"),
                ),
                vec![RepresentationDescriptor {
                    format: TransferFormat::Text,
                    declared_len: None,
                }],
                TransferActions::COPY,
            )
            .expect("one text representation under COPY"),
            None,
            Box::new(|_| Ok(TransferPayload::Text("dragged".to_owned()))),
            Box::new(move |outcome| *end.borrow_mut() = Some(outcome)),
        ))
        .expect("the session starts");

    focus_elsewhere(&fixture);
    run_for(&fixture.platform, Duration::from_millis(100)).expect("the loop turns");
    assert_eq!(
        *ended.borrow(),
        None,
        "the focus moving leaves a session that holds the keyboard"
    );
    xtest.key(keycode::ESCAPE, true);
    xtest.key(keycode::ESCAPE, false);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while ended.borrow().is_none() {
        assert!(
            std::time::Instant::now() < deadline,
            "Escape never reached the session"
        );
        run_for(&fixture.platform, Duration::from_millis(10)).expect("the loop turns");
    }
    assert_eq!(*ended.borrow(), Some(OsDragEnd::Cancelled));
    xtest.send(BUTTON_RELEASE_EVENT, 1, (0, 0));
}

/// A window whose application draws its title bar opens without the window
/// manager's decorations. A drag on its bar moves it through the window
/// manager, a drag on its right edge widens it, a double-click on the bar
/// maximizes it, and none of those presses reaches the handler, while a click
/// on its content does (ADR-0286 §10).
fn a_title_bar_of_the_applications_moves_and_resizes_through_the_window_manager() {
    use flui_foundation::geometry::{Rect, Size};
    use flui_platform::input_regions::{InputRegion, InputRegionKind};
    use flui_platform::{AppTitleBar, WindowFrame, WindowMode, WindowOptions};

    park_pointer();
    let handler = RecordingWindowHandler::new();
    let mut fixture = X11Fixture::open_with_options(
        WindowOptions::default()
            .title("app title bar")
            .size(Size::new(400.0, 300.0))
            .frame(WindowFrame::AppTitleBar(AppTitleBar::new())),
        Rc::clone(&handler) as Rc<dyn flui_platform::WindowHandler>,
    );
    assert!(
        matches!(fixture.window.state().frame, WindowFrame::AppTitleBar(_)),
        "openbox moves a window at its request: {:?}",
        fixture.window.state().frame
    );
    let bar = InputRegion::new(
        InputRegionKind::WindowMove,
        Rect::from_ltrb(0.0, 0.0, 400.0, 32.0),
    );
    fixture
        .window
        .set_input_regions(&[bar])
        .expect("the X11 host takes chrome regions");
    fixture.settle();

    // A drag on the bar: the move starts once the pointer has travelled.
    let before = fixture.window.state().outer;
    let xtest = Xtest::new(&fixture);
    xtest.move_to(200, 16);
    xtest.send(BUTTON_PRESS_EVENT, 1, (0, 0));
    for step in 1..=6 {
        xtest.move_to(200 + step * 10, 16 + step * 5);
    }
    xtest.send(BUTTON_RELEASE_EVENT, 1, (0, 0));
    fixture.settle();
    let moved = fixture.window.state().outer;
    let (from, to) = (
        before.position().expect("a position"),
        moved.position().expect("a position"),
    );
    assert!(
        to.x > from.x + 30.0 && to.y > from.y + 15.0,
        "the bar moved the window: {from:?} to {to:?}"
    );
    assert_eq!(moved.size(), before.size());

    // A drag on the right edge, inside the band along it.
    let xtest = Xtest::new(&fixture);
    xtest.move_to(398, 150);
    xtest.send(BUTTON_PRESS_EVENT, 1, (0, 0));
    for step in 1..=5 {
        xtest.move_to(398 + step * 10, 150);
    }
    xtest.send(BUTTON_RELEASE_EVENT, 1, (0, 0));
    fixture.settle();
    let widened = fixture.window.state().content_size.get();
    assert!(
        widened.width > 430.0 && widened.height == 300.0,
        "the edge widened the window: {widened:?}"
    );

    // A double-click on the bar.
    let xtest = Xtest::new(&fixture);
    xtest.move_to(200, 16);
    xtest.click(1);
    xtest.click(1);
    fixture.settle();
    assert_eq!(fixture.window.state().mode, WindowMode::Maximized);

    let presses = |handler: &RecordingWindowHandler| {
        pointer_events(handler)
            .iter()
            .filter_map(line)
            .filter(|line| line.starts_with("down") || line.starts_with("up"))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        presses(&handler),
        Vec::<String>::new(),
        "the chrome took them"
    );

    // A click on the content is the application's.
    let xtest = Xtest::new(&fixture);
    xtest.move_to(200, 200);
    xtest.click(1);
    assert_eq!(presses(&handler), ["down PRIMARY", "up PRIMARY"]);
    xtest.move_to_root(1270, 1010);
}

/// A real window manager paces an interactive resize by the window's counter:
/// a drag on the right edge of a window with a title bar of its own hands the
/// resize to openbox, which resets the counter, asks for a synchronised resize
/// before each step and holds the next step until the counter has the step's
/// value. The window answers each one, so the counter ends past its reset and
/// the drag widens the window as far as the pointer went.
fn openbox_paces_an_interactive_resize_by_the_windows_counter() {
    use flui_foundation::geometry::{Rect, Size};
    use flui_platform::input_regions::{InputRegion, InputRegionKind};
    use flui_platform::{AppTitleBar, WindowFrame, WindowOptions};

    use crate::cases::{counter_reader, counter_value, resize_counters};

    park_pointer();
    let mut fixture = X11Fixture::open_with_options(
        WindowOptions::default()
            .title("paced resize")
            .size(Size::new(400.0, 300.0))
            .frame(WindowFrame::AppTitleBar(AppTitleBar::new())),
        RecordingWindowHandler::new() as Rc<dyn flui_platform::WindowHandler>,
    );
    let bar = InputRegion::new(
        InputRegionKind::WindowMove,
        Rect::from_ltrb(0.0, 0.0, 400.0, 32.0),
    );
    fixture
        .window
        .set_input_regions(&[bar])
        .expect("the X11 host takes chrome regions");
    fixture.settle();
    let [basic, _] = resize_counters(&fixture.server, fixture.xid);
    let reader = counter_reader();

    let xtest = Xtest::new(&fixture);
    xtest.move_to(398, 150);
    xtest.send(BUTTON_PRESS_EVENT, 1, (0, 0));
    for step in 1..=5 {
        xtest.move_to(398 + step * 10, 150);
    }
    xtest.send(BUTTON_RELEASE_EVENT, 1, (0, 0));
    fixture.settle();
    assert!(
        counter_value(&reader, basic) >= 1,
        "openbox's requests were answered"
    );
    let widened = fixture.window.state().content_size.get();
    assert!(
        widened.width > 430.0 && widened.height == 300.0,
        "the edge widened the window: {widened:?}"
    );
    Xtest::new(&fixture).move_to_root(1270, 1010);
}
