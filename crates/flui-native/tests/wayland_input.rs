//! The native Wayland host's input from a real compositor seat: a Weston on
//! an X server of the harness's own, whose X input the harness drives
//! through XTest, so the window hears what a compositor's `wl_pointer` and
//! `wl_keyboard` deliver, translated by Weston from an X server as it would
//! be from libinput.
//!
//! Its own harness: it starts Xvfb and a Weston on Weston's X11 backend with
//! the kiosk shell (every window fullscreen on the one output, so a position
//! on the output is one on the window) before anything connects, and names
//! that Weston for the process. The X server is reached by its name, never
//! through `DISPLAY`, so the Wayland host is the only one the process sees.
//! A run without Xvfb or Weston is skipped, and fails where
//! `FLUI_REQUIRE_WAYLAND` is set.
//!
//! With `native-capture` (debug builds), the session's registry journal is
//! replayed and must equal what the window heard; `FLUI_NATIVE_RECORDING_BLESS`
//! writes it over the checked-in recording that `platform_it` replays.

fn main() {
    #[cfg(target_os = "linux")]
    cases::main();
}

#[cfg(target_os = "linux")]
#[path = "support/weston_x11.rs"]
mod weston_x11;

#[cfg(target_os = "linux")]
mod cases {
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use cursor_icon::CursorIcon;
    use flui_native::testing::wayland::{cursor_report, last_input_serial, present, run_for};
    use flui_native::{HostWindow, Platform as _, WaylandPlatform};
    use flui_platform::keyboard::{Code, Key, KeyState};
    use flui_platform::pointer::{PointerButton, PointerEvent, PointerKind, ScrollUnit};
    use flui_platform::{Availability, PlatformInput, WindowOptions};
    use flui_testing::window_commands::AdmitAndApply as _;
    use flui_testing::window_handler::{HandlerCall, RecordingWindowHandler};
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::{
        BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT, KEY_PRESS_EVENT, KEY_RELEASE_EVENT,
        MOTION_NOTIFY_EVENT,
    };
    use x11rb::protocol::xtest::ConnectionExt as _;

    use super::weston_x11::Session;

    /// The output's size, which the kiosk shell gives every window.
    const OUTPUT: (u16, u16) = (640, 480);
    /// The evdev keymap's `a`, as X numbers it (eight above the kernel's).
    const X_KEY_A: u8 = 38;

    pub(super) fn main() {
        // What the host reports of the compositor (its keymap, among them)
        // reaches the test's output.
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .with_writer(std::io::stderr)
            .try_init();
        let arguments = libtest_mimic::Arguments::from_args();
        let session = (!arguments.list)
            .then(|| Session::start("kiosk-shell.so", OUTPUT, "flui-wayland-input"))
            .flatten();
        let trials = vec![libtest_mimic::Trial::test(
            "a_compositor_seat_reaches_the_window",
            move || {
                match &session {
                    Some(session) => a_compositor_seat_reaches_the_window(session),
                    None => eprintln!("skipped: no Xvfb or Weston to run against"),
                }
                Ok(())
            },
        )];
        libtest_mimic::run(&arguments, trials).exit();
    }

