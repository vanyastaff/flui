//! The probe against a real X server.
//!
//! CI runs the crate's tests under `xvfb-run`, which sets `DISPLAY`; the tests
//! skip without it. A bare server has no window manager, so the tests set the
//! properties a window manager would: they stand in for one, and the probe
//! reads and watches them as it would a real one's.

use std::time::{Duration, Instant};

use x11rb::{
    connection::Connection,
    protocol::xproto::{AtomEnum, CreateWindowAux, PropMode, WindowClass},
    rust_connection::RustConnection,
};

use super::*;

fn x_server() -> bool {
    let present = std::env::var_os("DISPLAY").is_some_and(|display| !display.is_empty());
    if !present {
        eprintln!("skipped: no X server (DISPLAY is not set); CI runs this under xvfb-run");
    }
    present
}

fn signal() -> Arc<OwnerSignal> {
    OwnerSignal::new(Arc::new(|| Ok(())))
}

/// A window on its own connection, which stands in for the window manager when
/// it sets the window's properties.
struct Fixture {
    connection: RustConnection,
    window: u32,
}

impl Fixture {
    fn new() -> Self {
        let (connection, screen) = RustConnection::connect(None).expect("an X server answers");
        let root = connection.setup().roots[screen].root;
        let window = connection.generate_id().expect("a window id");
        connection
            .create_window(
                0,
                window,
                root,
                0,
                0,
                16,
                16,
                0,
                WindowClass::INPUT_OUTPUT,
                0,
                &CreateWindowAux::new(),
            )
            .expect("request")
            .check()
            .expect("the server creates the window");
        Self { connection, window }
    }

    fn atom(&self, name: &str) -> u32 {
        self.connection
            .intern_atom(false, name.as_bytes())
            .expect("request")
            .reply()
            .expect("the atom")
            .atom
    }

