//! The native X11 host on a real X server with a window manager: the window
//! against the window conformance checks of `flui-testing`, with the server's
//! own answers as the readback, and the loop's wakes, deadlines, closes and
//! exit.
//!
//! Its own harness: it starts an X server of its own (Xvfb) and a window
//! manager on it (openbox) before anything connects, so its windows are
//! managed the way a desktop manages them, and the server the rest of the
//! suite shares (`xvfb-run`, no window manager) is left alone. The X11 host
//! belongs to the thread that builds it, so each case builds its platform on
//! the harness's one thread.

fn main() {
    #[cfg(target_os = "linux")]
    cases::main();
}

#[cfg(target_os = "linux")]
mod cases {
    use std::cell::RefCell;
    use std::process::{Child, Command, Stdio};
    use std::rc::Rc;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use flui_foundation::geometry::{Size, WindowMetrics};
    use flui_native::testing::app::{AppCall, AppLog, with_log};
    use flui_native::testing::x11::run_for;
    use flui_native::{AppHandler, HostWindow, Platform as _, X11Platform};
    use flui_platform::{
        Availability, CloseDecision, PlatformWindow, WindowCommand, WindowMode, WindowOptions,
        WindowReveal,
    };
    use flui_testing::window_commands::AdmitAndApply as _;
    use flui_testing::window_conformance::{self, Readback, WindowFixture};
    use flui_testing::window_handler::{HandlerCall, RecordingWindowHandler};
    use raw_window_handle::{HandleError, HasWindowHandle as _, RawWindowHandle};
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::{
        AtomEnum, ClientMessageEvent, ConnectionExt as _, CreateWindowAux, EventMask, MapState,
        PropMode, WindowClass,
    };
    use x11rb::rust_connection::RustConnection;
    use x11rb::wrapper::ConnectionExt as _;

    const CASES: &[(&str, fn())] = &[
        (
            "a_decorated_window_is_placed_and_measured_by_its_frame",
            a_decorated_window_is_placed_and_measured_by_its_frame,
        ),
        ("the_x11_window_conforms", the_x11_window_conforms),
        (
            "a_window_opened_in_a_mode_is_mapped_in_it",
            a_window_opened_in_a_mode_is_mapped_in_it,
        ),
        (
            "the_window_managers_close_asks_the_handler",
            the_window_managers_close_asks_the_handler,
        ),
        (
            "a_frame_asked_for_on_another_thread_wakes_the_loop",
            a_frame_asked_for_on_another_thread_wakes_the_loop,
        ),
        (
            "the_loop_ends_with_its_last_window",
            the_loop_ends_with_its_last_window,
        ),
        (
            "a_wake_deadline_no_frame_serviced_is_delivered_once",
            a_wake_deadline_no_frame_serviced_is_delivered_once,
        ),
    ];

    pub(super) fn main() {
        let mut arguments = libtest_mimic::Arguments::from_args();
        // One thread: an X11 platform belongs to the thread that builds it.
        arguments.test_threads = Some(1);
        let filtered_out = |name: &str| {
            arguments.filter.as_deref().is_some_and(|filter| {
                if arguments.exact {
                    name != filter
                } else {
                    !name.contains(filter)
                }
            })
        };
        // A listing, or a run with nothing to run, starts no server.
        let session = (!arguments.list && CASES.iter().any(|(name, _)| !filtered_out(name)))
            .then(Session::start);
        let trials = CASES
            .iter()
            .map(|&(name, case)| {
                libtest_mimic::Trial::test(name, move || {
                    case();
                    Ok(())
                })
            })
            .collect();
        let conclusion = libtest_mimic::run(&arguments, trials);
        drop(session);
        conclusion.exit();
    }

    /// An X server of the harness's own with a window manager on it, which
    /// `DISPLAY` names for the rest of the process.
    struct Session {
        server: Child,
        window_manager: Child,
    }

    impl Session {
        /// Start Xvfb and openbox on it, and wait until the window manager
        /// has announced itself.
        fn start() -> Self {
            let (server, display) = Self::server();
            Self::finish(server, &display)
        }

