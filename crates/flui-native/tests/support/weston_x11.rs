//! A Weston of the harness's own on its X11 backend, on an Xvfb of its own:
//! the X server gives Weston a seat, whose input the harness drives through
//! XTest, and shows what Weston composited, which the harness reads back.
//!
//! The X server is reached by its name, never through `DISPLAY`, so the
//! Wayland host is the only one the process sees. Shared by the harnesses
//! that start one (`wayland_input`, `wayland_frame`).

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{ConnectionExt as _, InputFocus, MapState};
use x11rb::rust_connection::RustConnection;

/// Xvfb, and a Weston on it whose socket `WAYLAND_DISPLAY` names.
pub(crate) struct Session {
    server: Child,
    weston: Child,
    display: String,
    output: (u16, u16),
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

/// The X connection a harness drives the input with and reads the screen
/// through, with Weston's output window on it.
pub(crate) struct Screen {
    pub(crate) connection: RustConnection,
    pub(crate) root: u32,
    /// The origin of Weston's output window on the root window.
    pub(crate) origin: (i16, i16),
}

impl Session {
    /// Start Xvfb and a Weston with `shell` (`kiosk-shell.so`,
    /// `desktop-shell.so`) and one output of `output` pixels on socket
    /// `socket`, and name it for the process; `None` where Xvfb or Weston is
    /// missing, which fails where `FLUI_REQUIRE_WAYLAND` is set.
    ///
    /// Call it before any thread starts: it sets the environment.
    pub(crate) fn start(shell: &str, output: (u16, u16), socket: &str) -> Option<Self> {
        if !available("Xvfb") || !available("weston") {
            assert!(
                std::env::var_os("FLUI_REQUIRE_WAYLAND").is_none(),
                "FLUI_REQUIRE_WAYLAND is set and Xvfb or Weston is missing"
            );
            return None;
        }
        let (server, display) = x_server();
        let runtime = std::env::temp_dir().join(format!("{socket}-{}", std::process::id()));
        std::fs::create_dir_all(&runtime).expect("a runtime directory");
        std::fs::set_permissions(
            &runtime,
            <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o700),
        )
        .expect("the runtime directory is private");
        let weston = Command::new("weston")
            .args([
                "--backend=x11",
                "--renderer=pixman",
                &format!("--shell={shell}"),
                &format!("--socket={socket}"),
                &format!("--width={}", output.0),
                &format!("--height={}", output.1),
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
        #[expect(
            unsafe_code,
            reason = "the environment is set before any thread exists"
        )]
        unsafe {
            std::env::set_var("XDG_RUNTIME_DIR", &runtime);
            std::env::set_var("WAYLAND_DISPLAY", socket);
            std::env::remove_var("DISPLAY");
        }
        Some(Self {
            server,
            weston,
            display,
            output,
            _runtime: TempDir(runtime),
        })
    }

    /// The X connection the harness drives the input with, and Weston's
    /// output window on it, which is given the X focus: Weston's X11 backend
    /// gives its seat's keyboard to a client while its window has it.
    pub(crate) fn screen(&self) -> Screen {
        let (connection, screen) =
            RustConnection::connect(Some(&self.display)).expect("the X server answers");
        let root = connection.setup().roots[screen].root;
        let mut found = None;
        until("Weston's output window", || {
            let tree = connection
                .query_tree(root)
                .expect("request")
                .reply()
                .expect("reply");
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
                mapped
                    && geometry
                        .is_some_and(|geometry| (geometry.width, geometry.height) == self.output)
            });
            found.is_some()
        });
        let window = found.expect("found above");
        let origin = connection
            .translate_coordinates(window, root, 0, 0)
            .expect("request")
            .reply()
            .expect("reply");
        connection
            .set_input_focus(InputFocus::PARENT, window, x11rb::CURRENT_TIME)
            .expect("request");
        connection.flush().expect("flush");
        Screen {
            connection,
            root,
            origin: (origin.dst_x, origin.dst_y),
        }
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
pub(crate) fn until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}
