//! X11 window commands and snapshots on a real server (CI runs the suite under
//! `xvfb-run`).
//!
//! A bare server has no window manager, so a request that needs one (maximize,
//! minimize) is not carried out; the test stands in for the window manager by
//! setting the properties it would, and checks the snapshot follows what the
//! server holds. The commands a server carries out alone (size, position,
//! mapping) are checked as the commands themselves.
//!
//! The winit window runs on a real winit loop whose hooks report nothing
//! (`owner::winit::testing::QuietHost`), under a table of its own
//! ([`X11Windows`]): the window, its probe and the loop, without the host's.

use std::{
    sync::{Arc, atomic::Ordering},
    thread,
    time::{Duration, Instant},
};

use flui_foundation::geometry::{Point, Size};
use parking_lot::Mutex;
use winit::event_loop::EventLoop;
use x11rb::{
    connection::Connection as _,
    protocol::xproto::{
        AtomEnum, ConnectionExt as _, CreateWindowAux, MapState, PropMode, WindowClass,
    },
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

use crate::host::{Absent, Host, position};
use crate::host_window::HostWindow;
use crate::owner::winit::testing::{QuietHost, run_window_scenario};
use crate::owner::winit::{LoopWindow, WinitLoop};
use crate::platform::HostPlatform;
use crate::window::linux::x11::connect_window_manager;
use crate::window::winit::{WinitWindow, WinitWindows};
use flui_platform::{
    WindowCapabilities, WindowCommand, WindowGeometry, WindowMode, WindowOptions, WindowReveal,
    WindowState,
};

/// The table these tests run: the winit window and its X11 probe over the
/// winit loop, every position a window does not reach absent.
struct X11Windows;

impl Host for X11Windows {
    const NAME: &'static str = "winit window on X11 (tests)";

    type Loop = WinitLoop;
    type Window = WinitWindow;
    type Displays = WinitLoop;
    type Input = WinitLoop;
    type Text = Absent<position::Text>;
    type Accessibility = Absent<position::Accessibility>;
    type Clipboard = WinitLoop;
    type Drag = WinitLoop;
    type Haptics = Absent<position::Haptics>;
    type Preferences = WinitLoop;
    type Lifecycle = WinitLoop;
    type Chrome = Absent<position::Chrome>;
    type Storage = Absent<position::Storage>;
}

/// A platform of [`X11Windows`] whose windows attach the X11 probe.
fn platform() -> Arc<HostPlatform<X11Windows>> {
    Arc::new(HostPlatform::from_parts(
        |positions| WinitWindows::new(positions, connect_window_manager),
        |windows| WinitLoop::new::<QuietHost>(Arc::clone(windows) as _),
    ))
}

fn build_x11_event_loop() -> EventLoop<()> {
    use winit::platform::x11::EventLoopBuilderExtX11;
    let mut builder = EventLoop::builder();
    EventLoopBuilderExtX11::with_any_thread(&mut builder, true);
    EventLoopBuilderExtX11::with_x11(&mut builder);
    builder.build().expect("build a winit event loop on X11")
}

/// Poll (bounded, 5 s) until `check` holds of the window's snapshot.
fn until_state(window: &Arc<dyn HostWindow>, what: &str, check: impl Fn(&WindowState) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let state = window.state();
        if check(&state) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}; last snapshot {state:?}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

/// Poll (bounded, 5 s) until the visibility reports are exactly
/// `expected`, the last of which `is_visible` answers. A report may follow
/// a publication on another thread, hence the wait.
fn until_reports(
    window: &Arc<dyn HostWindow>,
    reports: &Mutex<Vec<bool>>,
    expected: &[bool],
    what: &str,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while reports.lock().as_slice() != expected {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}; reports {:?}",
            reports.lock()
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(Some(&window.is_visible()), expected.last(), "{what}");
}

fn map_state(connection: &RustConnection, id: u32) -> MapState {
    connection
        .get_window_attributes(id)
        .expect("request")
        .reply()
        .expect("the server knows the window")
        .map_state
}

fn until_map_state(connection: &RustConnection, id: u32, expected: MapState) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while map_state(connection, id) != expected {
        assert!(
            Instant::now() < deadline,
            "the window did not reach {expected:?}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

/// Set `_NET_WM_STATE` and `WM_STATE` the way a window manager does.
fn set_window_manager_state(connection: &RustConnection, id: u32, wm_state: u32, names: &[&str]) {
    let atom = |name: &str| {
        connection
            .intern_atom(false, name.as_bytes())
            .expect("request")
            .reply()
            .expect("the atom")
            .atom
    };
    let atoms: Vec<u32> = names.iter().map(|name| atom(name)).collect();
    let net_wm_state = atom("_NET_WM_STATE");
    connection
        .change_property32(PropMode::REPLACE, id, net_wm_state, AtomEnum::ATOM, &atoms)
        .expect("request");
    let wm_state_atom = atom("WM_STATE");
    connection
        .change_property32(
            PropMode::REPLACE,
            id,
            wm_state_atom,
            wm_state_atom,
            &[wm_state, 0],
        )
        .expect("request")
        .check()
        .expect("the server sets the properties");
}

/// Open a window with `opening` on a real X server in a loop of its own and
/// run `scenario` on a worker thread against it, with the window, its X11
/// id and a connection to the server.
fn run_x11_scenario(
    opening: WindowOptions,
    scenario: impl FnOnce(Arc<dyn HostWindow>, u32, &RustConnection) + Send + 'static,
) {
    run_x11_window(platform(), opening, move |window, server| {
        let id = window
            .as_any()
            .downcast_ref::<WinitWindow>()
            .and_then(LoopWindow::x11_id)
            .expect("an X11 window has an id once the probe is attached");
        scenario(window, id, server);
    });
}
/// [`run_x11_scenario`] on a platform the caller has prepared, for a
/// scenario that needs no X11 id.
fn run_x11_window(
    host: Arc<HostPlatform<X11Windows>>,
    opening: WindowOptions,
    scenario: impl FnOnce(Arc<dyn HostWindow>, &RustConnection) + Send + 'static,
) {
    run_window_scenario(
        host.shared_owner(),
        host,
        build_x11_event_loop(),
        opening,
        move |window| {
            let server = RustConnection::connect(None)
                .expect("an X server answers")
                .0;
            scenario(window, &server);
        },
    );
}

#[test]
fn x11_window_takes_commands_and_publishes_what_the_server_holds() {
    // The suite's tests share one X server (`xvfb-run`'s 1280×1024 screen)
    // and open their windows within 800×600 of the origin, so this one
    // stays beyond that, where none of theirs can cover it and make its
    // visibility reports another test's.
    let opening = WindowOptions::default()
        .with_title("x11-window")
        .with_size(Size::new(320.0, 240.0))
        .with_position(Some(Point::new(840.0, 640.0)))
        .with_reveal(WindowReveal::AfterFirstFrame);
    run_x11_scenario(opening, |window, id, server| {
        // Created hidden, to be shown with the first frame; the window
        // already counts as visible, which is the intent.
        assert_eq!(map_state(server, id), MapState::UNMAPPED);
        assert!(window.state().visible);
        assert_eq!(window.capabilities(), WindowCapabilities::ALL);
        let changes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = Arc::clone(&changes);
        window.on_state_changed(Box::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));
        // Every visibility report, after the one the window opened with:
        // an unmapped window has had no visibility event yet.
        // Each report also checks that the snapshot already says the same.
        let visibility = Arc::new(Mutex::new(vec![true]));
        let reported = Arc::clone(&visibility);
        let observed = Arc::downgrade(&window);
        window.on_visibility_status_change(Box::new(move |visible| {
            let published = observed.upgrade().map(|window| window.state().visible);
            assert_eq!(
                published,
                Some(visible),
                "the snapshot runs behind a report"
            );
            reported.lock().push(visible);
        }));

        window.reveal_after_first_frame();
        until_map_state(server, id, MapState::VIEWABLE);

        // The position it opened at, and the commands a server carries
        // out without a window manager.
        until_state(&window, "the opening position", |state| {
            state.outer.position == Some(Point::new(840.0, 640.0))
        });
        window
            .apply(WindowCommand::SetSize(Size::new(400.0, 300.0)))
            .expect("a desktop window resizes");
        until_state(&window, "the new size", |state| {
            state.content_size == Size::new(400.0, 300.0)
        });
        window
            .apply(WindowCommand::SetPosition(Point::new(860.0, 680.0)))
            .expect("a desktop window moves");
        until_state(&window, "the new position", |state| {
            state.outer.position == Some(Point::new(860.0, 680.0))
        });
        window
            .apply(WindowCommand::SetTitle("renamed".into()))
            .expect("a title is set");
        assert_eq!(window.state().title.as_deref(), Some("renamed"));

        // Hiding and showing are reported, so a late `Occluded` from
        // either cannot leave the last report behind what the window
        // answers.
        window
            .apply(WindowCommand::Hide)
            .expect("a desktop window hides");
        until_map_state(server, id, MapState::UNMAPPED);
        assert!(!window.state().visible);
        until_reports(&window, &visibility, &[true, false], "a report of the hide");
        window
            .apply(WindowCommand::Show)
            .expect("a desktop window shows");
        until_map_state(server, id, MapState::VIEWABLE);
        assert!(window.state().visible);
        until_reports(
            &window,
            &visibility,
            &[true, false, true],
            "a report of the show",
        );

        // The window manager minimizes the window: winit reports no
        // event, the probe hears of it, and the snapshot follows with
        // a notification. X sends no visibility event for it either, so
        // the window reports itself hidden from the mode.
        let before = changes.load(Ordering::SeqCst);
        set_window_manager_state(server, id, 3, &["_NET_WM_STATE_HIDDEN"]);
        until_state(&window, "the minimized mode", |state| {
            state.mode == WindowMode::Minimized
        });
        assert!(changes.load(Ordering::SeqCst) > before, "a notification");
        assert!(!window.state().visible, "a minimized window is not visible");
        // The report follows the publication on whichever thread made it.
        until_reports(
            &window,
            &visibility,
            &[true, false, true, false],
            "a report of the minimizing",
        );
        assert_eq!(
            window.state().normal_geometry.map(|geometry| geometry.size),
            Some(Size::new(400.0, 300.0)),
            "a minimized window still names the rectangle it returns to"
        );

        // It returns, maximized.
        set_window_manager_state(
            server,
            id,
            1,
            &[
                "_NET_WM_STATE_MAXIMIZED_HORZ",
                "_NET_WM_STATE_MAXIMIZED_VERT",
            ],
        );
        until_state(&window, "the maximized mode", |state| {
            state.mode == WindowMode::Maximized
        });
        assert_eq!(
            window.state().normal_geometry.map(|geometry| geometry.size),
            Some(Size::new(400.0, 300.0)),
            "it returns to the size it had in normal mode"
        );
        assert!(window.state().visible, "visible again");
        until_reports(
            &window,
            &visibility,
            &[true, false, true, false, true],
            "a report of the return",
        );

        // A window of the test's own covers it: winit reports it occluded.
        let cover = cover_window(server, (800, 600), (480, 424));
        until_reports(
            &window,
            &visibility,
            &[true, false, true, false, true, false],
            "a report of the cover",
        );
        // A show of a window that stays mapped keeps its occlusion.
        window.show().expect("a desktop window shows");
        assert!(!window.state().visible, "still covered");
        assert_eq!(*visibility.lock(), [true, false, true, false, true, false]);
        server.destroy_window(cover).expect("request");
        server.flush().expect("flush");
        until_reports(
            &window,
            &visibility,
            &[true, false, true, false, true, false, true],
            "a report of the uncovering",
        );
    });
}

/// Map a plain window of `connection`'s own at `at` with `size`, above the
/// others; a bare server has no window manager to stack it elsewhere.
fn cover_window(connection: &RustConnection, at: (i16, i16), size: (u16, u16)) -> u32 {
    let root = connection.setup().roots[0].root;
    let cover = connection.generate_id().expect("an id");
    connection
        .create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            cover,
            root,
            at.0,
            at.1,
            size.0,
            size.1,
            0,
            WindowClass::INPUT_OUTPUT,
            x11rb::COPY_FROM_PARENT,
            &CreateWindowAux::new(),
        )
        .expect("request");
    connection.map_window(cover).expect("request");
    connection.flush().expect("flush");
    cover
}

/// A window that opens maximized waits unmapped for its first frame, and
/// a window manager reads "maximized" from the window's properties when it
/// maps it, so the property is there before the map, and the snapshot
/// already shows the mode and the rectangle the window returns to.
#[test]
fn x11_window_opens_maximized_with_the_property_set_before_it_is_mapped() {
    let returns_to = WindowGeometry::new(Some(Point::new(40.0, 60.0)), Size::new(500.0, 400.0));
    let opening = WindowOptions::default()
        .with_size(Size::new(320.0, 240.0))
        .with_mode(WindowMode::Maximized)
        .with_normal_geometry(Some(returns_to))
        .with_reveal(WindowReveal::AfterFirstFrame);
    run_x11_scenario(opening, move |window, id, server| {
        assert_eq!(map_state(server, id), MapState::UNMAPPED);
        let atom = |name: &str| {
            server
                .intern_atom(false, name.as_bytes())
                .expect("request")
                .reply()
                .expect("the atom")
                .atom
        };
        let property = server
            .get_property(false, id, atom("_NET_WM_STATE"), AtomEnum::ATOM, 0, 64)
            .expect("request")
            .reply()
            .expect("the property");
        let atoms: Vec<u32> = property.value32().expect("atoms").collect();
        assert!(
            atoms.contains(&atom("_NET_WM_STATE_MAXIMIZED_HORZ"))
                && atoms.contains(&atom("_NET_WM_STATE_MAXIMIZED_VERT")),
            "the window manager reads the mode from the property: {atoms:?}"
        );
        let state = window.state();
        assert_eq!(state.mode, WindowMode::Maximized);
        assert_eq!(state.normal_geometry, Some(returns_to));
        assert!(state.visible, "intended to be shown");

        window.reveal_after_first_frame();
        until_map_state(server, id, MapState::VIEWABLE);
    });
}

/// A window opened in a mode, at once rather than after its first frame,
/// is still created unmapped and mapped only once the mode is in its
/// properties. On a bare server nothing but the window itself could have
/// set `_NET_WM_STATE`, so finding the mode there shows it was set.
fn opens_in_mode_with_the_property_set(mode: WindowMode, state_atoms: &'static [&'static str]) {
    let opening = WindowOptions::default()
        .with_size(Size::new(320.0, 240.0))
        .with_mode(mode)
        .with_reveal(WindowReveal::AtOpen);
    run_x11_scenario(opening, move |window, id, server| {
        until_map_state(server, id, MapState::VIEWABLE);
        let atom = |name: &str| {
            server
                .intern_atom(false, name.as_bytes())
                .expect("request")
                .reply()
                .expect("the atom")
                .atom
        };
        let property = server
            .get_property(false, id, atom("_NET_WM_STATE"), AtomEnum::ATOM, 0, 64)
            .expect("request")
            .reply()
            .expect("the property");
        let atoms: Vec<u32> = property.value32().expect("atoms").collect();
        for name in state_atoms {
            assert!(atoms.contains(&atom(name)), "{name} is set: {atoms:?}");
        }
        assert_eq!(window.state().mode, mode);
    });
}

