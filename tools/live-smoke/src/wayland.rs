//! The Wayland teardown-smoke mode — see the crate doc in `main.rs`.
//!
//! Narrower by necessity than the X11 harness: Wayland has no protocol a
//! third-party client can use to inject input into or close another
//! client's toplevel (`xdg_toplevel.close` is compositor→client only, and
//! weston implements neither `wlr-foreign-toplevel-management` nor a
//! scripting interface), so this mode cannot drive drags or capture pixels
//! the way `harness.rs` does with XTEST. What it CAN pin — and what shipped
//! broken while the X11 close check stayed green — is the close-path
//! teardown ordering: the app closes its own window through the platform's
//! harness self-close hook (`FLUI_SELF_CLOSE_AFTER_MS`, which synthesizes
//! the exact `CloseRequested` arm a compositor close takes) and the process
//! must exit 0. A wgpu surface destroyed after its `wl_surface` is a
//! use-after-free on the Wayland proxy and dies with SIGSEGV strictly
//! after all framework teardown has logged clean — the crash class of
//! issue #713, invisible to every in-process test.
//!
//! Runs the launch/close cycle several times: the upstream report was
//! flaky (~1 in 3 closes on a live GNOME session), so a single green run
//! is weak evidence. Under weston headless the pre-fix crash reproduced
//! deterministically, but the loop keeps the check honest if timing ever
//! re-enters the picture.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::self_close::{self, CloseRoute};

/// How many launch/self-close cycles must exit 0 on the compositor route.
const RUNS: u32 = 5;
/// How many further cycles must exit 0 on the programmatic route
/// (`PlatformWindow::close`, issue #919) — the same teardown body, entered
/// from the owner lane instead of the event arm; fewer cycles because the
/// #713 ordering it re-pins is the same code, only the entry differs.
const PROGRAMMATIC_RUNS: u32 = 2;
const APP_EXIT_TIMEOUT: Duration = Duration::from_secs(30);
const COMPOSITOR_SOCKET_TIMEOUT: Duration = Duration::from_secs(15);

pub(crate) fn run(app_path: &str) -> Result<()> {
    // SKIP (exit 0), never fail, when weston is absent: this mode is an
    // optional deepening of coverage wherever weston exists (CI installs it
    // explicitly, so it never silently skips there).
    if find_in_path("weston").is_none() {
        eprintln!(
            "live-smoke(wayland): SKIP — `weston` not found on PATH; install it \
             (e.g. `apt install weston`) to run the Wayland teardown check"
        );
        return Ok(());
    }

    let compositor = HeadlessCompositor::spawn()?;
    eprintln!(
        "live-smoke(wayland): weston headless up (socket {})",
        compositor.socket
    );

    for run in 1..=RUNS {
        run_one_cycle(app_path, &compositor, CloseRoute::Compositor, run, RUNS)?;
    }
    eprintln!("live-smoke(wayland): clean close OK ({RUNS}/{RUNS} cycles exited 0)");
    for run in 1..=PROGRAMMATIC_RUNS {
        run_one_cycle(
            app_path,
            &compositor,
            CloseRoute::Programmatic,
            run,
            PROGRAMMATIC_RUNS,
        )?;
    }
    eprintln!(
        "live-smoke(wayland): programmatic close OK ({PROGRAMMATIC_RUNS}/{PROGRAMMATIC_RUNS} \
         cycles exited 0)"
    );
    Ok(())
}

fn run_one_cycle(
    app_path: &str,
    compositor: &HeadlessCompositor,
    route: CloseRoute,
    run: u32,
    runs: u32,
) -> Result<()> {
    let log_path = std::env::temp_dir().join(format!(
        "flui-live-smoke-wayland-app-{}-{}-{run}.log",
        std::process::id(),
        route.env_value()
    ));
    let log_file = std::fs::File::create(&log_path)
        .with_context(|| format!("creating {}", log_path.display()))?;

    let mut app = Command::new(app_path)
        .env("XDG_RUNTIME_DIR", &compositor.runtime_dir)
        .env("WAYLAND_DISPLAY", &compositor.socket)
        // Force the Wayland path: with DISPLAY leaked from the environment
        // winit could fall back to X11 and this check would silently pin
        // the wrong backend's teardown.
        .env_remove("DISPLAY")
        .env(
            self_close::DEADLINE_ENV,
            self_close::SELF_CLOSE_AFTER_MS.to_string(),
        )
        .env(self_close::ROUTE_ENV, route.env_value())
        .env("RUST_LOG", "warn,flui_platform=debug")
        .stdout(log_file.try_clone().context("cloning the app log handle")?)
        .stderr(log_file)
        .spawn()
        .with_context(|| format!("spawning {app_path}"))?;

    let status = wait_with_timeout(&mut app, APP_EXIT_TIMEOUT)?;
    let route_name = route.env_value();
    let result = match status {
        Some(status) if status.success() => self_close::assert_route_observed(&log_path, route)
            .map(|()| {
                eprintln!(
                    "live-smoke(wayland): {route_name} close cycle {run}/{runs} exited 0, route \
                     observed"
                );
            }),
        Some(status) => Err(anyhow::anyhow!(
            "close check FAILED ({route_name} route, cycle {run}/{runs}): teardown finished \
             with {status} — a post-quit crash on the Wayland path (issue #713's class)"
        )),
        None => {
            let _ = app.kill();
            let _ = app.wait();
            Err(anyhow::anyhow!(
                "close check FAILED ({route_name} route, cycle {run}/{runs}): still running \
                 {}s after the self-close deadline — the close never fired or teardown hangs",
                APP_EXIT_TIMEOUT.as_secs()
            ))
        }
    };
    if result.is_err()
        && let Ok(log) = std::fs::read_to_string(&log_path)
    {
        let tail: Vec<&str> = log.lines().rev().take(200).collect();
        eprintln!("live-smoke(wayland): app log (last {} lines):", tail.len());
        for line in tail.iter().rev() {
            eprintln!("  {line}");
        }
    }
    let _ = std::fs::remove_file(&log_path);
    result
}