        /// Start Xvfb on a display number no other server holds. Processes
        /// start at once (nextest runs each case in its own), so each tries
        /// numbers from one its process id picks, and a server that finds
        /// its number taken exits at once and the next is tried. (`-displayfd`
        /// would let the server pick, but it needs a writable
        /// `/tmp/.X11-unix`, which WSLg's is not.)
        fn server() -> (Child, String) {
            let first = 100 + std::process::id() % 800;
            for number in (first..first + 200).map(|number| 100 + number % 900) {
                let display = format!(":{number}");
                let mut server = Command::new("Xvfb")
                    .args([&display, "-screen", "0", "1280x1024x24", "-nolisten", "tcp"])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .expect("Xvfb starts (apt-get install xvfb)");
                let deadline = Instant::now() + Duration::from_secs(10);
                loop {
                    if server.try_wait().ok().flatten().is_some() {
                        // The number was taken.
                        break;
                    }
                    // The server may listen on an abstract socket only,
                    // where `/tmp/.X11-unix` is not writable, so it is asked;
                    // still running a moment later, the server that answered
                    // is this one.
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

        fn finish(server: Child, display: &str) -> Self {
            // SAFETY: the harness has started no thread yet; nothing else
            // reads or writes the environment while it is set.
            #[expect(
                unsafe_code,
                reason = "the environment is set before any thread exists"
            )]
            unsafe {
                std::env::set_var("DISPLAY", display);
                std::env::remove_var("WAYLAND_DISPLAY");
            }
            let window_manager = Command::new("openbox")
                .arg("--sm-disable")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("openbox starts (apt-get install openbox)");
            // The window manager is ready once it manages a window: a probe
            // window mapped now gets the `WM_STATE` only a window manager
            // sets.
            let connection = RustConnection::connect(None)
                .expect("the X server answers")
                .0;
            let root = connection.setup().roots[0].root;
            let probe = connection.generate_id().expect("an id");
            connection
                .create_window(
                    x11rb::COPY_DEPTH_FROM_PARENT,
                    probe,
                    root,
                    0,
                    0,
                    10,
                    10,
                    0,
                    WindowClass::INPUT_OUTPUT,
                    x11rb::COPY_FROM_PARENT,
                    &CreateWindowAux::new(),
                )
                .expect("request");
            let wm_state = atom(&connection, "WM_STATE");
            until("the window manager", || {
                // Mapped again until it is managed: a map before the window
                // manager redirected the root's maps is not one it hears of.
                let _ = connection.map_window(probe);
                let _ = connection.flush();
                std::thread::sleep(Duration::from_millis(20));
                connection
                    .get_property(false, probe, wm_state, wm_state, 0, 2)
                    .ok()
                    .and_then(|cookie| cookie.reply().ok())
                    .is_some_and(|reply| reply.value_len > 0)
            });
            connection.destroy_window(probe).expect("request");
            connection.flush().expect("flush");
            Self {
                server,
                window_manager,
            }
        }
    }

