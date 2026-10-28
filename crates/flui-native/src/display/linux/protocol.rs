//! Which display protocol a process speaks, and the one rule every Linux
//! connection to a compositor follows.
//!
//! A session is Wayland when `WAYLAND_DISPLAY` names a socket (a path, or a
//! name inside `XDG_RUNTIME_DIR`), X11 when `DISPLAY` is set and not empty,
//! and Wayland first, because an X server is usually present under a Wayland
//! session too (WSLg offers both). The native Wayland host and the display
//! source both connect to that socket, so the windows and the outputs are the
//! same compositor's. `WAYLAND_SOCKET`, one descriptor already connected,
//! names no socket a second connection could reach, so it is not followed:
//! winit takes it, and the winit backend checks this choice against the
//! protocol of the display winit actually opened.

use std::path::PathBuf;

#[cfg(any(feature = "winit-backend", test))]
use raw_window_handle::RawDisplayHandle;

/// A display server protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Protocol {
    Wayland,
    X11,
}

impl Protocol {
    /// The protocol this process speaks, read from its own environment: the
    /// winit host's guess before winit opened its display.
    #[cfg(all(target_os = "linux", feature = "winit-backend"))]
    pub(crate) fn current() -> Option<Self> {
        Self::from_environment(&Environment::current())
    }

    /// The protocol of the session the environment names, or `None` when it
    /// names no display server (a headless session).
    pub(crate) fn from_environment(environment: &Environment) -> Option<Self> {
        let set = |value: &Option<String>| value.as_deref().is_some_and(|text| !text.is_empty());
        if environment.wayland_socket_path().is_some() {
            Some(Self::Wayland)
        } else if set(&environment.display) {
            Some(Self::X11)
        } else {
            None
        }
    }

    /// The protocol of the display a window system handle belongs to: the
    /// winit host checks the display winit opened against the environment's.
    #[cfg(any(feature = "winit-backend", test))]
    pub(crate) fn of_display(handle: RawDisplayHandle) -> Option<Self> {
        match handle {
            RawDisplayHandle::Wayland(_) => Some(Self::Wayland),
            RawDisplayHandle::Xlib(_) | RawDisplayHandle::Xcb(_) => Some(Self::X11),
            _ => None,
        }
    }
}

/// The variables that choose the protocol and find the Wayland socket.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Environment {
    pub wayland_display: Option<String>,
    pub wayland_socket: Option<String>,
    pub display: Option<String>,
    pub runtime_dir: Option<String>,
}

impl Environment {
    /// The process environment.
    #[cfg(target_os = "linux")]
    pub(crate) fn current() -> Self {
        let var = |name: &str| std::env::var(name).ok();
        Self {
            wayland_display: var("WAYLAND_DISPLAY"),
            wayland_socket: var("WAYLAND_SOCKET"),
            display: var("DISPLAY"),
            runtime_dir: var("XDG_RUNTIME_DIR"),
        }
    }

    /// The path of the Wayland socket named by `WAYLAND_DISPLAY`: the value
    /// itself when it is absolute, otherwise a name inside `XDG_RUNTIME_DIR`.
    ///
    /// `WAYLAND_SOCKET` is deliberately not followed. It hands over one already
    /// connected descriptor, which the process's own window system takes; a
    /// second reader of it would steal the connection.
    pub(crate) fn wayland_socket_path(&self) -> Option<PathBuf> {
        let name = self
            .wayland_display
            .as_deref()
            .filter(|name| !name.is_empty())?;
        // Socket paths are Unix paths; the string rule keeps this checkable on
        // any host.
        if name.starts_with('/') {
            return Some(PathBuf::from(name));
        }
        let runtime_dir = self.runtime_dir.as_deref().filter(|dir| !dir.is_empty())?;
        Some(PathBuf::from(format!(
            "{}/{name}",
            runtime_dir.trim_end_matches('/')
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use raw_window_handle::{RawDisplayHandle, WaylandDisplayHandle, XcbDisplayHandle};

    use super::*;

    fn environment(wayland: &str, socket: &str, display: &str) -> Environment {
        let some = |text: &str| Some(text.to_owned());
        Environment {
            wayland_display: some(wayland),
            wayland_socket: some(socket),
            display: some(display),
            runtime_dir: some("/run/user/1000"),
        }
    }

    /// A session is Wayland when a socket is named that every connection can
    /// reach, and an inherited descriptor (`WAYLAND_SOCKET`) is none: the
    /// native host, the display source and this choice agree.
    #[test]
    fn the_protocol_is_the_session_a_connection_can_reach() {
        // Wayland first, even with an X server present (WSLg).
        assert_eq!(
            Protocol::from_environment(&environment("wayland-0", "", ":0")),
            Some(Protocol::Wayland)
        );
        assert_eq!(
            Protocol::from_environment(&environment("", "7", ":0")),
            Some(Protocol::X11)
        );
        assert_eq!(Protocol::from_environment(&environment("", "7", "")), None);
        let mut no_runtime_dir = environment("wayland-0", "", "");
        no_runtime_dir.runtime_dir = None;
        assert_eq!(Protocol::from_environment(&no_runtime_dir), None);
        // Empty variables count as unset.
        assert_eq!(
            Protocol::from_environment(&environment("", "", ":0")),
            Some(Protocol::X11)
        );
        assert_eq!(Protocol::from_environment(&environment("", "", "")), None);
        assert_eq!(Protocol::from_environment(&Environment::default()), None);
    }

    #[test]
    fn the_protocol_of_a_display_handle() {
        let wayland = WaylandDisplayHandle::new(std::ptr::NonNull::dangling());
        assert_eq!(
            Protocol::of_display(RawDisplayHandle::Wayland(wayland)),
            Some(Protocol::Wayland)
        );
        assert_eq!(
            Protocol::of_display(RawDisplayHandle::Xcb(XcbDisplayHandle::new(None, 0))),
            Some(Protocol::X11)
        );
        assert_eq!(
            Protocol::of_display(RawDisplayHandle::Web(
                raw_window_handle::WebDisplayHandle::new()
            )),
            None
        );
    }

    #[test]
    fn the_wayland_socket_is_found_by_name_or_path() {
        let mut env = environment("wayland-0", "", "");
        assert_eq!(
            env.wayland_socket_path().as_deref(),
            Some(Path::new("/run/user/1000/wayland-0"))
        );
        env.wayland_display = Some("/tmp/compositor.sock".into());
        assert_eq!(
            env.wayland_socket_path().as_deref(),
            Some(Path::new("/tmp/compositor.sock"))
        );
        // A name needs a runtime dir to live in; an inherited socket is no name.
        env.wayland_display = Some("wayland-0".into());
        env.runtime_dir = None;
        assert_eq!(env.wayland_socket_path(), None);
        let inherited = environment("", "7", "");
        assert_eq!(inherited.wayland_socket_path(), None);
    }
}
