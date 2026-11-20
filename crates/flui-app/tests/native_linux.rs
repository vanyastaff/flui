//! An application on Linux runs on the native host of its session: the
//! native Wayland host under a Wayland compositor and the native X11 host on
//! an X server (ADR-0151 §11).
//!
//! Each case starts a display server of its own (a headless Weston, an Xvfb)
//! and runs the application in a child process of this binary whose session
//! names only that server. The child opens its main window, quits once the
//! window paints its first frame, and reports the host `current_platform`
//! started, which it reads from that function's own diagnostic. The case
//! checks the child exited cleanly on the expected host.
//!
//! A missing Weston or Xvfb skips the case, and fails it where
//! `FLUI_REQUIRE_WAYLAND` is set, as for the native Wayland suites.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use flui_app::app::{AppConfig, AppHandle, Application};
use flui_foundation::Leaf;
use flui_foundation::geometry::Size;
use flui_rendering::{
    BoxLayoutContext, PaintContext, parent_data::BoxParentData, traits::RenderBox,
};
use flui_view::prelude::*;

/// Set in the child's environment: the child case runs the application only
/// under it.
const CHILD: &str = "FLUI_NATIVE_LINUX_CHILD";

/// The child prints the host after this, on a line libtest may have begun
/// with the case's name.
const HOST_LINE: &str = "native-linux host: ";

/// The child's case, as the harness names it.
const CHILD_CASE: &str = "native_linux_child";

/// How long the child may take to start, paint and quit.
const CHILD_BUDGET: Duration = Duration::from_secs(90);

/// Set in the child's environment to a number of milliseconds: the child's
/// window animates without end, and the application asks to quit that long
/// after it is ready, from another thread.
const QUIT_AFTER: &str = "FLUI_NATIVE_LINUX_QUIT_AFTER_MS";

/// Every display server variable a session can name; the child's session
/// names only the server its case started.
const SESSION_VARIABLES: [&str; 4] = [
    "WAYLAND_DISPLAY",
    "WAYLAND_SOCKET",
    "DISPLAY",
    "FLUI_HEADLESS",
];

/// The application on a Wayland session runs on the native Wayland host.
pub(crate) fn an_app_runs_on_the_native_wayland_host() {
    let Some(compositor) = Weston::start() else {
        return;
    };
    let child = run_child(
        &[
            ("WAYLAND_DISPLAY", compositor.socket.as_str()),
            ("XDG_RUNTIME_DIR", path_text(compositor.runtime.path())),
        ],
        CHILD_BUDGET,
    );
    assert_host(&child, "Wayland");
}

/// An application whose window its compositor never draws (it answers no
/// frame callback, as for a minimized window or one on a hidden workspace)
/// still turns its owner thread. Its window animates, so the host asks it for
/// a frame once per callback grace, each a present on a swapchain whose last
/// frame was never shown; none of them waits for the compositor, so the quit
/// the application asks for four seconds after it is ready is carried out.
/// The child draws through Vulkan, whose `Fifo` present on Wayland waits for
/// that frame callback.
///
/// The window is drawn on the raster thread, where the Wayland host places
/// its executor (ADR-0305 §10), and presents one frame per frame callback:
/// every buffer the compositor is handed comes with a frame callback of its
/// own. The thread presents after the wake that queued the frame returned, so
/// until then no callback holds the window's next frame back; taken then,
/// that frame is presented right behind the first, with no callback, and the
/// compositor is handed two buffers for each one it would show.
pub(crate) fn an_app_whose_window_is_never_drawn_still_quits() {
    use flui_native::testing::wayland_compositor::{Offer, ScriptedCompositor};

    // The compositor offers `wl_shm` alone, which a software Vulkan driver
    // presents to and a hardware one may not: the child is pinned to Mesa's
    // lavapipe, and a machine without it fails here, naming it.
    let lavapipe = [
        "/usr/share/vulkan/icd.d/lvp_icd.json",
        "/usr/share/vulkan/icd.d/lvp_icd.x86_64.json",
    ]
    .into_iter()
    .find(|path| Path::new(path).exists())
    .expect(
        "this case draws through Mesa's lavapipe Vulkan driver (mesa-vulkan-drivers), \
         which is not installed",
    );
    let compositor = ScriptedCompositor::start(Offer::default().withhold_frame_callbacks())
        .expect("the compositor starts");
    let child = run_child(
        &[
            ("WAYLAND_DISPLAY", path_text(compositor.socket())),
            (QUIT_AFTER, "4000"),
            ("WGPU_BACKEND", "vulkan"),
            ("VK_DRIVER_FILES", lavapipe),
            ("VK_ICD_FILENAMES", lavapipe),
        ],
        Duration::from_secs(45),
    );
    assert_host(&child, "Wayland");
    let heard = compositor.heard();
    assert!(
        heard.frames >= 3 && heard.withheld >= heard.frames,
        "the window drew frame after frame, none of them shown and each with its frame \
         callback: {heard:?}"
    );
}