    /// The names of the atoms in `_NET_WM_STATE`.
    fn state_names(&self) -> Vec<String> {
        let property = self
            .connection
            .get_property(
                false,
                self.window,
                self.atom("_NET_WM_STATE"),
                AtomEnum::ATOM,
                0,
                64,
            )
            .expect("request")
            .reply()
            .expect("the property");
        property
            .value32()
            .map(|atoms| {
                atoms
                    .map(|atom| {
                        let name = self
                            .connection
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

    /// Set `_NET_WM_STATE` to the named atoms.
    fn set_state(&self, names: &[&str]) {
        let atoms: Vec<u32> = names.iter().map(|name| self.atom(name)).collect();
        self.connection
            .change_property32(
                PropMode::REPLACE,
                self.window,
                self.atom("_NET_WM_STATE"),
                AtomEnum::ATOM,
                &atoms,
            )
            .expect("request")
            .check()
            .expect("the server sets the property");
    }

    /// Set `WM_STATE` the way a window manager does.
    fn set_wm_state(&self, state: u32) {
        let wm_state = self.atom("WM_STATE");
        self.connection
            .change_property32(
                PropMode::REPLACE,
                self.window,
                wm_state,
                wm_state,
                &[state, 0],
            )
            .expect("request")
            .check()
            .expect("the server sets the property");
    }
}

#[test]
fn x11_probe_reads_the_mode_from_the_properties() {
    if !x_server() {
        return;
    }
    let probe = X11WindowProbe::connect(&signal()).expect("the probe connects");
    let fixture = Fixture::new();
    let window = fixture.window;

    assert_eq!(
        probe.mode(window),
        Some(WindowMode::Normal),
        "a window with no properties"
    );

    fixture.set_wm_state(1);
    fixture.set_state(&[
        "_NET_WM_STATE_MAXIMIZED_HORZ",
        "_NET_WM_STATE_MAXIMIZED_VERT",
    ]);
    assert_eq!(probe.mode(window), Some(WindowMode::Maximized));

    fixture.set_state(&[
        "_NET_WM_STATE_MAXIMIZED_HORZ",
        "_NET_WM_STATE_MAXIMIZED_VERT",
        "_NET_WM_STATE_FULLSCREEN",
    ]);
    assert_eq!(probe.mode(window), Some(WindowMode::Fullscreen));

    fixture.set_state(&["_NET_WM_STATE_MAXIMIZED_HORZ"]);
    assert_eq!(probe.mode(window), Some(WindowMode::Normal), "tiled");

    fixture.set_wm_state(3);
    fixture.set_state(&["_NET_WM_STATE_HIDDEN"]);
    assert_eq!(probe.mode(window), Some(WindowMode::Minimized));

    fixture.set_wm_state(1);
    assert_eq!(
        probe.mode(window),
        Some(WindowMode::Normal),
        "hidden but mapped is on another desktop, not minimized"
    );

    // The window goes away: the server cannot say.
    fixture
        .connection
        .destroy_window(window)
        .expect("request")
        .check()
        .expect("the server destroys the window");
    assert_eq!(probe.mode(window), None);
}

#[test]
fn x11_probe_hears_of_a_change_to_a_watched_window() {
    if !x_server() {
        return;
    }
    let probe = X11WindowProbe::connect(&signal()).expect("the probe connects");
    let fixture = Fixture::new();
    let other = Fixture::new();
    probe.watch(fixture.window);

    let hears = |expected: &[u32]| {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut heard = Vec::new();
        while Instant::now() < deadline {
            heard.extend(probe.take_changed());
            if heard == expected {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        eprintln!("heard {heard:?}, expected {expected:?}");
        false
    };

    // An unwatched window's change is not heard; the watched one's is.
    other.set_state(&["_NET_WM_STATE_FULLSCREEN"]);
    fixture.set_state(&["_NET_WM_STATE_FULLSCREEN"]);
    assert!(hears(&[fixture.window]));

    // A property this model does not read is not a change of mode.
    let name = fixture.atom("WM_NAME");
    fixture
        .connection
        .change_property8(
            PropMode::REPLACE,
            fixture.window,
            name,
            AtomEnum::STRING,
            b"x",
        )
        .expect("request")
        .check()
        .expect("the server sets the property");
    fixture.set_wm_state(3);
    assert!(hears(&[fixture.window]), "WM_STATE is read");
}

#[test]
fn x11_probe_sets_the_mode_of_a_window_that_is_not_mapped() {
    if !x_server() {
        return;
    }
    let probe = X11WindowProbe::connect(&signal()).expect("the probe connects");
    let fixture = Fixture::new();
    assert_eq!(probe.mode(fixture.window), Some(WindowMode::Normal));

    // Minimized is no state to set.
    probe.set_mode_hint(fixture.window, WindowMode::Minimized);
    assert_eq!(fixture.state_names(), Vec::<String>::new());

    probe.set_mode_hint(fixture.window, WindowMode::Maximized);
    assert_eq!(probe.mode(fixture.window), Some(WindowMode::Maximized));

    // A later mode replaces the earlier one: the window opens in the last.
    probe.set_mode_hint(fixture.window, WindowMode::Fullscreen);
    assert_eq!(probe.mode(fixture.window), Some(WindowMode::Fullscreen));
    assert_eq!(fixture.state_names(), ["_NET_WM_STATE_FULLSCREEN"]);

    // Normal clears the mode and leaves what is not a mode alone.
    fixture.set_state(&["_NET_WM_STATE_ABOVE", "_NET_WM_STATE_FULLSCREEN"]);
    probe.set_mode_hint(fixture.window, WindowMode::Normal);
    assert_eq!(probe.mode(fixture.window), Some(WindowMode::Normal));
    assert_eq!(fixture.state_names(), ["_NET_WM_STATE_ABOVE"]);
}

#[test]
fn x11_probe_reads_the_frame_a_window_manager_reports() {
    if !x_server() {
        return;
    }
    let probe = X11WindowProbe::connect(&signal()).expect("the probe connects");
    let fixture = Fixture::new();

    // No window manager answers the request on a bare server.
    let asked = Instant::now();
    assert_eq!(
        probe.frame_extents(fixture.window, Duration::from_millis(30)),
        None
    );
    assert!(
        asked.elapsed() < Duration::from_secs(2),
        "the wait is bounded"
    );

    // One that answers a little late is waited for: left, right, top, bottom.
    let answering = Fixture::new();
    let window = answering.window;
    let property = answering.atom("_NET_FRAME_EXTENTS");
    let answer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        answering
            .connection
            .change_property32(
                PropMode::REPLACE,
                window,
                property,
                AtomEnum::CARDINAL,
                &[1, 2, 28, 3],
            )
            .expect("request")
            .check()
            .expect("the server sets the property");
        answering
    });
    assert_eq!(
        probe.frame_extents(window, Duration::from_secs(5)),
        Some(flui_foundation::geometry::Size::new(3, 31))
    );
    drop(answer.join().expect("the stand-in answered"));
}
