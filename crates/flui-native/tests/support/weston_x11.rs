//! A Weston of the harness's own on its X11 backend, on an Xvfb of its own
//! (`x11_session.rs`). Shared by the harnesses that start one
//! (`wayland_input`, `wayland_frame`, `wayland_clipboard`).

use std::process::{Command, Stdio};

#[path = "x11_session.rs"]
pub(crate) mod x11_session;

pub(crate) use x11_session::Session;

impl Session {
    /// Start Xvfb and a Weston with `shell` (`kiosk-shell.so`,
    /// `desktop-shell.so`) and one output of `output` pixels on socket
    /// `socket`, and name it for the process; `None` where Xvfb or Weston is
    /// missing, which fails where `FLUI_REQUIRE_WAYLAND` is set.
    ///
    /// Call it before any thread starts: it sets the environment.
    pub(crate) fn start(shell: &str, output: (u16, u16), socket: &str) -> Option<Self> {
        let prepared = x11_session::Prepared::start("weston", socket)?;
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
            .env("DISPLAY", &prepared.display)
            .env("XDG_RUNTIME_DIR", prepared.runtime())
            .env_remove("WAYLAND_DISPLAY")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Weston starts");
        Some(prepared.hold(weston, socket, output))
    }
}