    impl Drop for Session {
        fn drop(&mut self) {
            for child in [&mut self.window_manager, &mut self.server] {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    /// Wait, bounded at 10 s, until `done` holds.
    fn until(what: &str, done: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn atom(connection: &RustConnection, name: &str) -> u32 {
        connection
            .intern_atom(false, name.as_bytes())
            .expect("request")
            .reply()
            .expect("the atom")
            .atom
    }

    /// The X id behind a window's surface target, as an embedder reads it.
    fn xid_of(window: &dyn HostWindow) -> u32 {
        let target = window.surface_target();
        let handle = target.window_handle().expect("an open window has a handle");
        match handle.as_raw() {
            RawWindowHandle::Xcb(handle) => handle.window.get(),
            other => panic!("an X11 window hands out an XCB handle, not {other:?}"),
        }
    }

    /// The `_NET_WM_STATE` atoms of `xid` by name.
    fn state_names(server: &RustConnection, xid: u32) -> Vec<String> {
        let state = atom(server, "_NET_WM_STATE");
        let atoms: Vec<u32> = server
            .get_property(false, xid, state, AtomEnum::ATOM, 0, 64)
            .expect("request")
            .reply()
            .expect("the property")
            .value32()
            .map(Iterator::collect)
            .unwrap_or_default();
        atoms
            .into_iter()
            .map(|atom| {
                let name = server
                    .get_atom_name(atom)
                    .expect("request")
                    .reply()
                    .expect("the name")
                    .name;
                String::from_utf8(name).expect("an atom name")
            })
            .collect()
    }

    /// Send the window manager's close (`WM_DELETE_WINDOW`) to `xid`.
    fn send_close(server: &RustConnection, xid: u32) {
        let protocols = atom(server, "WM_PROTOCOLS");
        let delete = atom(server, "WM_DELETE_WINDOW");
        server
            .send_event(
                false,
                xid,
                EventMask::NO_EVENT,
                ClientMessageEvent::new(32, xid, protocols, [delete, 0, 0, 0, 0]),
            )
            .expect("request");
        server.flush().expect("flush");
    }

    /// An X11 window for the conformance checks, read back on a connection of
    /// the test's own. Dropping it closes the window and settles, so a case
    /// that fails part way leaves no window for the next one.
    struct X11Fixture {
        platform: X11Platform,
        window: Rc<dyn HostWindow>,
        xid: u32,
        server: RustConnection,
        /// `_NET_SUPPORTED` as the window manager left it, put back as the
        /// fixture goes.
        supported: Option<Vec<u32>>,
    }

    impl X11Fixture {
        fn open(title: &str) -> Self {
            let platform = X11Platform::new().expect("the X11 platform");
            let window = platform
                .open_window(
                    WindowOptions::default()
                        .with_title(title)
                        .with_size(Size::new(400.0, 300.0)),
                    RecordingWindowHandler::new(),
                )
                .expect("an X11 window opens");
            let xid = xid_of(window.as_ref());
            let server = RustConnection::connect(None)
                .expect("the X server answers")
                .0;
            let mut fixture = Self {
                platform,
                window,
                xid,
                server,
                supported: None,
            };
            fixture.settle();
            fixture
        }

        fn root(&self) -> u32 {
            self.server.setup().roots[0].root
        }
    }

    impl WindowFixture for X11Fixture {
        fn window(&self) -> Rc<dyn PlatformWindow> {
            Rc::clone(&self.window) as Rc<dyn PlatformWindow>
        }

        fn readback(&self) -> Readback {
            let server = &self.server;
            let attributes = server
                .get_window_attributes(self.xid)
                .ok()
                .and_then(|cookie| cookie.reply().ok());
            let viewable = attributes.is_some_and(|reply| reply.map_state == MapState::VIEWABLE);
            let geometry = server
                .get_geometry(self.xid)
                .ok()
                .and_then(|cookie| cookie.reply().ok());
            let metrics = geometry.filter(|_| viewable).and_then(|geometry| {
                WindowMetrics::new(
                    Size::new(i32::from(geometry.width), i32::from(geometry.height)),
                    1.0,
                )
                .ok()
            });
            let focus = server
                .get_input_focus()
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .map(|reply| reply.focus);
            let active_atom = atom(server, "_NET_ACTIVE_WINDOW");
            let active = server
                .get_property(false, self.root(), active_atom, AtomEnum::WINDOW, 0, 1)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .and_then(|reply| reply.value32()?.next());
            Readback {
                metrics,
                focused: focus == Some(self.xid),
                active: active == Some(self.xid),
                visible: viewable,
                // X11 draws no material behind a window, and none is admitted.
                material: None,
            }
        }

        fn settle(&mut self) {
            // Until the snapshot stops moving: the window manager answers a
            // request with several events, and some after a while.
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut last = self.window.state().revision;
            let mut quiet_since = Instant::now();
            while Instant::now() < deadline {
                run_for(&self.platform, Duration::from_millis(30)).expect("the loop turns");
                let now = self.window.state().revision;
                if now == last {
                    if quiet_since.elapsed() >= Duration::from_millis(300) {
                        return;
                    }
                } else {
                    last = now;
                    quiet_since = Instant::now();
                }
            }
        }

        fn close(&mut self) {
            send_close(&self.server, self.xid);
            self.settle();
        }

        /// The window manager stops supporting fullscreen: the window
        /// revises what it honours, and the title stays honoured.
        fn revise_capabilities(&mut self) -> bool {
            let root = self.root();
            let supported_atom = atom(&self.server, "_NET_SUPPORTED");
            let fullscreen = atom(&self.server, "_NET_WM_STATE_FULLSCREEN");
            let supported: Vec<u32> = self
                .server
                .get_property(false, root, supported_atom, AtomEnum::ATOM, 0, 1024)
                .expect("request")
                .reply()
                .expect("the property")
                .value32()
                .map(Iterator::collect)
                .unwrap_or_default();
            let revised: Vec<u32> = supported
                .iter()
                .copied()
                .filter(|atom| *atom != fullscreen)
                .collect();
            self.server
                .change_property32(
                    PropMode::REPLACE,
                    root,
                    supported_atom,
                    AtomEnum::ATOM,
                    &revised,
                )
                .expect("request");
            self.server.flush().expect("flush");
            self.supported = Some(supported);
            self.settle();
            true
        }

        /// An X11 window takes no input region: its input capabilities say so.
        fn input_regions(&self) -> Availability {
            Availability::Unavailable
        }

        fn has_native_surface(&self) -> bool {
            true
        }
    }

    impl Drop for X11Fixture {
        fn drop(&mut self) {
            if let Some(supported) = self.supported.take() {
                let root = self.root();
                let supported_atom = atom(&self.server, "_NET_SUPPORTED");
                let _ = self.server.change_property32(
                    PropMode::REPLACE,
                    root,
                    supported_atom,
                    AtomEnum::ATOM,
                    &supported,
                );
                let _ = self.server.flush();
            }
            self.window.close_for_cleanup();
            self.settle();
        }
    }

    /// A decorated window's outer rectangle is its frame's: the window
    /// manager places the frame where the window was asked to go, and the
    /// snapshot names that place and the frame's size, not the content's.
    fn a_decorated_window_is_placed_and_measured_by_its_frame() {
        let mut fixture = X11Fixture::open("framed");
        let extents_atom = atom(&fixture.server, "_NET_FRAME_EXTENTS");
        let extents: Vec<u32> = fixture
            .server
            .get_property(false, fixture.xid, extents_atom, AtomEnum::CARDINAL, 0, 4)
            .expect("request")
            .reply()
            .expect("the property")
            .value32()
            .map(Iterator::collect)
            .unwrap_or_default();
        let [left, right, top, bottom] = extents[..] else {
            panic!("openbox frames a decorated window: {extents:?}");
        };
        assert!(left + right + top + bottom > 0, "a frame: {extents:?}");
        fixture
            .window
            .admit_and_apply(WindowCommand::SetPosition(
                flui_foundation::geometry::Point::new(120.0, 90.0),
            ))
            .expect("a window moves");
        fixture.settle();
        let content = fixture
            .server
            .translate_coordinates(fixture.xid, fixture.root(), 0, 0)
            .expect("request")
            .reply()
            .expect("the content's place");
        let as_f64 = |value: u32| f64::from(value);
        assert_eq!(
            (f64::from(content.dst_x), f64::from(content.dst_y)),
            (120.0 + as_f64(left), 90.0 + as_f64(top)),
            "the frame is where the window was asked to go"
        );
        let state = fixture.window.state();
        assert_eq!(
            state.outer.position,
            Some(flui_foundation::geometry::Point::new(120.0, 90.0))
        );
        assert_eq!(
            state.outer.size,
            Size::new(400.0 + as_f64(left + right), 300.0 + as_f64(top + bottom)),
            "the outer size is the content's grown by the frame"
        );
        assert_eq!(state.content_size, Size::new(400.0, 300.0));
    }

    fn the_x11_window_conforms() {
        let mut fixture = X11Fixture::open("window conformance");
        window_conformance::assert_conforms(&mut fixture);
    }

    /// A window opened maximized is mapped once `_NET_WM_STATE` holds the
    /// mode, so the window manager maps it maximized and it never shows in
    /// normal mode first.
    fn a_window_opened_in_a_mode_is_mapped_in_it() {
        let platform = X11Platform::new().expect("the X11 platform");
        let server = RustConnection::connect(None)
            .expect("the X server answers")
            .0;
        let window = platform
            .open_window(
                WindowOptions::default()
                    .with_size(Size::new(320.0, 240.0))
                    .with_mode(WindowMode::Maximized)
                    .with_reveal(WindowReveal::AtOpen),
                RecordingWindowHandler::new(),
            )
            .expect("an X11 window opens");
        let xid = xid_of(window.as_ref());
        let names = state_names(&server, xid);
        assert!(
            names.contains(&"_NET_WM_STATE_MAXIMIZED_HORZ".to_owned())
                && names.contains(&"_NET_WM_STATE_MAXIMIZED_VERT".to_owned()),
            "the window manager reads the mode from the property: {names:?}"
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        // The mode is in the properties at once; the window manager maps the
        // window in it.
        assert_eq!(window.state().mode, WindowMode::Maximized);
        while !window.state().visible {
            assert!(
                Instant::now() < deadline,
                "the window manager did not map the window: {:?}",
                window.state()
            );
            run_for(&platform, Duration::from_millis(20)).expect("the loop turns");
        }
        assert_eq!(
            window.state().mode,
            WindowMode::Maximized,
            "mapped maximized"
        );
        window.close_for_cleanup();
        run_for(&platform, Duration::from_millis(50)).expect("the loop turns");
    }

    /// The window manager's close asks the handler: kept open when it says
    /// so, closed when it grants it, after which the window is gone from the
    /// server and its surface target hands out nothing.
    fn the_window_managers_close_asks_the_handler() {
        let platform = X11Platform::new().expect("the X11 platform");
        let server = RustConnection::connect(None)
            .expect("the X server answers")
            .0;
        let handler = RecordingWindowHandler::new();
        let answers = Rc::new(RefCell::new(vec![
            CloseDecision::Close,
            CloseDecision::KeepOpen,
        ]));
        let answered = Rc::clone(&answers);
        handler.react_to_close_request(move || {
            answered.borrow_mut().pop().expect("an answer per request")
        });
        let window = platform
            .open_window(WindowOptions::default(), Rc::clone(&handler) as _)
            .expect("an X11 window opens");
        let xid = xid_of(window.as_ref());
        let target = window.surface_target();
        run_for(&platform, Duration::from_millis(200)).expect("the loop turns");

        send_close(&server, xid);
        run_for(&platform, Duration::from_millis(200)).expect("the loop turns");
        assert_eq!(handler.count(&HandlerCall::CloseRequested), 1);
        assert_eq!(
            handler.count(&HandlerCall::Closed),
            0,
            "the handler kept it"
        );
        window
            .admit_and_apply(WindowCommand::SetTitle("kept".into()))
            .expect("a kept window takes commands");

        send_close(&server, xid);
        run_for(&platform, Duration::from_millis(200)).expect("the loop turns");
        assert_eq!(handler.count(&HandlerCall::CloseRequested), 2);
        assert_eq!(
            handler.count(&HandlerCall::Closed),
            1,
            "the handler granted it"
        );
        assert!(
            matches!(target.window_handle(), Err(HandleError::Unavailable)),
            "a closed window's target hands out no handle"
        );
        until("the X window to be destroyed", || {
            server
                .get_window_attributes(xid)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .is_none()
        });
    }

    /// A frame asked for on another thread wakes a loop that waits with no
    /// deadline: the loop's doorbell, not a poll.
    fn a_frame_asked_for_on_another_thread_wakes_the_loop() {
        let platform = Box::new(X11Platform::new().expect("the X11 platform"));
        let framed = Arc::new(AtomicBool::new(false));
        let seen = Arc::clone(&framed);
        let app = with_log(move |log, owner| {
            log.answer_deadline(None);
            let proxy = owner.proxy();
            let handler = RecordingWindowHandler::new();
            let window = owner
                .open_window(
                    WindowOptions::default().with_visible(false),
                    Rc::clone(&handler) as _,
                )?
                .try_ready()?;
            let quit = proxy.clone();
            handler.react_to_frame(move |_| {
                seen.store(true, Ordering::SeqCst);
                let _ = quit.request_quit();
            });
            let requester = window.redraw_requester();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(200));
                requester.request_redraw();
            });
            // The case fails rather than hangs if the frame never wakes it.
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(10));
                let _ = proxy.request_quit();
            });
            // The window lives as long as the loop's table holds it.
            drop(window);
            Ok(())
        });
        let started = Instant::now();
        platform.run(app).expect("the loop runs");
        assert!(
            framed.load(Ordering::SeqCst),
            "the frame asked for on another thread never reached the handler"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the frame came only with the watchdog's quit"
        );
    }

