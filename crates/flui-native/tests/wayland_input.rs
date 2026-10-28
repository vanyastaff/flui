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

#[cfg(all(target_os = "linux", debug_assertions, feature = "native-capture"))]
#[path = "native_recording/codec.rs"]
mod codec;
#[cfg(all(target_os = "linux", debug_assertions, feature = "native-capture"))]
mod native_recording {
    pub(crate) use super::codec;
    #[path = "../native_recording/journal.rs"]
    pub(crate) mod journal;
}

#[cfg(target_os = "linux")]
mod cases {
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use cursor_icon::CursorIcon;
    use flui_native::testing::wayland::{present, run_for};
    use flui_native::{HostWindow, Platform as _, WaylandPlatform};
    use flui_platform::keyboard::{Code, Key, KeyState};
    use flui_platform::pointer::{PointerButton, PointerEvent, PointerKind, ScrollUnit};
    use flui_platform::{Availability, PlatformInput, WindowOptions};
    use flui_testing::window_commands::AdmitAndApply as _;
    use flui_testing::window_handler::{HandlerCall, RecordingWindowHandler};
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::{
        BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT, ConnectionExt as _, InputFocus,
        KEY_PRESS_EVENT, KEY_RELEASE_EVENT, MOTION_NOTIFY_EVENT, MapState,
    };
    use x11rb::protocol::xtest::ConnectionExt as _;
    use x11rb::rust_connection::RustConnection;

    /// The output's size, which the kiosk shell gives every window.
    const OUTPUT: (u16, u16) = (640, 480);
    /// The evdev keymap's `a`, as X numbers it (eight above the kernel's).
    const X_KEY_A: u8 = 38;