/// The application on an X11 session runs on the native X11 host.
pub(crate) fn an_app_runs_on_the_native_x11_host() {
    let Some(server) = Xvfb::start() else {
        return;
    };
    let child = run_child(&[("DISPLAY", server.display.as_str())], CHILD_BUDGET);
    assert_host(&child, "X11");
}

/// The child's side: run the application with a window that quits once it
/// paints, or with an animating one that quits after [`QUIT_AFTER`], and
/// print the host `current_platform` started. It does nothing unless
/// [`CHILD`] is set.
pub(crate) fn child() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    let quit_after = std::env::var(QUIT_AFTER)
        .ok()
        .and_then(|millis| millis.parse::<u64>().ok())
        .map(Duration::from_millis);
    let (result, log) = flui_testing::log_capture::capture(|| {
        let application = Application::new()
            .config(AppConfig::new().title("native linux"))
            .on_ready(move |handle| {
                drop(
                    handle
                        .request_show_main_window()
                        .expect("the ready application shows its main window"),
                );
                if let Some(delay) = quit_after {
                    let handle = handle.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(delay);
                        handle
                            .request_quit()
                            .expect("the application accepts the quit while it runs");
                    });
                }
            });
        if quit_after.is_some() {
            application.run_resident(|_: &AppHandle| flui_widgets::ActivityIndicator::new())
        } else {
            application.run_resident(|handle: &AppHandle| QuitOnPaint {
                handle: handle.clone(),
                asked: Arc::new(AtomicBool::new(false)),
            })
        }
    });
    result.expect("the application runs, paints and quits");
    let host = log
        .records()
        .iter()
        .find_map(|record| record.field("host"))
        .unwrap_or_else(|| panic!("current_platform reported no host:\n{log}"));
    println!("{HOST_LINE}{host}");
}

