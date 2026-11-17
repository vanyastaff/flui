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

#![allow(
    clippy::disallowed_methods,
    reason = "test code catches a panic to assert on it (ADR-0303 §5)"
)]
fn main() {
    #[cfg(target_os = "linux")]
    cases::main();
}

// The X11 host's input on the harness's server, through XTest.
#[cfg(target_os = "linux")]
mod x11_input;
// The native recordings an input session captured are held against.
#[cfg(all(target_os = "linux", debug_assertions, feature = "native-capture"))]
mod native_recording;

#[cfg(target_os = "linux")]
mod cases {
    use std::cell::RefCell;
    use std::process::{Child, Command, Stdio};
    use std::rc::Rc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use flui_foundation::geometry::{Size, WindowMetrics};
    use flui_native::testing::app::{AppCall, AppLog, with_log};
    use flui_native::testing::x11::run_for;
    use flui_native::{
        AppHandler, CurrentHost, HostStart, HostWindow, NativeExecutor, Platform as _,
        WindowRequest, X11Platform,
    };
    use flui_platform::{
        CloseDecision, CursorGrab, PlatformWindow, WindowCommand, WindowHandler, WindowMode,
        WindowOptions, WindowReveal,
    };
    use flui_testing::window_commands::AdmitAndApply as _;
    use flui_testing::window_conformance::{self, Readback, WindowFixture};
    use flui_testing::window_handler::{HandlerCall, RecordingWindowHandler};
    use raw_window_handle::{HandleError, HasWindowHandle as _, RawWindowHandle};
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::{
        AtomEnum, ClientMessageEvent, ConnectionExt as _, CreateWindowAux, EventMask, GrabMode,
        GrabStatus, InputFocus, MapState, PropMode, WindowClass,
    };
    use x11rb::rust_connection::RustConnection;
    use x11rb::wrapper::ConnectionExt as _;