    /// Turn the loop until `done` holds over what the handler heard,
    /// drawing each frame the window asks for, bounded at 10 s.
    fn turn_until(
        platform: &WaylandPlatform,
        window: &Rc<dyn HostWindow>,
        handler: &RecordingWindowHandler,
        what: &str,
        done: impl Fn(&[HandlerCall]) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            run_for(platform, Duration::from_millis(20)).expect("the loop turns");
            window
                .surface_target()
                .pre_present_notify(flui_platform::SwapChainStatistics::None);
            let _ = present(window.as_ref());
            if done(&handler.calls()) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {what}; heard {:?}",
                handler.inputs()
            );
        }
    }

    /// The pointer events the handler heard.
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

    /// Weston's seat reaches the window: its capability snapshot first, then
    /// the cursor's enter, move, press and release, a wheel notch as one
    /// step in detents, and a key under the compositor's keymap.
    fn a_compositor_seat_reaches_the_window(session: &Session) {
        #[cfg(all(debug_assertions, feature = "native-capture"))]
        let recorder = flui_native::journal::recorder::Recorder::start(
            flui_native::journal::codec::Backend::Wayland,
        );
        let platform = WaylandPlatform::new().expect("the harness's Weston answers");
        let handler = RecordingWindowHandler::new();
        let window = platform
            .open_window(
                WindowOptions::default().with_title("input"),
                Rc::clone(&handler) as _,
            )
            .expect("a Wayland window opens");
        turn_until(
            &platform,
            &window,
            &handler,
            "the window on the output",
            |_| window.state().visible && window.state().focused,
        );

        let Some(PlatformInput::Capabilities(capabilities)) = handler.inputs().into_iter().next()
        else {
            panic!(
                "the snapshot is the handler's first input: {:?}",
                handler.inputs()
            );
        };
        assert_eq!(capabilities.precise_scroll(), Availability::Native);

        let screen = session.screen();
        let (x, root, origin) = (&screen.connection, screen.root, screen.origin);
        let fake = |kind: u8, detail: u8, at: Option<(i16, i16)>| {
            let (px, py) = at.unwrap_or((0, 0));
            x.xtest_fake_input(kind, detail, x11rb::CURRENT_TIME, root, px, py, 0)
                .expect("request");
            x.flush().expect("flush");
        };
        let at = |dx: i16, dy: i16| Some((origin.0 + dx, origin.1 + dy));
        fake(MOTION_NOTIFY_EVENT, 0, at(100, 120));
        turn_until(&platform, &window, &handler, "the cursor's move", |_| {
            pointer_events(&handler)
                .iter()
                .any(|event| matches!(event, PointerEvent::Move(_)))
        });
        // The keyboard entering the window is the first input an activation
        // token may name; each click and key names a later serial.
        let focused = last_input_serial(&platform).expect("the keyboard entered the window");
        fake(BUTTON_PRESS_EVENT, 1, None);
        fake(BUTTON_RELEASE_EVENT, 1, None);
        turn_until(&platform, &window, &handler, "the click", |_| {
            pointer_events(&handler)
                .iter()
                .any(|event| matches!(event, PointerEvent::Up(_)))
        });
        let clicked = last_input_serial(&platform).expect("the click is input");
        assert!(
            clicked > focused,
            "the click's serial {clicked} after {focused}"
        );
        // Button 4 is the wheel turned away from the user: toward earlier
        // content.
        fake(BUTTON_PRESS_EVENT, 4, None);
        fake(BUTTON_RELEASE_EVENT, 4, None);
        turn_until(&platform, &window, &handler, "the wheel", |_| {
            pointer_events(&handler)
                .iter()
                .any(|event| matches!(event, PointerEvent::Scroll(_)))
        });
        // The capture journals the seats' registry as the session goes.
        #[cfg(all(debug_assertions, feature = "native-capture"))]
        assert!(
            recorder
                .journals()
                .snapshot()
                .first()
                .is_some_and(|recording| !recording.rows.is_empty()),
            "the seats' registry is journalled"
        );
        fake(KEY_PRESS_EVENT, X_KEY_A, None);
        fake(KEY_RELEASE_EVENT, X_KEY_A, None);
        turn_until(&platform, &window, &handler, "the key", |calls| {
            calls.iter().any(|call| {
                matches!(call, HandlerCall::Input(PlatformInput::Keyboard(key)) if key.state() == KeyState::Up)
            })
        });
        let keyed = last_input_serial(&platform).expect("the key is input");
        assert!(
            keyed > clicked,
            "the key's serial {keyed} after the click's {clicked}"
        );

        let pointer = pointer_events(&handler);
        let enter = pointer
            .iter()
            .position(|event| matches!(event, PointerEvent::Enter(signal) if signal.pointer.kind == PointerKind::Mouse))
            .expect("the cursor entered");
        let down = pointer
            .iter()
            .find_map(|event| match event {
                PointerEvent::Down(press) => Some(press),
                _ => None,
            })
            .expect("a press");
        assert_eq!(down.button(), PointerButton::PRIMARY);
        let position = down.sample.position.get();
        assert_eq!(
            (position.x, position.y),
            (100.0, 120.0),
            "where the cursor was put"
        );
        assert!(
            pointer
                .iter()
                .position(|event| matches!(event, PointerEvent::Down(_)))
                > Some(enter),
            "the press follows the enter"
        );
        assert!(pointer.iter().any(
            |event| matches!(event, PointerEvent::Up(up) if up.button() == PointerButton::PRIMARY)
        ));
        let scroll = pointer
            .iter()
            .find_map(|event| match event {
                PointerEvent::Scroll(scroll) => Some(scroll),
                _ => None,
            })
            .expect("a scroll");
        assert_eq!(scroll.delta.unit(), ScrollUnit::Detents);
        assert_eq!(
            (scroll.delta.x(), scroll.delta.y()),
            (0.0, -1.0),
            "one notch toward earlier content"
        );
        assert_eq!(scroll.phase, None, "a wheel has no phases");

        let keys: Vec<_> = handler
            .inputs()
            .into_iter()
            .filter_map(|input| match input {
                PlatformInput::Keyboard(key) => Some(key),
                _ => None,
            })
            .collect();
        assert_eq!(
            keys.iter()
                .map(|key| (key.state(), key.key.clone(), key.code))
                .collect::<Vec<_>>(),
            [
                (KeyState::Down, Key::character("a"), Code::KeyA),
                (KeyState::Up, Key::character("a"), Code::KeyA),
            ]
        );
        // A keymap may name no layout (Weston's X11 backend on CI's Xvfb
        // sends none): the key above is what the keymap is checked by.
        eprintln!("keyboard layout: {:?}", platform.keyboard_layout());
        // The cursor shows over the window: by `cursor-shape` where the
        // compositor has it, from the cursor theme where it has not (Weston
        // 13).
        window
            .set_cursor(CursorIcon::Pointer)
            .expect("the window shows a cursor");
        run_for(&platform, Duration::from_millis(20)).expect("the loop turns");
        let shown = cursor_report(&platform).expect("a cursor was shown");
        assert!(
            shown.name == "4" || shown.scale >= 1,
            "the pointer cursor, by shape (4) or from the theme: {shown:?}"
        );
        eprintln!("cursor: {shown:?}");

        #[cfg(all(debug_assertions, feature = "native-capture"))]
        hold_recording(recorder, &pointer);
        window.close_for_cleanup();
        run_for(&platform, Duration::from_millis(50)).expect("the loop turns");
    }

    /// The registry's journal replays to exactly the pointer input the
    /// window heard, and is written over the checked-in recording when
    /// blessed.
    #[cfg(all(debug_assertions, feature = "native-capture"))]
    fn hold_recording(recorder: flui_native::journal::recorder::Recorder, heard: &[PointerEvent]) {
        let recordings = recorder.finish();
        let [recording] = recordings.as_slice() else {
            panic!("one registry, the seats': {}", recordings.len());
        };
        let pointers = |inputs: Vec<PlatformInput>| -> Vec<PointerEvent> {
            inputs
                .into_iter()
                .filter_map(|input| match input {
                    PlatformInput::Pointer(event) => Some(event),
                    _ => None,
                })
                .collect()
        };
        let replayed = pointers(recording.replay().expect("a registry's recording replays"));
        assert_eq!(
            replayed.as_slice(),
            heard,
            "the journal replays to what the window heard"
        );
        let mut out = Vec::new();
        recording.write(&mut out).expect("a recording writes");
        let read = flui_native::journal::codec::NativeRecording::read(out.as_slice())
            .expect("it reads back");
        assert_eq!(
            pointers(read.replay().expect("a registry's recording replays")),
            replayed,
            "the written recording replays alike"
        );
        if std::env::var_os("FLUI_NATIVE_RECORDING_BLESS").is_some() {
            let path = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/native_recording/wayland_weston_mouse_wheel.jsonl"
            );
            std::fs::write(path, out).expect("the recording is written");
        }
    }
}
