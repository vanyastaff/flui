//! Live-smoke E2E harness — drives a REAL windowed FLUI binary with REAL X11
//! input and asserts on captured pixels and the process's exit code.
//!
//! Run: `flui-live-smoke <path-to-app-binary>` under an X server (CI:
//! `xvfb-run -a flui-live-smoke target/debug/examples/sliver_demo`), or via
//! `just live-smoke`, which builds the demo and wraps in Xvfb when no
//! display is present.
//!
//! # Why this exists
//!
//! The synthetic-event test harness covers everything DOWNSTREAM of event
//! synthesis and nothing upstream of it. Three live-runtime breakages
//! shipped while every gesture test was green: the winit translation
//! stamped no held buttons (drag-moves were hovers, live drag-scrolling
//! did nothing), a redraw request set flags without waking the parked
//! event loop (125 pointer events produced 2 frames), and closing the
//! window crashed after a clean teardown. This harness exercises exactly
//! that untested band: real platform translation, the real wake chain,
//! real teardown.
//!
//! # Checks
//!
//! 1. **launch** — the app opens an X window within the timeout.
//! 2. **drag scrolls** — an XTEST press-move-release drag changes the
//!    window's pixels (the list scrolls, the bar collapses).
//! 3. **clean close** — a `WM_DELETE_WINDOW` client message (a real window
//!    close, no window manager required) makes the process exit `0` within
//!    the timeout: no hang, no post-teardown crash.
//!
//! Wheel scrolling is deliberately NOT asserted yet: pointer-scroll events
//! are emitted by the platform but consumed by nothing downstream, so a
//! wheel assertion would either pin the broken behavior or fail the suite
//! on a known, tracked gap. Add the check when wheel dispatch lands.

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    AtomEnum, ClientMessageEvent, ConnectionExt, EventMask, ImageFormat, Window,
};
use x11rb::protocol::xtest::ConnectionExt as XTestConnectionExt;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as WrapperConnectionExt;

const WINDOW_TIMEOUT: Duration = Duration::from_secs(30);
const EXIT_TIMEOUT: Duration = Duration::from_secs(30);
/// XTEST core-pointer event codes (X11 `MotionNotify`/`ButtonPress`/
/// `ButtonRelease` — `x11rb` exposes `fake_input` untyped).
const MOTION_NOTIFY: u8 = 6;
const BUTTON_PRESS: u8 = 4;
const BUTTON_RELEASE: u8 = 5;

fn main() -> Result<()> {
    let app_path = std::env::args()
        .nth(1)
        .context("usage: flui-live-smoke <path-to-app-binary>")?;

    let mut app = Command::new(&app_path)
        // The app must open its window on the SAME X display this harness
        // drives. A leaked WAYLAND_DISPLAY would win backend selection and
        // put the window on the developer's real compositor instead of the
        // harness's (usually Xvfb) X server.
        .env_remove("WAYLAND_DISPLAY")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("spawning {app_path}"))?;

    // Ensure the child never outlives a failed run.
    let result = run_checks(&mut app);
    if result.is_err() {
        let _ = app.kill();
        let _ = app.wait();
    }
    result
}