    pub(super) fn main() {
        let arguments = libtest_mimic::Arguments::from_args();
        let session = (!arguments.list).then(Session::start).flatten();
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

    /// Xvfb, and a Weston on it whose socket `WAYLAND_DISPLAY` names.
    struct Session {
        server: Child,
        weston: Child,
        display: String,
        _runtime: TempDir,
    }

    /// A directory removed with its contents when dropped.
    struct TempDir(PathBuf);

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    impl Drop for Session {
        fn drop(&mut self) {
            for child in [&mut self.weston, &mut self.server] {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    fn available(program: &str) -> bool {
        Command::new(program)
            .arg("--help")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
    }

    impl Session {
        fn start() -> Option<Self> {
            if !available("Xvfb") || !available("weston") {
                assert!(
                    std::env::var_os("FLUI_REQUIRE_WAYLAND").is_none(),
                    "FLUI_REQUIRE_WAYLAND is set and Xvfb or Weston is missing"
                );
                return None;
            }
            let (server, display) = x_server();
            let runtime = std::env::temp_dir().join(format!("flui-wayland-input-{}", std::process::id()));
            std::fs::create_dir_all(&runtime).expect("a runtime directory");
            std::fs::set_permissions(
                &runtime,
                <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o700),
            )
            .expect("the runtime directory is private");
            let socket = "flui-wayland-input";
            let weston = Command::new("weston")
                .args([
                    "--backend=x11",
                    "--renderer=pixman",
                    "--shell=kiosk-shell.so",
                    &format!("--socket={socket}"),
                    &format!("--width={}", OUTPUT.0),
                    &format!("--height={}", OUTPUT.1),
                    "--idle-time=0",
                ])
                .env("DISPLAY", &display)
                .env("XDG_RUNTIME_DIR", &runtime)
                .env_remove("WAYLAND_DISPLAY")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("Weston starts");
            until("Weston's socket", || runtime.join(socket).exists());
            // SAFETY: no thread has started; nothing reads the environment
            // while it is set.
            #[expect(unsafe_code, reason = "the environment is set before any thread exists")]
            unsafe {
                std::env::set_var("XDG_RUNTIME_DIR", &runtime);
                std::env::set_var("WAYLAND_DISPLAY", socket);
                std::env::remove_var("DISPLAY");
            }
            Some(Self {
                server,
                weston,
                display,
                _runtime: TempDir(runtime),
            })
        }

        /// The X connection the harness drives the input with, and Weston's
        /// output window on it.
        fn x(&self) -> (RustConnection, u32, (i16, i16)) {
            let (connection, screen) =
                RustConnection::connect(Some(&self.display)).expect("the X server answers");
            let root = connection.setup().roots[screen].root;
            let mut found = None;
            until("Weston's output window", || {
                let tree = connection.query_tree(root).expect("request").reply().expect("reply");
                found = tree.children.iter().copied().find(|window| {
                    let mapped = connection
                        .get_window_attributes(*window)
                        .ok()
                        .and_then(|cookie| cookie.reply().ok())
                        .is_some_and(|attributes| attributes.map_state == MapState::VIEWABLE);
                    let geometry = connection
                        .get_geometry(*window)
                        .ok()
                        .and_then(|cookie| cookie.reply().ok());
                    mapped && geometry.is_some_and(|geometry| (geometry.width, geometry.height) == OUTPUT)
                });
                found.is_some()
            });
            let window = found.expect("found above");
            let origin = connection
                .translate_coordinates(window, root, 0, 0)
                .expect("request")
                .reply()
                .expect("reply");
            // Weston's X11 backend gives its seat's keyboard to a client
            // while its window has the X focus.
            connection
                .set_input_focus(InputFocus::PARENT, window, x11rb::CURRENT_TIME)
                .expect("request");
            connection.flush().expect("flush");
            (connection, root, (origin.dst_x, origin.dst_y))
        }
    }

    /// Start Xvfb on a display number no other server holds (see
    /// `x11_window.rs`).
    fn x_server() -> (Child, String) {
        let first = 100 + std::process::id() % 800;
        for number in (first..first + 200).map(|number| 100 + number % 900) {
            let display = format!(":{number}");
            let mut server = Command::new("Xvfb")
                .args([&display, "-screen", "0", "1024x768x24", "-nolisten", "tcp"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("Xvfb starts");
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if server.try_wait().ok().flatten().is_some() {
                    break;
                }
                if RustConnection::connect(Some(&display)).is_ok() {
                    std::thread::sleep(Duration::from_millis(200));
                    if server.try_wait().ok().flatten().is_none() {
                        return (server, display);
                    }
                    break;
                }
                assert!(Instant::now() < deadline, "timed out waiting for Xvfb");
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        panic!("no free X display number");
    }

    /// Wait, bounded at 10 s, until `done` holds.
    fn until(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
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
            window.surface_target().pre_present_notify();
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
        let recorder = super::native_recording::journal::Recorder::start(
            super::codec::Backend::Wayland,
        );
        let platform = WaylandPlatform::new().expect("the harness's Weston answers");
        let handler = RecordingWindowHandler::new();
        let window = platform
            .open_window(WindowOptions::default().with_title("input"), Rc::clone(&handler) as _)
            .expect("a Wayland window opens");
        turn_until(&platform, &window, &handler, "the window on the output", |_| {
            window.state().visible && window.state().focused
        });

        let Some(PlatformInput::Capabilities(capabilities)) = handler.inputs().into_iter().next()
        else {
            panic!("the snapshot is the handler's first input: {:?}", handler.inputs());
        };
        assert_eq!(capabilities.precise_scroll(), Availability::Native);

        let (x, root, origin) = session.x();
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
        fake(BUTTON_PRESS_EVENT, 1, None);
        fake(BUTTON_RELEASE_EVENT, 1, None);
        // Button 4 is the wheel turned away from the user: toward earlier
        // content.
        fake(BUTTON_PRESS_EVENT, 4, None);
        fake(BUTTON_RELEASE_EVENT, 4, None);
        turn_until(&platform, &window, &handler, "the wheel", |_| {
            pointer_events(&handler)
                .iter()
                .any(|event| matches!(event, PointerEvent::Scroll(_)))
        });
        fake(KEY_PRESS_EVENT, X_KEY_A, None);
        fake(KEY_RELEASE_EVENT, X_KEY_A, None);
        turn_until(&platform, &window, &handler, "the key", |calls| {
            calls.iter().any(|call| {
                matches!(call, HandlerCall::Input(PlatformInput::Keyboard(key)) if key.state() == KeyState::Up)
            })
        });

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
        assert_eq!((position.x, position.y), (100.0, 120.0), "where the cursor was put");
        assert!(
            pointer.iter().position(|event| matches!(event, PointerEvent::Down(_))) > Some(enter),
            "the press follows the enter"
        );
        assert!(pointer.iter().any(|event| matches!(event, PointerEvent::Up(up) if up.button() == PointerButton::PRIMARY)));
        let scroll = pointer
            .iter()
            .find_map(|event| match event {
                PointerEvent::Scroll(scroll) => Some(scroll),
                _ => None,
            })
            .expect("a scroll");
        assert_eq!(scroll.delta.unit(), ScrollUnit::Detents);
        assert_eq!((scroll.delta.x(), scroll.delta.y()), (0.0, -1.0), "one notch toward earlier content");
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
            keys.iter().map(|key| (key.state(), key.key.clone(), key.code)).collect::<Vec<_>>(),
            [
                (KeyState::Down, Key::character("a"), Code::KeyA),
                (KeyState::Up, Key::character("a"), Code::KeyA),
            ]
        );
        assert!(!platform.keyboard_layout().is_empty(), "the keymap names its layout");
        // Weston 13 has no `cursor-shape`: the host says so rather than
        // pretend.
        if let Err(error) = window.set_cursor(CursorIcon::Pointer) {
            assert!(error.unsupported().is_some(), "{error}");
        }

        #[cfg(all(debug_assertions, feature = "native-capture"))]
        hold_recording(recorder, &pointer);
        window.close_for_cleanup();
        run_for(&platform, Duration::from_millis(50)).expect("the loop turns");
    }

    /// The registry's journal replays to exactly the pointer input the
    /// window heard, and is written over the checked-in recording when
    /// blessed.
    #[cfg(all(debug_assertions, feature = "native-capture"))]
    fn hold_recording(
        recorder: super::native_recording::journal::Recorder,
        heard: &[PointerEvent],
    ) {
        let recordings = recorder.finish();
        let [recording] = recordings.as_slice() else {
            panic!("one registry, the seats': {}", recordings.len());
        };
        let replayed: Vec<PointerEvent> = recording
            .replay()
            .into_iter()
            .filter_map(|input| match input {
                PlatformInput::Pointer(event) => Some(event),
                _ => None,
            })
            .collect();
        assert_eq!(replayed.as_slice(), heard, "the journal replays to what the window heard");
        if std::env::var_os("FLUI_NATIVE_RECORDING_BLESS").is_some() {
            let path = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/native_recording/wayland_weston_mouse_wheel.jsonl"
            );
            let mut out = Vec::new();
            recording.write(&mut out).expect("a recording writes");
            std::fs::write(path, out).expect("the recording is written");
        }
    }
}
