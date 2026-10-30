//! A sway (wlroots) of the harness's own on its X11 backend, on an Xvfb of
//! its own (`x11_session.rs`): the compositor of the protocols Weston lacks,
//! such as the primary selection (`wayland_primary_selection`).

use std::process::{Command, Stdio};

#[path = "x11_session.rs"]
mod x11_session;

pub(crate) use x11_session::Session;

impl Session {
    /// Start Xvfb and a sway with no configuration, whose X11 backend opens
    /// one output window of `output` pixels, and name it for the process;
    /// `None` where Xvfb or sway is missing, which fails where
    /// `FLUI_REQUIRE_WAYLAND` is set.
    ///
    /// Call it before any thread starts: it sets the environment.
    pub(crate) fn start_sway(output: (u16, u16), name: &str) -> Option<Self> {
        let prepared = x11_session::Prepared::start("sway", name)?;
        let config = prepared.runtime().join("sway.conf");
        std::fs::write(&config, "").expect("an empty sway configuration");
        let sway = Command::new("sway")
            .arg("--config")
            .arg(&config)
            .env("DISPLAY", &prepared.display)
            .env("XDG_RUNTIME_DIR", prepared.runtime())
            .env("WLR_BACKENDS", "x11")
            .env("WLR_RENDERER", "pixman")
            .env("WLR_X11_OUTPUTS", "1")
            .env_remove("WAYLAND_DISPLAY")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("sway starts");
        // sway names its socket itself: the first `wayland-N` it can take.
        let mut socket = None;
        x11_session::until("sway's socket", || {
            socket = std::fs::read_dir(prepared.runtime())
                .ok()
                .and_then(|entries| {
                    entries
                        .filter_map(Result::ok)
                        .map(|entry| entry.file_name().to_string_lossy().into_owned())
                        .find(|name| name.starts_with("wayland-") && !name.contains('.'))
                });
            socket.is_some()
        });
        let socket = socket.expect("found above");
        Some(prepared.hold(sway, &socket, output))
    }
}