    const CASES: &[(&str, fn())] = &[
        (
            "the_host_runs_its_io_on_the_runtime_it_is_handed",
            the_host_runs_its_io_on_the_runtime_it_is_handed,
        ),
        (
            "a_window_carries_the_application_id_it_was_started_with",
            a_window_carries_the_application_id_it_was_started_with,
        ),
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
            "a_window_destroyed_by_another_client_closes_and_lets_go",
            a_window_destroyed_by_another_client_closes_and_lets_go,
        ),
        (
            "a_covered_window_reads_occluded_and_shown",
            a_covered_window_reads_occluded_and_shown,
        ),
        (
            "a_frame_asked_for_on_another_thread_wakes_the_loop",
            a_frame_asked_for_on_another_thread_wakes_the_loop,
        ),
        (
            "a_window_asked_for_on_another_thread_opens_on_the_loop",
            a_window_asked_for_on_another_thread_opens_on_the_loop,
        ),
        (
            "the_loop_ends_with_its_last_window",
            the_loop_ends_with_its_last_window,
        ),
        (
            "a_wake_deadline_no_frame_serviced_is_delivered_once",
            a_wake_deadline_no_frame_serviced_is_delivered_once,
        ),
        (
            "a_turn_deadline_gives_the_owner_a_turn_with_no_window",
            a_turn_deadline_gives_the_owner_a_turn_with_no_window,
        ),
        (
            "a_parked_exit_policy_panic_comes_out_ahead_of_a_later_one",
            a_parked_exit_policy_panic_comes_out_ahead_of_a_later_one,
        ),
        (
            "a_surface_targets_display_equals_the_platforms_connection",
            a_surface_targets_display_equals_the_platforms_connection,
        ),
        (
            "a_later_launch_while_no_window_is_shown_asks_to_reopen",
            a_later_launch_while_no_window_is_shown_asks_to_reopen,
        ),
        (
            "the_window_reopened_for_a_later_launch_comes_forward",
            the_window_reopened_for_a_later_launch_comes_forward,
        ),
        (
            "a_window_outlives_a_window_manager_that_was_killed",
            a_window_outlives_a_window_manager_that_was_killed,
        ),
        (
            "a_handler_in_the_turn_a_window_manager_dies_acts_without_it",
            a_handler_in_the_turn_a_window_manager_dies_acts_without_it,
        ),
        (
            "a_check_window_that_is_no_window_managers_is_refused",
            a_check_window_that_is_no_window_managers_is_refused,
        ),
        (
            "a_window_manager_without_ewmh_is_followed",
            a_window_manager_without_ewmh_is_followed,
        ),
        (
            "a_minimized_window_that_is_hidden_reads_hidden",
            a_minimized_window_that_is_hidden_reads_hidden,
        ),
        (
            "a_command_acts_on_the_window_as_it_is_now",
            a_command_acts_on_the_window_as_it_is_now,
        ),
        (
            "a_window_minimized_as_it_hears_its_show_stays_minimized",
            a_window_minimized_as_it_hears_its_show_stays_minimized,
        ),
        (
            "a_stream_of_events_leaves_the_frames_their_turns",
            a_stream_of_events_leaves_the_frames_their_turns,
        ),
    ];

    /// The harness's window manager, which a case that kills it starts again
    /// before it ends: the cases after it are managed too.
    static WINDOW_MANAGER: Mutex<Option<Child>> = Mutex::new(None);

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
        let cases: Vec<(&str, fn())> = CASES
            .iter()
            .chain(crate::x11_input::CASES)
            .copied()
            .chain(recording_cases())
            .collect();
        let session = (!arguments.list && cases.iter().any(|(name, _)| !filtered_out(name)))
            .then(Session::start);
        let trials = cases
            .into_iter()
            .map(|(name, case)| {
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

    /// The recordings an X11 session is held against are this harness's too,
    /// with their codec's contracts, in a build that captures.
    fn recording_cases() -> Vec<(&'static str, fn())> {
        // The list is the module's whole, so nothing it builds is unused
        // here; `platform_it` runs the rest.
        #[cfg(all(debug_assertions, feature = "native-capture"))]
        return crate::native_recording::contracts()
            .into_iter()
            .filter(|(name, _)| name.contains("x11"))
            .collect();
        #[cfg(not(all(debug_assertions, feature = "native-capture")))]
        Vec::new()
    }

    /// An X server of the harness's own with a window manager on it, which
    /// `DISPLAY` names for the rest of the process, and a session bus of its
    /// own, which `DBUS_SESSION_BUS_ADDRESS` names (none when no `dbus-daemon`
    /// is installed).
    struct Session {
        server: Child,
        bus: Option<Child>,
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

        /// A `dbus-daemon` that activates no service, so the session's own
        /// services (a portal, the accessibility bus) stay out of the cases,
        /// and its address.
        fn bus() -> Option<(Child, String)> {
            let config = std::env::temp_dir()
                .join(format!("flui-x11-window-bus-{}.conf", std::process::id()));
            std::fs::write(
                &config,
                "<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen>\
                 <auth>EXTERNAL</auth><policy context=\"default\">\
                 <allow send_destination=\"*\" eavesdrop=\"true\"/>\
                 <allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>",
            )
            .ok()?;
            let mut daemon = Command::new("dbus-daemon")
                .arg(format!("--config-file={}", config.display()))
                .args(["--nofork", "--print-address"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .ok()?;
            let mut address = String::new();
            let read = std::io::BufRead::read_line(
                &mut std::io::BufReader::new(daemon.stdout.take()?),
                &mut address,
            );
            let _ = std::fs::remove_file(&config);
            let address = address.trim().to_owned();
            if read.is_err() || address.is_empty() {
                let _ = daemon.kill();
                let _ = daemon.wait();
                return None;
            }
            Some((daemon, address))
        }

        fn finish(server: Child, display: &str) -> Self {
            let bus = Self::bus();
            #[expect(
                unsafe_code,
                reason = "the environment is set before any thread exists"
            )]
            // SAFETY: the harness has started no thread yet; nothing else
            // reads or writes the environment while it is set.
            unsafe {
                std::env::set_var("DISPLAY", display);
                std::env::remove_var("WAYLAND_DISPLAY");
                // A locale whose compose table XKB has, for the dead keys
                // the input cases type; the table is X's, no locale need
                // be installed.
                std::env::set_var("LC_CTYPE", "en_US.UTF-8");
                match &bus {
                    Some((_, address)) => std::env::set_var("DBUS_SESSION_BUS_ADDRESS", address),
                    None => std::env::remove_var("DBUS_SESSION_BUS_ADDRESS"),
                }
            }
            start_window_manager();
            Self {
                server,
                bus: bus.map(|(daemon, _)| daemon),
            }
        }
    }

    impl Drop for Session {
        fn drop(&mut self) {
            kill_window_manager();
            for child in std::iter::once(&mut self.server).chain(self.bus.as_mut()) {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    /// Start openbox on the harness's server, and wait until it manages
    /// windows.
    fn start_window_manager() {
        let window_manager = Command::new("openbox")
            .arg("--sm-disable")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("openbox starts (apt-get install openbox)");
        *WINDOW_MANAGER
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(window_manager);
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
    }

    /// Kill the harness's window manager, as a crash does: it says nothing on
    /// the root window as it goes, and the server destroys its windows.
    fn kill_window_manager() {
        let window_manager = WINDOW_MANAGER
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(mut window_manager) = window_manager {
            let _ = window_manager.kill();
            let _ = window_manager.wait();
        }
    }

    /// Starts the harness's window manager again as it drops, whatever the
    /// case that killed it found: the cases after it are managed too.
    struct StartedAgain;

    impl Drop for StartedAgain {
        fn drop(&mut self) {
            start_window_manager();
        }
    }

    /// The window the root's `_NET_SUPPORTING_WM_CHECK` names.
    fn check_window(server: &RustConnection) -> u32 {
        let root = server.setup().roots[0].root;
        server
            .get_property(
                false,
                root,
                atom(server, "_NET_SUPPORTING_WM_CHECK"),
                AtomEnum::WINDOW,
                0,
                1,
            )
            .expect("request")
            .reply()
            .expect("the property")
            .value32()
            .and_then(|mut words| words.next())
            .expect("the window manager names its window")
    }

    /// Wait, bounded at 10 s, until `done` holds.
    fn until(what: &str, done: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub(crate) fn atom(connection: &RustConnection, name: &str) -> u32 {
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
            .map_or_default(Iterator::collect);
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
    pub(crate) struct X11Fixture {
        pub(crate) platform: X11Platform,
        pub(crate) window: Rc<dyn HostWindow>,
        pub(crate) xid: u32,
        pub(crate) server: RustConnection,
        /// `_NET_SUPPORTED` as the window manager left it, put back as the
        /// fixture goes.
        supported: Option<Vec<u32>>,
        /// Windows of the test's own client, destroyed as the fixture goes.
        others: Vec<u32>,
    }

    impl X11Fixture {
        fn open(title: &str) -> Self {
            Self::open_with(title, RecordingWindowHandler::new())
        }

        /// A window delivering to `handler`.
        pub(crate) fn open_with(title: &str, handler: Rc<RecordingWindowHandler>) -> Self {
            Self::open_with_handler(title, handler)
        }

        /// A window delivering to any `handler`.
        pub(crate) fn open_with_handler(
            title: &str,
            handler: Rc<dyn flui_platform::WindowHandler>,
        ) -> Self {
            Self::open_with_options(
                WindowOptions::default()
                    .title(title)
                    .size(Size::new(400.0, 300.0)),
                handler,
            )
        }

        /// A window opened with `options`, delivering to `handler`.
        pub(crate) fn open_with_options(
            options: WindowOptions,
            handler: Rc<dyn flui_platform::WindowHandler>,
        ) -> Self {
            let platform = X11Platform::try_new().expect("the X11 platform");
            Self::open_on(platform, options, handler)
        }

        /// A window of `platform` opened with `options`, delivering to
        /// `handler`.
        pub(crate) fn open_on(
            platform: X11Platform,
            options: WindowOptions,
            handler: Rc<dyn flui_platform::WindowHandler>,
        ) -> Self {
            let window = platform
                .open_window(options, handler)
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
                others: Vec::new(),
            };
            fixture.settle();
            fixture
        }

        pub(crate) fn root(&self) -> u32 {
            self.server.setup().roots[0].root
        }

        /// A window of the test's own client, mapped and managed by the
        /// window manager (it set `WM_STATE`), which the fixture destroys as it
        /// goes.
        pub(crate) fn open_other_window(&mut self) -> u32 {
            let server = &self.server;
            let other = server.generate_id().expect("an id");
            server
                .create_window(
                    x11rb::COPY_DEPTH_FROM_PARENT,
                    other,
                    self.root(),
                    0,
                    0,
                    200,
                    150,
                    0,
                    WindowClass::INPUT_OUTPUT,
                    x11rb::COPY_FROM_PARENT,
                    &CreateWindowAux::new(),
                )
                .expect("request");
            server.map_window(other).expect("request");
            server.flush().expect("flush");
            let wm_state = atom(server, "WM_STATE");
            until("the window manager to manage the other window", || {
                let managed = server
                    .get_property(false, other, wm_state, wm_state, 0, 2)
                    .ok()
                    .and_then(|cookie| cookie.reply().ok())
                    .is_some_and(|reply| reply.value_len > 0);
                let viewable = server
                    .get_window_attributes(other)
                    .ok()
                    .and_then(|cookie| cookie.reply().ok())
                    .is_some_and(|reply| reply.map_state == MapState::VIEWABLE);
                managed && viewable
            });
            self.others.push(other);
            self.settle();
            other
        }

        /// Whether a client other than the test's holds the pointer: the
        /// server refuses the test's own grab as already taken, and the test
        /// lets go of one it took at once.
        pub(crate) fn pointer_grabbed_elsewhere(&self) -> bool {
            let (other, _) = RustConnection::connect(None).expect("the X server answers");
            let status = other
                .grab_pointer(
                    false,
                    self.root(),
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
            status == GrabStatus::ALREADY_GRABBED
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
            let map_state = attributes.map(|reply| reply.map_state);
            let viewable = map_state == Some(MapState::VIEWABLE);
            // `WM_STATE`'s first word, as the window manager sets it: 3 is
            // iconic (ICCCM 4.1.3.1).
            let wm_state = atom(server, "WM_STATE");
            let iconic = server
                .get_property(false, self.xid, wm_state, wm_state, 0, 1)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .and_then(|reply| reply.value32()?.next())
                == Some(3);
            let geometry = server
                .get_geometry(self.xid)
                .ok()
                .and_then(|cookie| cookie.reply().ok());
            let metrics = geometry.filter(|_| viewable).and_then(|geometry| {
                WindowMetrics::try_new(
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
                // Shown: mapped by the application, or unmapped by the
                // window manager that iconified it.
                visible: map_state.is_some_and(|state| state != MapState::UNMAPPED) || iconic,
                // X reports a covering only as an event.
                occluded: None,
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
                .map_or_default(Iterator::collect);
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

        /// An X11 window takes the chrome regions (ADR-0286 §10), as its input
        /// capabilities say.
        fn input_capabilities(&self) -> flui_platform::input_capabilities::InputCapabilities {
            flui_native::input::availability::x11(false)
        }

        fn has_native_surface(&self) -> bool {
            true
        }

        /// The server's answer to a grab of the test's own: one it refuses as
        /// already taken is the window's (no other client of the harness
        /// grabs). The server says that a client holds the pointer, not how,
        /// so a hold reads as `Confined`, the one the check asks of an X11
        /// window, which confines before it locks.
        fn held_grab(&self) -> Option<CursorGrab> {
            Some(if self.pointer_grabbed_elsewhere() {
                CursorGrab::Confined
            } else {
                CursorGrab::None
            })
        }

        /// A window of the test's own client takes the focus from the
        /// server (`SetInputFocus`), as a click on it would through the
        /// window manager.
        fn take_focus_away(&mut self) -> bool {
            let other = self.open_other_window();
            self.server
                .set_input_focus(InputFocus::PARENT, other, x11rb::CURRENT_TIME)
                .expect("request");
            self.server.flush().expect("flush");
            self.settle();
            true
        }
    }

    impl Drop for X11Fixture {
        fn drop(&mut self) {
            for other in std::mem::take(&mut self.others) {
                let _ = self.server.destroy_window(other);
            }
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
            .map_or_default(Iterator::collect);
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
            state.outer.position(),
            Some(flui_foundation::geometry::Point::new(120.0, 90.0))
        );
        assert_eq!(
            state.outer.size(),
            Size::new(400.0 + as_f64(left + right), 300.0 + as_f64(top + bottom)),
            "the outer size is the content's grown by the frame"
        );
        assert_eq!(state.content_size, Size::new(400.0, 300.0));
    }

    fn the_x11_window_conforms() {
        let mut fixture = X11Fixture::open("window conformance");
        window_conformance::assert_conforms(&mut fixture);
    }

    /// The platform's display connection answers the display every window
    /// target of the platform answers, and keeps answering it after the
    /// window closed, when the target answers nothing (ADR-0305 §2).
    fn a_surface_targets_display_equals_the_platforms_connection() {
        use raw_window_handle::HasDisplayHandle as _;

        let platform = X11Platform::try_new().expect("the X11 platform");
        let connection = platform.display_connection();
        let window = platform
            .open_window(
                WindowOptions::default().size(Size::new(64.0, 64.0)),
                RecordingWindowHandler::new(),
            )
            .expect("an X11 window opens");
        let target = window.surface_target();
        let display = connection
            .display_handle()
            .expect("the connection answers its display")
            .as_raw();
        assert_eq!(
            target
                .display_handle()
                .expect("an open window answers")
                .as_raw(),
            display,
            "the window's target is on the connection's display"
        );
        assert!(
            matches!(display, raw_window_handle::RawDisplayHandle::Xcb(_)),
            "an XCB display: {display:?}"
        );
        window.close_for_cleanup();
        run_for(&platform, Duration::from_millis(50)).expect("the loop turns");
        assert!(matches!(
            target.display_handle(),
            Err(HandleError::Unavailable)
        ));
        assert_eq!(
            connection
                .display_handle()
                .expect("the connection outlives the window")
                .as_raw(),
            display
        );
    }

    /// A window opened maximized is mapped once `_NET_WM_STATE` holds the
    /// mode, so the window manager maps it maximized and it never shows in
    /// normal mode first.
    fn a_window_opened_in_a_mode_is_mapped_in_it() {
        let platform = X11Platform::try_new().expect("the X11 platform");
        let server = RustConnection::connect(None)
            .expect("the X server answers")
            .0;
        let window = platform
            .open_window(
                WindowOptions::default()
                    .size(Size::new(320.0, 240.0))
                    .mode(WindowMode::Maximized)
                    .reveal(WindowReveal::AtOpen),
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
        let platform = X11Platform::try_new().expect("the X11 platform");
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

    /// A window another client destroys (as `xkill` does) closes: its handler
    /// hears `closed` once and is let go of, its surface target hands out no
    /// handle for the window that is gone, and a frame asked for it reaches
    /// nothing.
    fn a_window_destroyed_by_another_client_closes_and_lets_go() {
        let platform = X11Platform::try_new().expect("the X11 platform");
        let server = RustConnection::connect(None)
            .expect("the X server answers")
            .0;
        let handler = RecordingWindowHandler::new();
        let window = platform
            .open_window(WindowOptions::default(), Rc::clone(&handler) as _)
            .expect("an X11 window opens");
        let xid = xid_of(window.as_ref());
        let target = window.surface_target();
        let requester = window.redraw_requester();
        run_for(&platform, Duration::from_millis(200)).expect("the loop turns");

        server.destroy_window(xid).expect("request");
        server.flush().expect("flush");
        run_for(&platform, Duration::from_millis(200)).expect("the loop turns");

        assert_eq!(handler.count(&HandlerCall::Closed), 1, "closed, once");
        assert_eq!(
            Rc::strong_count(&handler),
            1,
            "the window let go of its handler"
        );
        assert!(
            matches!(target.window_handle(), Err(HandleError::Unavailable)),
            "the target of a window the server destroyed hands out no handle"
        );
        handler.take_calls();
        requester.request_redraw();
        window.request_redraw();
        run_for(&platform, Duration::from_millis(100)).expect("the loop turns");
        assert!(
            handler.take_calls().is_empty(),
            "a destroyed window delivers nothing more"
        );
    }

    /// A window another client covers whole reads occluded and still shown,
    /// and reads unoccluded once the cover goes: the server's
    /// `VisibilityNotify`, not a guess from the mode.
    fn a_covered_window_reads_occluded_and_shown() {
        let platform = X11Platform::try_new().expect("the X11 platform");
        let server = RustConnection::connect(None)
            .expect("the X server answers")
            .0;
        let handler = RecordingWindowHandler::new();
        let window = platform
            .open_window(WindowOptions::default(), Rc::clone(&handler) as _)
            .expect("an X11 window opens");
        let settle = |what: &str, done: &dyn Fn() -> bool| {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !done() {
                assert!(Instant::now() < deadline, "timed out waiting for {what}");
                run_for(&platform, Duration::from_millis(20)).expect("the loop turns");
            }
        };
        settle("the window shown and uncovered", &|| {
            let state = window.state();
            state.visible && !state.occluded
        });

        // Override-redirect, so the window manager neither frames nor places
        // it: it lies over the whole screen.
        let screen = &server.setup().roots[0];
        let cover = server.generate_id().expect("an id");
        server
            .create_window(
                screen.root_depth,
                cover,
                screen.root,
                0,
                0,
                screen.width_in_pixels,
                screen.height_in_pixels,
                0,
                WindowClass::INPUT_OUTPUT,
                0,
                &CreateWindowAux::new()
                    .override_redirect(1)
                    .background_pixel(screen.black_pixel),
            )
            .expect("request");
        server.map_window(cover).expect("request");
        server.flush().expect("flush");
        settle("the window covered", &|| window.state().occluded);
        assert!(window.state().visible, "a covered window is still shown");

        server.destroy_window(cover).expect("request");
        server.flush().expect("flush");
        settle("the window uncovered", &|| !window.state().occluded);
        window.close_for_cleanup();
        run_for(&platform, Duration::from_millis(100)).expect("the loop turns");
    }

    /// A frame asked for on another thread wakes a loop that waits with no
    /// deadline: the loop's doorbell, not a poll.
    fn a_frame_asked_for_on_another_thread_wakes_the_loop() {
        let platform = Box::new(X11Platform::try_new().expect("the X11 platform"));
        let framed = Arc::new(AtomicBool::new(false));
        let seen = Arc::clone(&framed);
        let app = with_log(move |log, owner| {
            log.answer_deadline(None);
            let proxy = owner.proxy();
            let handler = RecordingWindowHandler::new();
            let window = owner
                .open_window(
                    WindowOptions::default().visible(false),
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

    /// A window another thread asks for through the proxy opens on the
    /// loop's thread in its next turn, the requester hears its identity, and
    /// the loop keeps it as it keeps the windows its owner opens: it is drawn
    /// until the loop quits.
    fn a_window_asked_for_on_another_thread_opens_on_the_loop() {
        thread_local! {
            static OPENED: RefCell<Option<Rc<RecordingWindowHandler>>> =
                const { RefCell::new(None) };
        }
        let platform = Box::new(X11Platform::try_new().expect("the X11 platform"));
        let heard = Arc::new(std::sync::Mutex::new(None));
        let answer = Arc::clone(&heard);
        let app = with_log(move |log, owner| {
            log.answer_deadline(None);
            let proxy = owner.proxy();
            quit_after(proxy.clone(), Duration::from_secs(10));
            std::thread::spawn(move || {
                let request = WindowRequest::new(WindowOptions::default(), || {
                    let handler = RecordingWindowHandler::new();
                    OPENED.with(|opened| *opened.borrow_mut() = Some(Rc::clone(&handler)));
                    handler as Rc<dyn WindowHandler>
                });
                let opened = proxy
                    .open_window(request)
                    .expect("the loop takes the open")
                    .wait()
                    .map_err(|error| error.to_string());
                *answer.lock().expect("the answer") = Some(opened);
                std::thread::sleep(Duration::from_millis(500));
                let _ = proxy.request_quit();
            });
            Ok(())
        });
        platform.run(app).expect("the loop runs");

        let opened = heard.lock().expect("the answer").take();
        let id = opened
            .expect("the worker heard an answer")
            .expect("the window opened");
        let handler = OPENED
            .with(|opened| opened.borrow_mut().take())
            .expect("the factory ran on the loop's thread");
        assert!(
            handler
                .calls()
                .iter()
                .any(|call| matches!(call, HandlerCall::FrameRequested(_))),
            "window {id:?} lived to be drawn"
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

    /// The window an application opens, a turn later, for the reopen a later
    /// launch asked of it while it held none comes forward when it is
    /// mapped: the host keeps the launch's activation for it, as Wayland's
    /// table keeps a forwarded token for the first window. Read from the root
    /// window, where the host sends `_NET_ACTIVE_WINDOW` and a client that
    /// selects substructure notifies hears it.
    fn the_window_reopened_for_a_later_launch_comes_forward() {
        const APP_ID: &str = "org.flui.X11WindowCases.Reopened";
        /// `_NET_ACTIVE_WINDOW`'s source indication for an application.
        const SOURCE_APPLICATION: u32 = 1;
        if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
            eprintln!("skipped: no dbus-daemon for a session bus of the harness's own");
            return;
        }
        let watcher = RustConnection::connect(None)
            .expect("the X server answers")
            .0;
        let root = watcher.setup().roots[0].root;
        watcher
            .change_window_attributes(
                root,
                &x11rb::protocol::xproto::ChangeWindowAttributesAux::new()
                    .event_mask(EventMask::SUBSTRUCTURE_NOTIFY),
            )
            .expect("request")
            .check()
            .expect("the root's substructure notifies");
        let active_window = atom(&watcher, "_NET_ACTIVE_WINDOW");

        let HostStart::Started(platform) = CurrentHost::X11
            .start_single(NativeExecutor::own(), &app_id(APP_ID))
            .expect("the X11 platform")
        else {
            panic!("the first launch owns the application's name");
        };
        let (later, forwarded) = std::sync::mpsc::channel();
        let log = AppLog::new();
        log.react(|call, owner| {
            if call != AppCall::Reopen {
                return;
            }
            // Opened a turn later, as the desktop runner opens its main
            // window for a reopen.
            let proxy = owner.proxy();
            std::thread::spawn(move || {
                let request = WindowRequest::new(WindowOptions::default(), || {
                    RecordingWindowHandler::new() as Rc<dyn WindowHandler>
                });
                let _ = proxy
                    .open_window(request)
                    .expect("the loop takes the open")
                    .wait();
                std::thread::sleep(Duration::from_millis(500));
                let _ = proxy.request_quit();
            });
        });
        log.bootstrap(move |owner| {
            quit_after(owner.proxy(), Duration::from_secs(10));
            std::thread::spawn(move || {
                let started = CurrentHost::X11.start_single(NativeExecutor::own(), &app_id(APP_ID));
                let _ = later.send(matches!(started, Ok(HostStart::Forwarded)));
            });
            Ok(())
        });
        platform
            .run(Rc::clone(&log) as Rc<dyn AppHandler>)
            .expect("the loop runs");
        assert_eq!(
            forwarded.recv_timeout(Duration::from_secs(5)),
            Ok(true),
            "the later launch forwards to the first and ends"
        );
        assert_eq!(log.count(AppCall::Reopen), 1, "{:?}", log.calls());
        let mut activations = 0;
        while let Some(event) = watcher.poll_for_event().expect("the X server answers") {
            if let x11rb::protocol::Event::ClientMessage(message) = event
                && message.type_ == active_window
                && message.data.as_data32()[0] == SOURCE_APPLICATION
            {
                activations += 1;
            }
        }
        assert_eq!(
            activations, 1,
            "the reopened window is brought forward once, as it is mapped"
        );
    }

    /// An application that keeps one instance and shows no window (it holds
    /// one, hidden) is asked to reopen by a later launch, as a Dock click asks
    /// on macOS, and the host leaves the hidden window hidden: whether it is
    /// shown again is the application's to say. The later launch forwards
    /// and ends.
    fn a_later_launch_while_no_window_is_shown_asks_to_reopen() {
        const APP_ID: &str = "org.flui.X11WindowCases";
        if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
            eprintln!("skipped: no dbus-daemon for a session bus of the harness's own");
            return;
        }
        let HostStart::Started(platform) = CurrentHost::X11
            .start_single(NativeExecutor::own(), &app_id(APP_ID))
            .expect("the X11 platform")
        else {
            panic!("the first launch owns the application's name");
        };
        let held: Rc<RefCell<Option<Rc<dyn HostWindow>>>> = Rc::default();
        let hidden_at_reopen = Rc::new(std::cell::Cell::new(None));
        let (later, forwarded) = std::sync::mpsc::channel();
        let log = AppLog::new();
        let window_slot = Rc::clone(&held);
        let seen = Rc::clone(&hidden_at_reopen);
        log.react(move |call, owner| {
            if call == AppCall::Reopen {
                let window = window_slot.borrow();
                seen.set(window.as_ref().map(|window| !window.state().visible));
                let _ = owner.proxy().request_quit();
            }
        });
        let window_slot = Rc::clone(&held);
        log.bootstrap(move |owner| {
            quit_after(owner.proxy(), Duration::from_secs(10));
            let window = owner
                .open_window(
                    WindowOptions::default().visible(false),
                    RecordingWindowHandler::new(),
                )?
                .try_ready()?;
            *window_slot.borrow_mut() = Some(window);
            std::thread::spawn(move || {
                let started = CurrentHost::X11.start_single(NativeExecutor::own(), &app_id(APP_ID));
                let _ = later.send(matches!(started, Ok(HostStart::Forwarded)));
            });
            Ok(())
        });
        platform
            .run(Rc::clone(&log) as Rc<dyn AppHandler>)
            .expect("the loop runs");
        assert_eq!(
            forwarded.recv_timeout(Duration::from_secs(5)),
            Ok(true),
            "the later launch forwards to the first and ends"
        );
        assert_eq!(log.count(AppCall::Reopen), 1, "{:?}", log.calls());
        assert_eq!(
            hidden_at_reopen.get(),
            Some(true),
            "the hidden window stays hidden until the application shows it"
        );
    }

    /// The loop ends when its last window closes and the application lets
    /// it, and the application hears the quit last.
    fn the_loop_ends_with_its_last_window() {
        let platform = Box::new(X11Platform::try_new().expect("the X11 platform"));
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

    /// A turn deadline asked with no window open gives the owner a turn at
    /// its instant, within a second of it, and is asked a few times, not
    /// polled.
    fn a_turn_deadline_gives_the_owner_a_turn_with_no_window() {
        let platform = Box::new(X11Platform::try_new().expect("the X11 platform"));
        let log = AppLog::new();
        let due = Rc::new(std::cell::OnceCell::new());
        let answer = Rc::clone(&due);
        log.answer_turn_deadline_with(move || {
            Some(*answer.get_or_init(|| web_time::Instant::now() + Duration::from_millis(80)))
        });
        let turns = Rc::new(std::cell::RefCell::new(Vec::new()));
        let seen = Rc::clone(&turns);
        let quit_at = Rc::clone(&due);
        log.bootstrap(move |owner| {
            // The run ends anyway after three seconds, so a loop that never
            // gives the turn fails the assertion below instead of hanging.
            quit_after(owner.proxy(), Duration::from_secs(3));
            let quit = owner.proxy();
            owner
                .on_wake(Box::new(move || {
                    let now = web_time::Instant::now();
                    seen.borrow_mut().push(now);
                    if quit_at.get().is_some_and(|due| now >= *due) {
                        let _ = quit.request_quit();
                    }
                }))
                .expect("the owner takes a wake callback");
            Ok(())
        });
        platform
            .run(Rc::clone(&log) as Rc<dyn AppHandler>)
            .expect("the loop runs");
        let due = *due.get().expect("the deadline was asked");
        assert!(
            turns
                .borrow()
                .iter()
                .any(|turn| *turn >= due && *turn < due + Duration::from_secs(1)),
            "the deadline gave the owner its turn: {:?}",
            turns.borrow()
        );
        let questions = log.count(AppCall::TurnDeadline);
        assert!(questions < 20, "asked {questions} times: polled");
    }

    /// A wake deadline whose frame no handler ran is delivered once, not on
    /// every turn: the loop waits for the next event instead of spinning.
    fn a_wake_deadline_no_frame_serviced_is_delivered_once() {
        let platform = Box::new(X11Platform::try_new().expect("the X11 platform"));
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
                .open_window(WindowOptions::default().visible(false), opened)?
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

    /// An exit policy that opens a window and then panics is the run's first
    /// failure: the loop goes on for the window, a later handler (the wake
    /// deadline) panics and leaves the loop, and the policy's panic is the
    /// one that comes out of `run`.
    fn a_parked_exit_policy_panic_comes_out_ahead_of_a_later_one() {
        let platform = Box::new(X11Platform::try_new().expect("the X11 platform"));
        let log = AppLog::new();
        let kept = Rc::new(std::cell::RefCell::new(None));
        let failed = Rc::new(std::cell::Cell::new(false));
        log.bootstrap(|owner| {
            quit_after(owner.proxy(), Duration::from_secs(10));
            owner.request_exit_policy_reevaluation();
            Ok(())
        });
        log.react({
            let kept = Rc::clone(&kept);
            let failed = Rc::clone(&failed);
            move |call, owner| match call {
                AppCall::ExitPolicy => {
                    let window = owner
                        .open_window(
                            WindowOptions::default().visible(false),
                            RecordingWindowHandler::new(),
                        )
                        .expect("a window")
                        .try_ready()
                        .expect("at once");
                    *kept.borrow_mut() = Some(window);
                    failed.set(true);
                    panic!("the exit policy");
                }
                AppCall::WakeDeadline if failed.get() => panic!("a later handler"),
                _ => {}
            }
        });
        let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            platform.run(Rc::clone(&log) as Rc<dyn AppHandler>)
        }));
        drop(kept.borrow_mut().take());
        let payload = ran.expect_err("the run returned");
        assert_eq!(payload.downcast_ref::<&str>(), Some(&"the exit policy"));
        assert_eq!(log.count(AppCall::ExitPolicy), 1);
    }

    /// An application id parsed for the cases.
    fn app_id(id: &str) -> flui_foundation::AppId {
        flui_foundation::AppId::try_new(id).expect("a valid application id")
    }

    /// A window of a host started with an application id carries it as both
    /// halves of `WM_CLASS` and as `_GTK_APPLICATION_ID`, which a desktop
    /// entry named by the id matches; a host started without one carries the
    /// executable's stem and claims no id. Either way the window says it takes
    /// the input focus (`WM_HINTS`) and names its process.
    fn a_window_carries_the_application_id_it_was_started_with() {
        let stem = std::env::current_exe()
            .expect("the test's executable")
            .file_stem()
            .expect("a file stem")
            .to_string_lossy()
            .into_owned();
        let notes = app_id("com.example.Notes");
        let cases = [
            (
                X11Platform::try_new_for(&notes),
                "com.example.Notes",
                Some("com.example.Notes"),
            ),
            (X11Platform::try_new(), stem.as_str(), None),
        ];
        for (platform, class, claimed) in cases {
            let fixture = X11Fixture::open_on(
                platform.expect("the X11 platform"),
                WindowOptions::default().title("identity"),
                RecordingWindowHandler::new(),
            );
            let read = |name: &str| {
                let property = match name {
                    "WM_CLASS" => u32::from(AtomEnum::WM_CLASS),
                    "WM_HINTS" => u32::from(AtomEnum::WM_HINTS),
                    other => atom(&fixture.server, other),
                };
                fixture
                    .server
                    .get_property(false, fixture.xid, property, AtomEnum::ANY, 0, 64)
                    .expect("request")
                    .reply()
                    .expect("the property")
            };
            assert_eq!(
                read("WM_CLASS").value,
                format!("{class}\0{class}\0").into_bytes(),
                "{class}: the instance and the class"
            );
            let gtk = read("_GTK_APPLICATION_ID");
            assert_eq!(
                (gtk.type_ != u32::from(AtomEnum::NONE)).then_some(gtk.value),
                claimed.map(|id| id.as_bytes().to_vec()),
                "{class}: the claimed application id"
            );
            let hints: Vec<u32> = read("WM_HINTS").value32().expect("32-bit hints").collect();
            assert_eq!(
                hints.get(..2),
                Some(&[1, 1][..]),
                "{class}: InputHint set, input True"
            );
            let pid: Vec<u32> = read("_NET_WM_PID")
                .value32()
                .expect("a 32-bit pid")
                .collect();
            assert_eq!(pid, [std::process::id()], "{class}");
        }
    }

    /// A host started on a runtime its creator lends runs its IO there (ADR-0327 §3): the
    /// input-method bus's task is the lent runtime's, and it ends when the platform is dropped.
    fn the_host_runs_its_io_on_the_runtime_it_is_handed() {
        let lent = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("lent-io")
            .enable_all()
            .build()
            .expect("a lent runtime");
        let metrics = lent.handle().metrics();
        assert_eq!(metrics.num_alive_tasks(), 0, "the lent runtime starts idle");

        let platform = flui_native::CurrentHost::X11
            .start_with(
                flui_native::NativeExecutor::from_tokio(lent.handle().clone()),
                None,
            )
            .expect("the X11 platform");
        assert!(
            metrics.num_alive_tasks() >= 1,
            "the host's input-method bus runs on the lent runtime"
        );

        drop(platform);
        let deadline = Instant::now() + Duration::from_secs(5);
        while metrics.num_alive_tasks() > 0 {
            assert!(
                Instant::now() < deadline,
                "the host's tasks end when the platform is dropped"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// A window manager that is killed says nothing on the root window as it
    /// goes, and leaves what it wrote on the windows it managed: an iconic
    /// window keeps its `WM_STATE` and its `_NET_WM_STATE_HIDDEN` while the
    /// server maps it again. The window hears the window manager's own window
    /// go: it is in normal mode and shown, without a frame, honours nothing
    /// only a window manager carries out, and takes the focus from the
    /// server. A window manager that starts later is followed again.
    fn a_window_outlives_a_window_manager_that_was_killed() {
        use flui_platform::WindowCapability;
        const UNAVAILABLE: WindowCapability =
            WindowCapability::Absent(flui_platform::CapabilityAbsence::NOT_ON_THIS_DEVICE);

        let mut fixture = X11Fixture::open("orphaned");
        fixture
            .window
            .admit_and_apply(WindowCommand::SetMode(WindowMode::Minimized))
            .expect("a managed window minimizes");
        fixture.settle();
        let managed = fixture.window.state();
        assert_eq!(managed.mode, WindowMode::Minimized);
        assert_eq!(managed.capabilities.minimizes(), WindowCapability::Native);

        kill_window_manager();
        let started_again = StartedAgain;
        fixture.settle();
        let state = fixture.window.state();
        let wm_state = atom(&fixture.server, "WM_STATE");
        let left_on_the_window = fixture
            .server
            .get_property(false, fixture.xid, wm_state, wm_state, 0, 1)
            .expect("request")
            .reply()
            .expect("the property")
            .value32()
            .and_then(|mut words| words.next());
        assert_eq!(
            left_on_the_window,
            Some(3),
            "the dead window manager's iconic WM_STATE is still on the window"
        );
        assert_eq!(state.mode, WindowMode::Normal, "nobody holds it iconic");
        assert!(state.visible, "the server mapped it again");
        assert_eq!(
            state.capabilities.minimizes(),
            UNAVAILABLE,
            "nobody minimizes it now"
        );
        assert_eq!(
            state.outer.size(),
            state.content_size,
            "the dead window manager's frame is gone"
        );
        fixture
            .window
            .admit_and_apply(WindowCommand::Activate)
            .expect("a window takes the focus without a window manager");
        fixture.settle();
        let focus = fixture
            .server
            .get_input_focus()
            .expect("request")
            .reply()
            .expect("the focus")
            .focus;
        assert_eq!(focus, fixture.xid, "the server focused the window");
        assert!(fixture.window.state().focus.is_focused());

        drop(started_again);
        fixture.settle();
        assert_eq!(
            fixture.window.state().capabilities.minimizes(),
            WindowCapability::Native,
            "the window manager that started is followed"
        );
    }

    /// A handler that runs in the turn that hears the window manager die acts
    /// without it. The turn hears the window manager's own window go, then a
    /// close request whose handler applies a maximize admitted while the
    /// window manager ran, then reads the window and asks it for the focus,
    /// all before the turn's read of what the window manager supports: the
    /// maximize is superseded, the state it reads honours nothing only a
    /// window manager carries out, and the focus is taken from the server,
    /// not asked of the window manager that is gone.
    fn a_handler_in_the_turn_a_window_manager_dies_acts_without_it() {
        use flui_platform::WindowCapability;
        const UNAVAILABLE: WindowCapability =
            WindowCapability::Absent(flui_platform::CapabilityAbsence::NOT_ON_THIS_DEVICE);
        use x11rb::protocol::xproto::InputFocus;

        let handler = RecordingWindowHandler::new();
        let mut fixture = X11Fixture::open_with("bereaved", Rc::clone(&handler));
        let managed = fixture.window.state().capabilities;
        assert_eq!(managed.minimizes(), WindowCapability::Native);
        // Admitted while the window manager runs, applied in the turn that
        // hears it go, before anything reads the window.
        let maximize = WindowCommand::SetMode(WindowMode::Maximized)
            .admit(&managed)
            .expect("the window manager maximizes");
        let maximized = Rc::new(RefCell::new(None));
        let read = Rc::new(std::cell::Cell::new(None));
        let window = Rc::downgrade(&fixture.window);
        handler.react_to_close_request({
            let read = Rc::clone(&read);
            let maximized = Rc::clone(&maximized);
            let maximize = RefCell::new(Some(maximize));
            move || {
                if let Some(window) = window.upgrade() {
                    if let Some(maximize) = maximize.borrow_mut().take() {
                        *maximized.borrow_mut() = Some(window.apply(maximize));
                    }
                    let state = window.state();
                    read.set(Some(state.capabilities.minimizes()));
                    let activate = WindowCommand::Activate
                        .admit(&state.capabilities)
                        .expect("a window takes the focus");
                    window.apply(activate).expect("the window takes the focus");
                }
                CloseDecision::KeepOpen
            }
        });

        let check = check_window(&fixture.server);
        kill_window_manager();
        let started_again = StartedAgain;
        let server = &fixture.server;
        until("the window manager's window to go", || {
            server
                .get_window_attributes(check)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .is_none()
        });
        // Nobody has the focus: a window that has it next took it.
        server
            .set_input_focus(InputFocus::NONE, x11rb::NONE, x11rb::CURRENT_TIME)
            .expect("request");
        send_close(server, fixture.xid);
        // A round trip: the server has sent the window manager's end and the
        // close request before the loop turns, so one turn hears both.
        server
            .get_input_focus()
            .expect("request")
            .reply()
            .expect("the focus");

        fixture.settle();
        assert_eq!(handler.count(&HandlerCall::CloseRequested), 1);
        let maximized = maximized.borrow_mut().take();
        assert!(
            matches!(
                maximized,
                Some(Err(flui_platform::ApplyError::Superseded { .. }))
            ),
            "a maximize admitted under the window manager is superseded once it went: {maximized:?}"
        );
        assert_eq!(
            read.get(),
            Some(UNAVAILABLE),
            "the handler read the window as nobody manages it"
        );
        let focus = fixture
            .server
            .get_input_focus()
            .expect("request")
            .reply()
            .expect("the focus")
            .focus;
        assert_eq!(focus, fixture.xid, "the server focused the window");
        drop(started_again);
        fixture.settle();
    }

    /// The root's `_NET_SUPPORTING_WM_CHECK` is any client's to write. Naming
    /// the root, or a window of the application's own that names itself in
    /// turn, it names no window manager that speaks EWMH: the window honours
    /// none of what `_NET_SUPPORTED` lists (the window manager still runs, as
    /// the owner of `WM_S0`), and the platform still hears the root's property
    /// changes (selecting the window's end on either would have replaced the
    /// events the platform hears of it), so it follows the window manager's
    /// list again once the property names it back.
    fn a_check_window_that_is_no_window_managers_is_refused() {
        use flui_platform::WindowCapability;
        const UNAVAILABLE: WindowCapability =
            WindowCapability::Absent(flui_platform::CapabilityAbsence::NOT_ON_THIS_DEVICE);

        let mut fixture = X11Fixture::open("checked");
        let root = fixture.root();
        let check = atom(&fixture.server, "_NET_SUPPORTING_WM_CHECK");
        let window_manager = check_window(&fixture.server);
        let name = |server: &RustConnection, on: u32, named: u32| {
            server
                .change_property32(PropMode::REPLACE, on, check, AtomEnum::WINDOW, &[named])
                .expect("request");
            server.flush().expect("flush");
        };
        // (case, the window the root names)
        let table = [
            ("the root", root),
            ("a window of the application's own", fixture.xid),
        ];
        for (case, named) in table {
            if named != root {
                name(&fixture.server, named, named);
            }
            name(&fixture.server, root, named);
            fixture.settle();
            assert_eq!(
                fixture.window.state().capabilities.enters_fullscreen(),
                UNAVAILABLE,
                "{case}: no window manager that lists fullscreen is named"
            );
            name(&fixture.server, root, window_manager);
            fixture.settle();
            assert_eq!(
                fixture.window.state().capabilities.enters_fullscreen(),
                WindowCapability::Native,
                "{case}: the root's property changes are still heard"
            );
            if named != root {
                fixture
                    .server
                    .delete_property(named, check)
                    .expect("request");
            }
        }
    }

    /// A window manager that speaks only ICCCM owns the screen's `WM_S0`
    /// selection, announces itself with `MANAGER` and publishes no
    /// `_NET_SUPPORTING_WM_CHECK` (twm, older and some tiling ones). It runs
    /// all the same: a window it iconifies (unmapped, `WM_STATE` iconic,
    /// ICCCM §4.1.4) reads minimized and shown, and `Show` maps it again. Once
    /// it goes, its selection window with it, what it wrote says nothing.
    fn a_window_manager_without_ewmh_is_followed() {
        use flui_platform::WindowCapability;
        const UNAVAILABLE: WindowCapability =
            WindowCapability::Absent(flui_platform::CapabilityAbsence::NOT_ON_THIS_DEVICE);

        kill_window_manager();
        let started_again = StartedAgain;
        let mut fixture = X11Fixture::open("iconified without EWMH");
        let root = fixture.root();
        let selection = atom(&fixture.server, "WM_S0");
        let wm_state = atom(&fixture.server, "WM_STATE");
        let owner = |server: &RustConnection| {
            server
                .get_selection_owner(selection)
                .expect("request")
                .reply()
                .expect("the owner")
                .owner
        };
        until("the killed window manager's selection to go", || {
            owner(&fixture.server) == x11rb::NONE
        });
        fixture.settle();
        assert_eq!(
            fixture.window.state().capabilities.minimizes(),
            UNAVAILABLE,
            "nobody iconifies the window"
        );

        let stand_in = RustConnection::connect(None)
            .expect("the X server answers")
            .0;
        let selection_window = stand_in.generate_id().expect("an id");
        stand_in
            .create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                selection_window,
                root,
                -1,
                -1,
                1,
                1,
                0,
                WindowClass::INPUT_OUTPUT,
                x11rb::COPY_FROM_PARENT,
                &CreateWindowAux::new(),
            )
            .expect("request");
        stand_in
            .set_selection_owner(selection_window, selection, x11rb::CURRENT_TIME)
            .expect("request");
        stand_in
            .send_event(
                false,
                root,
                EventMask::STRUCTURE_NOTIFY,
                ClientMessageEvent::new(
                    32,
                    root,
                    atom(&stand_in, "MANAGER"),
                    [x11rb::CURRENT_TIME, selection, selection_window, 0, 0],
                ),
            )
            .expect("request");
        stand_in.flush().expect("flush");
        fixture.settle();
        assert_eq!(
            fixture.window.state().capabilities.minimizes(),
            WindowCapability::Native,
            "the window manager that took the selection iconifies the window"
        );

        stand_in
            .change_property32(
                PropMode::REPLACE,
                fixture.xid,
                wm_state,
                wm_state,
                &[3, x11rb::NONE],
            )
            .expect("request");
        stand_in.unmap_window(fixture.xid).expect("request");
        stand_in.flush().expect("flush");
        fixture.settle();
        let iconic = fixture.window.state();
        assert_eq!(iconic.mode, WindowMode::Minimized, "WM_STATE stands");
        assert!(iconic.visible, "an iconic window is shown");

        fixture
            .window
            .admit_and_apply(WindowCommand::Show)
            .expect("a window shows");
        fixture.settle();
        let map_state = fixture
            .server
            .get_window_attributes(fixture.xid)
            .expect("request")
            .reply()
            .expect("the window's attributes")
            .map_state;
        assert_eq!(map_state, MapState::VIEWABLE, "Show mapped the icon");

        drop(stand_in);
        fixture.settle();
        assert_eq!(
            fixture.window.state().mode,
            WindowMode::Normal,
            "what a window manager that went wrote says nothing"
        );
        drop(started_again);
        fixture.settle();
    }

    /// Hiding withdraws a window, iconic or not: a minimized window the
    /// application hides reads hidden as the command returns, while the
    /// window manager still says iconic of it, and stays hidden.
    fn a_minimized_window_that_is_hidden_reads_hidden() {
        let mut fixture = X11Fixture::open("minimized, then hidden");
        fixture
            .window
            .admit_and_apply(WindowCommand::SetMode(WindowMode::Minimized))
            .expect("a managed window minimizes");
        fixture.settle();
        let minimized = fixture.window.state();
        assert_eq!(minimized.mode, WindowMode::Minimized);
        assert!(minimized.visible, "a minimized window is shown");

        fixture
            .window
            .admit_and_apply(WindowCommand::Hide)
            .expect("a window hides");
        assert!(
            !fixture.window.state().visible,
            "hidden as the command returns"
        );
        fixture.settle();
        assert!(!fixture.window.state().visible, "and once the dust settled");

        fixture
            .window
            .admit_and_apply(WindowCommand::Show)
            .expect("a window shows");
        fixture.settle();
        assert!(fixture.window.state().visible, "shown again");
    }

    /// A command acts on the window as it is, not as its last snapshot had
    /// it. Another client has the window manager iconify the window and then
    /// asks the window to close, before the loop turns: the handler hears the
    /// close request in the turn that hears the window went iconic, before
    /// the window read itself again, and the command it answers with, which
    /// it was admitted before, still takes the window out of its icon.
    fn a_command_acts_on_the_window_as_it_is_now() {
        let handler = RecordingWindowHandler::new();
        // The pointer is nowhere near: a window it left as it went iconic
        // would read itself for the hover it lost.
        crate::x11_input::park_pointer();
        let mut fixture = X11Fixture::open_with("restored", Rc::clone(&handler));
        let capabilities = fixture.window.state().capabilities;
        let window = Rc::downgrade(&fixture.window);
        handler.react_to_close_request(move || {
            if let Some(window) = window.upgrade() {
                let restore = WindowCommand::SetMode(WindowMode::Normal)
                    .admit(&capabilities)
                    .expect("a managed window takes its mode");
                window.apply(restore).expect("the window leaves its icon");
            }
            CloseDecision::KeepOpen
        });

        let server = &fixture.server;
        let change_state = atom(server, "WM_CHANGE_STATE");
        let wm_state = atom(server, "WM_STATE");
        // ICCCM 4.1.4: a client message to the root window asks the window
        // manager for the iconic state (3).
        server
            .send_event(
                false,
                fixture.root(),
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                ClientMessageEvent::new(32, fixture.xid, change_state, [3, 0, 0, 0, 0]),
            )
            .expect("request");
        server.flush().expect("flush");
        until("the window manager to iconify the window", || {
            server
                .get_property(false, fixture.xid, wm_state, wm_state, 0, 1)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .and_then(|reply| reply.value32()?.next())
                == Some(3)
        });
        send_close(server, fixture.xid);
        // A round trip: the server has sent the window the request before
        // the loop turns.
        server
            .get_input_focus()
            .expect("request")
            .reply()
            .expect("the focus");

        fixture.settle();
        assert_eq!(handler.count(&HandlerCall::CloseRequested), 1);
        assert_eq!(
            fixture.window.state().mode,
            WindowMode::Normal,
            "the command took the window out of its icon"
        );
        let map_state = fixture
            .server
            .get_window_attributes(fixture.xid)
            .expect("request")
            .reply()
            .expect("the window's attributes")
            .map_state;
        assert_eq!(map_state, MapState::VIEWABLE, "and the server maps it");
    }

    /// A command tells the window's handler what it read of the window only
    /// once its own state is committed. In the turn that changed a minimized
    /// window's frame, another window's handler shows it, so the show reads
    /// the window anew; the window's own handler minimizes it again as it
    /// hears the state change. The window manager hears the show's map, then
    /// the handler's minimize, and the window ends minimized: a handler told
    /// in the middle of the show would have its minimize undone by the map
    /// that follows it.
    fn a_window_minimized_as_it_hears_its_show_stays_minimized() {
        let asker = RecordingWindowHandler::new();
        let mut fixture = X11Fixture::open_with("asker", Rc::clone(&asker));
        let handler = RecordingWindowHandler::new();
        let shown = fixture
            .platform
            .open_window(
                WindowOptions::default()
                    .title("shown")
                    .size(Size::new(300.0, 200.0)),
                Rc::clone(&handler) as Rc<dyn WindowHandler>,
            )
            .expect("an X11 window opens");
        let xid = xid_of(shown.as_ref());
        shown
            .admit_and_apply(WindowCommand::SetMode(WindowMode::Minimized))
            .expect("a managed window minimizes");
        fixture.settle();
        let minimized = shown.state();
        assert_eq!(minimized.mode, WindowMode::Minimized);

        // Admitted against what was read before the turn: a read in the
        // handler would tell the window's handler first.
        let capabilities = minimized.capabilities;
        let window = Rc::downgrade(&shown);
        asker.react_to_close_request(move || {
            if let Some(window) = window.upgrade() {
                let show = WindowCommand::Show
                    .admit(&capabilities)
                    .expect("a window shows");
                window.apply(show).expect("the window shows");
            }
            CloseDecision::KeepOpen
        });
        let armed = Rc::new(std::cell::Cell::new(true));
        let window = Rc::downgrade(&shown);
        handler.react_to_state_change({
            let armed = Rc::clone(&armed);
            move || {
                if armed.replace(false)
                    && let Some(window) = window.upgrade()
                {
                    let minimize = WindowCommand::SetMode(WindowMode::Minimized)
                        .admit(&window.state().capabilities)
                        .expect("a managed window minimizes");
                    window.apply(minimize).expect("the window minimizes");
                }
            }
        });

        let server = &fixture.server;
        // Any client may write the frame the window manager put around the
        // window: the window reads a new outer size.
        server
            .change_property32(
                PropMode::REPLACE,
                xid,
                atom(server, "_NET_FRAME_EXTENTS"),
                AtomEnum::CARDINAL,
                &[7, 7, 7, 7],
            )
            .expect("request");
        send_close(server, fixture.xid);
        // A round trip: one turn hears the frame's change and the request.
        server
            .get_input_focus()
            .expect("request")
            .reply()
            .expect("the focus");

        fixture.settle();
        assert_eq!(asker.count(&HandlerCall::CloseRequested), 1);
        assert!(!armed.get(), "the handler heard the state change");
        assert_eq!(
            shown.state().mode,
            WindowMode::Minimized,
            "the handler's minimize stands"
        );
        shown.close_for_cleanup();
        fixture.settle();
    }

    /// A stream of events that never runs dry leaves the frames their turns:
    /// here each change of the window's state makes the next (as a resize
    /// that answers a resize does), so there is always another event to
    /// read, and the frames the window asks for are delivered between them,
    /// not once the stream ends.
    fn a_stream_of_events_leaves_the_frames_their_turns() {
        /// How many state changes follow one another.
        const STREAM: u32 = 200;

        let handler = RecordingWindowHandler::new();
        let mut fixture = X11Fixture::open_with("feedback", Rc::clone(&handler));
        let other = Rc::new(
            RustConnection::connect(None)
                .expect("the X server answers")
                .0,
        );
        let extents = atom(&other, "_NET_FRAME_EXTENTS");
        let xid = fixture.xid;
        // The frame the window manager is said to have put around the
        // window: each value is another outer rectangle, another snapshot.
        let reframe = {
            let other = Rc::clone(&other);
            move |width: u32| {
                other
                    .change_property32(
                        PropMode::REPLACE,
                        xid,
                        extents,
                        AtomEnum::CARDINAL,
                        &[width; 4],
                    )
                    .expect("request");
                // Two round trips: the server has sent the window its
                // property's change before the loop looks for an event.
                for _ in 0..2 {
                    other
                        .get_input_focus()
                        .expect("request")
                        .reply()
                        .expect("the focus");
                }
            }
        };
        let left = Rc::new(std::cell::Cell::new(STREAM));
        let frames_meanwhile = Rc::new(std::cell::Cell::new(0_u32));
        handler.react_to_state_change({
            let (left, reframe) = (Rc::clone(&left), reframe.clone());
            move || {
                if left.get() > 0 {
                    left.set(left.get() - 1);
                    reframe(40 + left.get() % 2);
                }
            }
        });
        let redraw = fixture.window.redraw_requester();
        handler.react_to_frame({
            let (left, frames) = (Rc::clone(&left), Rc::clone(&frames_meanwhile));
            let redraw = Arc::clone(&redraw);
            move |_| {
                if left.get() > 0 {
                    frames.set(frames.get() + 1);
                    redraw.request_redraw();
                }
            }
        });

        redraw.request_redraw();
        reframe(39);
        let deadline = Instant::now() + Duration::from_secs(30);
        while left.get() > 0 {
            assert!(Instant::now() < deadline, "the stream never ran dry");
            run_for(&fixture.platform, Duration::from_millis(20)).expect("the loop turns");
        }
        assert!(
            frames_meanwhile.get() >= STREAM / 2,
            "{} frames were delivered while {STREAM} events streamed",
            frames_meanwhile.get()
        );
        fixture.settle();
    }
}