/// A headless weston instance on a private socket, killed on drop.
struct HeadlessCompositor {
    child: Child,
    runtime_dir: PathBuf,
    socket: String,
    log_path: PathBuf,
}

impl HeadlessCompositor {
    fn spawn() -> Result<Self> {
        // A private runtime dir, always: weston refuses XDG_RUNTIME_DIR
        // with wrong ownership/permissions, and CI runners often export
        // none at all. Must be 0700 per the XDG spec (weston checks).
        let runtime_dir =
            std::env::temp_dir().join(format!("flui-live-smoke-wayland-{}", std::process::id()));
        std::fs::create_dir_all(&runtime_dir)
            .with_context(|| format!("creating {}", runtime_dir.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&runtime_dir, std::fs::Permissions::from_mode(0o700))
                .context("restricting the runtime dir to 0700")?;
        }

        let socket = format!("wayland-flui-smoke-{}", std::process::id());
        let log_path = runtime_dir.join("weston.log");
        let log_file = std::fs::File::create(&log_path)
            .with_context(|| format!("creating {}", log_path.display()))?;

        let child = Command::new("weston")
            .arg("--backend=headless-backend.so")
            // The kiosk shell speaks the same xdg-shell the app needs but
            // launches no helper clients (the default desktop shell spawns
            // `weston-desktop-shell`/`weston-keyboard` and aborts the whole
            // compositor when either fails to start — pure liability for a
            // headless teardown check).
            .arg("--shell=kiosk-shell.so")
            .arg(format!("--socket={socket}"))
            // No idle-out mid-run, no user config surprises.
            .arg("--idle-time=0")
            .arg("--no-config")
            .env("XDG_RUNTIME_DIR", &runtime_dir)
            // Never chain onto an outer session's compositor or X server.
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("DISPLAY")
            .stdin(Stdio::null())
            .stdout(log_file.try_clone().context("cloning the weston log handle")?)
            .stderr(log_file)
            .spawn()
            .context("spawning weston")?;

        let mut compositor = Self {
            child,
            runtime_dir,
            socket,
            log_path,
        };
        compositor.wait_for_socket()?;
        Ok(compositor)
    }

    /// Poll for the compositor's socket, failing early (with weston's own
    /// log) if it dies during startup.
    fn wait_for_socket(&mut self) -> Result<()> {
        let socket_path = self.runtime_dir.join(&self.socket);
        let deadline = Instant::now() + COMPOSITOR_SOCKET_TIMEOUT;
        loop {
            if socket_path.exists() {
                return Ok(());
            }
            if let Some(status) = self.child.try_wait()? {
                self.dump_log();
                bail!("weston exited during startup with {status}");
            }
            if Instant::now() > deadline {
                self.dump_log();
                bail!(
                    "weston did not create {} within {}s",
                    socket_path.display(),
                    COMPOSITOR_SOCKET_TIMEOUT.as_secs()
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn dump_log(&self) {
        if let Ok(log) = std::fs::read_to_string(&self.log_path) {
            let tail: Vec<&str> = log.lines().rev().take(50).collect();
            eprintln!(
                "live-smoke(wayland): weston log (last {} lines):",
                tail.len()
            );
            for line in tail.iter().rev() {
                eprintln!("  {line}");
            }
        }
    }
}

impl Drop for HeadlessCompositor {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.runtime_dir);
    }
}

fn wait_with_timeout(
    app: &mut Child,
    timeout: Duration,
) -> Result<Option<std::process::ExitStatus>> {
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

fn find_in_path(binary: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(binary))
        .find(|candidate| candidate.is_file())
}