#[test]
fn x11_window_opened_maximized_at_once_is_mapped_with_the_mode_set() {
    opens_in_mode_with_the_property_set(
        WindowMode::Maximized,
        &[
            "_NET_WM_STATE_MAXIMIZED_HORZ",
            "_NET_WM_STATE_MAXIMIZED_VERT",
        ],
    );
}

#[test]
fn x11_window_opened_fullscreen_at_once_is_mapped_with_the_mode_set() {
    opens_in_mode_with_the_property_set(WindowMode::Fullscreen, &["_NET_WM_STATE_FULLSCREEN"]);
}

/// The names of the atoms in the window's `_NET_WM_STATE`.
fn state_names(server: &RustConnection, id: u32) -> Vec<String> {
    let atom = |name: &str| {
        server
            .intern_atom(false, name.as_bytes())
            .expect("request")
            .reply()
            .expect("the atom")
            .atom
    };
    let property = server
        .get_property(false, id, atom("_NET_WM_STATE"), AtomEnum::ATOM, 0, 64)
        .expect("request")
        .reply()
        .expect("the property");
    property
        .value32()
        .map(|atoms| {
            atoms
                .map(|atom| {
                    let name = server
                        .get_atom_name(atom)
                        .expect("request")
                        .reply()
                        .expect("the name")
                        .name;
                    String::from_utf8(name).expect("an ASCII atom name")
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A window the application opens hidden, in a mode, is unmapped with the
/// mode in its properties, and `Show` maps it from there.
#[test]
fn x11_window_opened_hidden_in_a_mode_keeps_the_mode_until_it_is_shown() {
    let opening = WindowOptions::default()
        .with_size(Size::new(320.0, 240.0))
        .with_mode(WindowMode::Maximized)
        .with_visible(false);
    run_x11_scenario(opening, |window, id, server| {
        assert_eq!(map_state(server, id), MapState::UNMAPPED);
        let names = state_names(server, id);
        assert!(
            names.contains(&"_NET_WM_STATE_MAXIMIZED_HORZ".to_owned())
                && names.contains(&"_NET_WM_STATE_MAXIMIZED_VERT".to_owned()),
            "{names:?}"
        );
        assert!(!window.state().visible);

        window
            .apply(WindowCommand::Show)
            .expect("a desktop window shows");
        until_map_state(server, id, MapState::VIEWABLE);
    });
}

/// A mode asked for while the window waits unmapped for its first frame
/// replaces the one it opened in, in the properties the window manager
/// reads when it maps it.
#[test]
fn x11_window_asked_for_a_mode_before_its_first_frame_opens_in_that_mode() {
    let opening = WindowOptions::default()
        .with_size(Size::new(320.0, 240.0))
        .with_mode(WindowMode::Maximized)
        .with_reveal(WindowReveal::AfterFirstFrame);
    run_x11_scenario(opening, |window, id, server| {
        window
            .apply(WindowCommand::SetMode(WindowMode::Fullscreen))
            .expect("a desktop window enters fullscreen");
        assert_eq!(state_names(server, id), ["_NET_WM_STATE_FULLSCREEN"]);

        window
            .apply(WindowCommand::SetMode(WindowMode::Normal))
            .expect("a desktop window returns to normal");
        assert_eq!(state_names(server, id), Vec::<String>::new());

        // Minimizing it leaves no mode to be mapped in; the window is
        // minimized once it is mapped.
        window
            .apply(WindowCommand::SetMode(WindowMode::Fullscreen))
            .expect("a desktop window enters fullscreen");
        assert_eq!(state_names(server, id), ["_NET_WM_STATE_FULLSCREEN"]);
        window
            .apply(WindowCommand::SetMode(WindowMode::Minimized))
            .expect("a minimize is requested");
        assert_eq!(state_names(server, id), Vec::<String>::new());

        window.reveal_after_first_frame();
        until_map_state(server, id, MapState::VIEWABLE);
    });
}

/// A window whose probe could not be made does not claim to see a minimize
/// that nothing would tell it of: winit sends no event for one on X11.
#[test]
fn x11_window_without_a_probe_does_not_claim_to_observe_minimizing() {
    let host = platform();
    // The probe's connections failed: the platform remembers that.
    assert!(
        host.window_shared().window_manager.set(None).is_ok(),
        "no probe was made yet"
    );
    run_x11_window(host, WindowOptions::default(), |window, _server| {
        let capabilities = window.capabilities();
        assert!(!capabilities.observes_minimized);
        // Nothing else is taken away.
        let mut expected = WindowCapabilities::ALL;
        expected.observes_minimized = false;
        assert_eq!(capabilities, expected);
        // Asking is still allowed: the compositor or window manager acts.
        window
            .apply(WindowCommand::SetMode(WindowMode::Minimized))
            .expect("a minimize is requested");
    });
}