    /// Ask `proxy`'s loop to quit after `after`, so a case that would hang
    /// fails on its own assertions instead.
    fn quit_after(proxy: flui_native::PlatformProxy, after: Duration) {
        std::thread::spawn(move || {
            std::thread::sleep(after);
            let _ = proxy.request_quit();
        });
    }

    /// The loop ends when its last window closes and the application lets
    /// it, and the application hears the quit last.
    fn the_loop_ends_with_its_last_window() {
        let platform = Box::new(X11Platform::new().expect("the X11 platform"));
        let log = AppLog::new();
        log.answer_exit(true);
        log.bootstrap(|owner| {
            quit_after(owner.proxy(), Duration::from_secs(10));
            let window = owner
                .open_window(WindowOptions::default(), RecordingWindowHandler::new())?
                .try_ready()?;
            window
                .admit_and_apply(WindowCommand::Close)
                .map_err(|refusal| format!("{refusal:?}"))?;
            Ok(())
        });
        let started = Instant::now();
        platform
            .run(Rc::clone(&log) as Rc<dyn AppHandler>)
            .expect("the loop runs");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the loop ran on after its last window closed"
        );
        let calls = log.calls();
        assert!(calls.contains(&AppCall::ExitPolicy), "{calls:?}");
        assert_eq!(calls.last(), Some(&AppCall::Quit), "{calls:?}");
    }

    /// A wake deadline whose frame no handler ran is delivered once, not on
    /// every turn: the loop waits for the next event instead of spinning.
    fn a_wake_deadline_no_frame_serviced_is_delivered_once() {
        let platform = Box::new(X11Platform::new().expect("the X11 platform"));
        let log = AppLog::new();
        // One instant, answered again on every turn.
        log.answer_deadline(Some(web_time::Instant::now() + Duration::from_millis(50)));
        // A hidden window, which no expose asks for a frame, whose handler
        // declines every frame, as one with nothing to draw with does.
        let handler = RecordingWindowHandler::new();
        let opened = Rc::clone(&handler);
        log.bootstrap(move |owner| {
            quit_after(owner.proxy(), Duration::from_millis(600));
            let _window = owner
                .open_window(WindowOptions::default().with_visible(false), opened)?
                .try_ready()?;
            Ok(())
        });
        platform
            .run(Rc::clone(&log) as Rc<dyn AppHandler>)
            .expect("the loop runs");
        let questions = log.count(AppCall::WakeDeadline);
        assert!(
            questions < 20,
            "the loop spun on a deadline no frame serviced: asked {questions} times"
        );
        let frames = handler
            .calls()
            .iter()
            .filter(|call| matches!(call, HandlerCall::FrameRequested(_)))
            .count();
        assert_eq!(frames, 1, "the deadline asked for one frame");
    }
}
