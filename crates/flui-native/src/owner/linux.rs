//! The Linux hosts' loops: Wayland's, over the compositor's connection
//! (`wayland`), and X11's, over the X server's (`x11`); and what both keep
//! of the session in the same shape: the Settings portal's preferences, the
//! sources of the host's notices, and the desktop's opener.
//!
//! The session's sources are the preference and lifecycle features'
//! (`preferences::linux`, `lifecycle::linux`); a loop holds them through the
//! hooks here, which each host's table fills, and names no feature
//! (ADR-0151 §10).

#[cfg(any(wayland_host, test))]
pub(crate) mod wayland;
#[cfg(x11_host)]
pub(crate) mod x11;

#[cfg(any(wayland_host, x11_host))]
pub(crate) use session::{LinuxNotices, LinuxPreferences, PortalSlot, SessionLoop, xdg_open};

#[cfg(any(wayland_host, x11_host))]
mod session {
    use std::sync::{Arc, Weak};

    use flui_platform::{SessionEndAnswer, SessionEndPhase, SystemNotice, WindowAppearance};
    use parking_lot::Mutex;

    use crate::owner::sources::SystemPreferenceSource;

    /// The Settings portal's preferences as a Linux loop keeps them: the
    /// snapshot, and the colour scheme every window follows.
    pub(crate) trait LinuxPreferences: SystemPreferenceSource + Send + Sync {
        /// The colour scheme the portal reports.
        fn appearance(&self) -> WindowAppearance;
    }

    /// A Linux loop as the sources of the host's notices reach it.
    pub(crate) trait SessionLoop {
        /// Report `notice` to the application, through the loop's notice
        /// channel.
        fn notice(&self, notice: SystemNotice);

        /// Ask the application about a phase of the session ending; with no
        /// handler bound the session goes on.
        fn session_end(&self, phase: SessionEndPhase) -> SessionEndAnswer;
    }

    /// The sources of the host's system notices (logind, the kernel's memory
    /// pressure) a Linux loop keeps.
    pub(crate) trait LinuxNotices: Send + Sync {
        /// Deliver what the sources queued since the loop's last turn, oldest
        /// first, to `owner`.
        fn deliver(&self, owner: &dyn SessionLoop);
    }

    /// The portal's source as the loop and its windows reach it: weakly,
    /// since the loop starts it as it runs and stops it as it ends.
    #[derive(Clone, Default)]
    pub(crate) struct PortalSlot(Arc<Mutex<Option<Weak<dyn LinuxPreferences>>>>);

    impl PortalSlot {
        /// The colour scheme the portal reports; light while it is not
        /// running.
        pub(crate) fn appearance(&self) -> WindowAppearance {
            self.source()
                .map_or(WindowAppearance::Light, |source| source.appearance())
        }

        /// The source, while the loop runs.
        pub(crate) fn source(&self) -> Option<Arc<dyn LinuxPreferences>> {
            self.0.lock().as_ref().and_then(Weak::upgrade)
        }

        /// The loop started `source`, or stopped it with `None`.
        pub(crate) fn set(&self, source: Option<&Arc<dyn LinuxPreferences>>) {
            *self.0.lock() = source.map(Arc::downgrade);
        }
    }

    /// Hand `target` (a URL or a path) to the desktop's opener, `xdg-open`,
    /// and reap it on a thread of its own, so no exited child is left behind.
    /// Every Linux host opens through it. A failure is logged: the services
    /// answer nothing.
    pub(crate) fn xdg_open(target: &std::ffi::OsStr) {
        let _ = spawn_reaped(std::process::Command::new("xdg-open").arg(target));
    }

    /// Start `command` and wait for it on a thread of its own; its process
    /// id, or `None` when it could not start.
    fn spawn_reaped(command: &mut std::process::Command) -> Option<u32> {
        let child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                tracing::warn!(%error, "the desktop's opener could not start");
                return None;
            }
        };
        let id = child.id();
        let reaped = std::thread::Builder::new()
            .name("flui-xdg-open".into())
            .spawn(move || {
                let mut child = child;
                if let Err(error) = child.wait() {
                    tracing::warn!(%error, "the desktop's opener could not be waited for");
                }
            });
        if let Err(error) = reaped {
            tracing::warn!(%error, "the opener's reaper could not start; it stays unreaped");
        }
        Some(id)
    }

    #[cfg(test)]
    mod tests {
        /// A process the desktop's opener started is reaped once it exits: no
        /// zombie is left in the process table.
        #[test]
        fn an_opener_that_exited_leaves_no_zombie() {
            let id = super::spawn_reaped(&mut std::process::Command::new("true"))
                .expect("`true` starts");
            let entry = std::path::PathBuf::from(format!("/proc/{id}"));
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while entry.exists() && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert!(
                !entry.exists(),
                "the exited opener {id} is still in the process table"
            );
        }
    }
}