fn run_checks(app: &mut Child) -> Result<()> {
    let (conn, screen_num) = x11rb::connect(None).context("connecting to the X display")?;
    let root = conn.setup().roots[screen_num].root;

    // Check 1: the window appears.
    let window = wait_for_window(&conn, root, app)?;
    eprintln!("live-smoke: launch OK (window {window:#x})");
    // Let the first frames land before the baseline capture.
    std::thread::sleep(Duration::from_secs(2));

    // Check 2: a drag scrolls — captured pixels must change.
    let geometry = conn.get_geometry(window)?.reply()?;
    let coords = conn
        .translate_coordinates(window, root, 0, 0)?
        .reply()
        .context("window position on the root")?;
    let (win_x, win_y) = (i32::from(coords.dst_x), i32::from(coords.dst_y));
    let center_x = (win_x + i32::from(geometry.width) / 2) as i16;
    let drag_from_y = (win_y + i32::from(geometry.height) * 3 / 4) as i16;

    let before = capture(&conn, window, &geometry)?;

    fake_motion(&conn, root, center_x, drag_from_y)?;
    conn.sync()?;
    std::thread::sleep(Duration::from_millis(200));
    conn.xtest_fake_input(BUTTON_PRESS, 1, 0, root, 0, 0, 0)?;
    conn.sync()?;
    let drag_to = |from: i16, steps: i16| -> Result<i16> {
        for step in 1..=steps {
            fake_motion(&conn, root, center_x, from - step * 15)?;
            conn.sync()?;
            std::thread::sleep(Duration::from_millis(25));
        }
        Ok(from - steps * 15)
    };

    // First half of the drag, still held: the screen must ALREADY have
    // moved by the second capture — a screen that only updates after the
    // release means input stopped producing frames (the parked-loop bug:
    // 125 pointer events, 2 frames).
    let reached = drag_to(drag_from_y, 10)?;
    std::thread::sleep(Duration::from_millis(300));
    let mid_first = capture(&conn, window, &geometry)?;
    let _ = drag_to(reached, 10)?;
    std::thread::sleep(Duration::from_millis(300));
    let mid_second = capture(&conn, window, &geometry)?;
    if mid_first == mid_second {
        bail!(
            "mid-drag check FAILED: pixels frozen between two held-drag \
             segments 150px apart — input is not producing frames"
        );
    }
    eprintln!("live-smoke: mid-drag tracking OK (screen follows the pointer)");

    conn.xtest_fake_input(BUTTON_RELEASE, 1, 0, root, 0, 0, 0)?;
    conn.sync()?;
    std::thread::sleep(Duration::from_secs(2));

    let after = capture(&conn, window, &geometry)?;
    if before == after {
        bail!(
            "drag check FAILED: {} identical bytes before and after a 300px \
             drag — pointer moves are not reaching the scrollable at all",
            before.len(),
        );
    }
    eprintln!("live-smoke: drag scrolls OK (pixels changed)");

    // Check 3: a real window close exits cleanly.
    send_wm_delete(&conn, window)?;
    let status = wait_for_exit(app, EXIT_TIMEOUT)?;
    match status {
        Some(status) if status.success() => {
            eprintln!("live-smoke: clean close OK (exit 0)");
            Ok(())
        }
        Some(status) => bail!(
            "close check FAILED: teardown finished with {status} — a post-quit \
             crash (the signal a green test suite never sees)"
        ),
        None => bail!(
            "close check FAILED: still running {}s after WM_DELETE_WINDOW — \
             the close request never reached teardown, or teardown hangs",
            EXIT_TIMEOUT.as_secs()
        ),
    }
}

/// Poll the root's children for a viewable window whose `WM_NAME` contains
/// "FLUI", failing early if the app dies first.
fn wait_for_window(conn: &RustConnection, root: Window, app: &mut Child) -> Result<Window> {
    let deadline = Instant::now() + WINDOW_TIMEOUT;
    loop {
        if let Some(status) = app.try_wait()? {
            bail!("launch check FAILED: the app exited during startup with {status}");
        }
        for &child in &conn.query_tree(root)?.reply()?.children {
            let name = conn
                .get_property(false, child, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 256)?
                .reply()?;
            if String::from_utf8_lossy(&name.value).contains("FLUI") {
                return Ok(child);
            }
        }
        if Instant::now() > deadline {
            bail!(
                "launch check FAILED: no FLUI window within {}s",
                WINDOW_TIMEOUT.as_secs()
            );
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn fake_motion(conn: &RustConnection, root: Window, x: i16, y: i16) -> Result<()> {
    conn.xtest_fake_input(MOTION_NOTIFY, 0, 0, root, x, y, 0)?;
    Ok(())
}

/// The window's current pixels, straight from the server.
fn capture(
    conn: &RustConnection,
    window: Window,
    geometry: &x11rb::protocol::xproto::GetGeometryReply,
) -> Result<Vec<u8>> {
    let image = conn
        .get_image(
            ImageFormat::Z_PIXMAP,
            window,
            0,
            0,
            geometry.width,
            geometry.height,
            !0,
        )?
        .reply()
        .context("capturing window pixels")?;
    Ok(image.data)
}

/// A real window close: the ICCCM `WM_DELETE_WINDOW` client message —
/// exactly what a window manager sends for the titlebar button, and the
/// path that reaches winit's `CloseRequested`. (Destroying the window
/// outright would bypass the app's teardown entirely.)
fn send_wm_delete(conn: &RustConnection, window: Window) -> Result<()> {
    let protocols = conn.intern_atom(false, b"WM_PROTOCOLS")?.reply()?.atom;
    let delete = conn.intern_atom(false, b"WM_DELETE_WINDOW")?.reply()?.atom;
    let event = ClientMessageEvent::new(32, window, protocols, [delete, 0, 0, 0, 0]);
    conn.send_event(false, window, EventMask::NO_EVENT, event)?;
    conn.sync()?;
    Ok(())
}

fn wait_for_exit(app: &mut Child, timeout: Duration) -> Result<Option<std::process::ExitStatus>> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = app.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() > deadline {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}