/// Run this binary's [`CHILD_CASE`] in a session that names only
/// `session`, bounded by `budget`.
fn run_child(session: &[(&str, &str)], budget: Duration) -> Output {
    let mut command = Command::new(std::env::current_exe().expect("the test binary's path"));
    command
        .args([CHILD_CASE, "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for variable in SESSION_VARIABLES {
        command.env_remove(variable);
    }
    command.envs(session.iter().copied());
    let mut child = command.spawn().expect("the test binary runs again");
    let deadline = Instant::now() + budget;
    while child.try_wait().expect("the child's status").is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            let output = child.wait_with_output().expect("the killed child's output");
            panic!(
                "the application did not quit within {budget:?}:\n{}",
                describe(&output)
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    child.wait_with_output().expect("the child's output")
}

/// The child exited cleanly and reported `expected` as its host.
fn assert_host(child: &Output, expected: &str) {
    assert!(
        child.status.success(),
        "the application failed:\n{}",
        describe(child)
    );
    let stdout = String::from_utf8_lossy(&child.stdout);
    let host = stdout
        .lines()
        .find_map(|line| line.split_once(HOST_LINE).map(|(_, host)| host.trim()))
        .unwrap_or_else(|| panic!("the child reported no host:\n{}", describe(child)));
    assert_eq!(host, expected, "the application ran on the {host} host");
}

fn describe(output: &Output) -> String {
    format!(
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn path_text(path: &Path) -> &str {
    path.to_str()
        .expect("a temporary directory's path is UTF-8")
}

/// Whether `program` runs; a missing one fails where `FLUI_REQUIRE_WAYLAND`
/// is set.
fn available(program: &str) -> bool {
    let found = Command::new(program)
        .arg("--help")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok();
    assert!(
        found || std::env::var_os("FLUI_REQUIRE_WAYLAND").is_none(),
        "FLUI_REQUIRE_WAYLAND is set and {program} is missing"
    );
    if !found {
        eprintln!("skipped: {program} is missing");
    }
    found
}

/// Wait, bounded at 10 s, until `done` holds or `server` exits.
fn until_up(what: &str, server: &mut Child, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(
            server.try_wait().expect("the server's status").is_none(),
            "{what} exited before it was up"
        );
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A headless Weston on a socket of its own, in a private runtime directory.
struct Weston {
    server: Child,
    socket: String,
    runtime: tempfile::TempDir,
}

impl Weston {
    fn start() -> Option<Self> {
        if !available("weston") {
            return None;
        }
        let runtime = tempfile::tempdir().expect("a runtime directory");
        std::fs::set_permissions(
            runtime.path(),
            <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o700),
        )
        .expect("the runtime directory is private");
        let socket = "flui-app-native".to_owned();
        let mut server = Command::new("weston")
            .args([
                "--backend=headless",
                "--renderer=pixman",
                "--idle-time=0",
                &format!("--socket={socket}"),
            ])
            .env("XDG_RUNTIME_DIR", runtime.path())
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("DISPLAY")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("weston starts");
        let path: PathBuf = runtime.path().join(&socket);
        until_up("Weston", &mut server, || path.exists());
        Some(Self {
            server,
            socket,
            runtime,
        })
    }
}

impl Drop for Weston {
    fn drop(&mut self) {
        let _ = self.server.kill();
        let _ = self.server.wait();
    }
}

/// An Xvfb on a display number no other server holds.
struct Xvfb {
    server: Child,
    display: String,
}

impl Xvfb {
    fn start() -> Option<Self> {
        if !available("Xvfb") {
            return None;
        }
        let first = 100 + std::process::id() % 800;
        for number in (first..first + 200).map(|number| 100 + number % 900) {
            let socket = PathBuf::from(format!("/tmp/.X11-unix/X{number}"));
            if socket.exists() {
                continue;
            }
            let display = format!(":{number}");
            let mut server = Command::new("Xvfb")
                .args([&display, "-screen", "0", "1024x768x24", "-nolisten", "tcp"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("Xvfb starts");
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if server.try_wait().expect("Xvfb's status").is_some() {
                    // Another server took the number first.
                    break;
                }
                if socket.exists() {
                    return Some(Self { server, display });
                }
                assert!(Instant::now() < deadline, "timed out waiting for Xvfb");
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        panic!("no free X display number");
    }
}

impl Drop for Xvfb {
    fn drop(&mut self) {
        let _ = self.server.kill();
        let _ = self.server.wait();
    }
}

/// The main window's content: a leaf whose first paint asks the application
/// to quit.
#[derive(Clone)]
struct QuitOnPaint {
    handle: AppHandle,
    asked: Arc<AtomicBool>,
}

#[derive(Debug)]
struct QuitOnPaintBox {
    handle: AppHandle,
    asked: Arc<AtomicBool>,
}

impl flui_foundation::inspect::Inspect for QuitOnPaintBox {
    fn kind(&self) -> &'static str {
        "QuitOnPaintBox"
    }

    fn inspect(&self, _: &mut flui_foundation::inspect::Fields<'_>) {}
}

impl RenderBox for QuitOnPaintBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        Ok(Size::new(40.0, 24.0))
    }

    fn paint(&self, _ctx: &mut PaintContext<'_, Leaf>) {
        if !self.asked.swap(true, Ordering::SeqCst) {
            self.handle
                .request_quit()
                .expect("the application accepts the quit while it runs");
        }
    }
}

impl RenderView for QuitOnPaint {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = QuitOnPaintBox;

    fn create_render_object(&self, _ctx: &flui_view::RenderObjectContext<'_>) -> QuitOnPaintBox {
        QuitOnPaintBox {
            handle: self.handle.clone(),
            asked: Arc::clone(&self.asked),
        }
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut QuitOnPaintBox,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }
}

impl View for QuitOnPaint {
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}
