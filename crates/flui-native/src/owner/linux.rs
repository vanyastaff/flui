//! The Linux hosts' loops: Wayland's, over the compositor's connection
//! (`wayland`), and X11's, over the X server's (`x11`); and what both keep
//! of the session in the same shape: the Settings portal's preferences and the
//! sources of the host's notices.
//!
//! The session's sources are the preference and lifecycle features'
//! (`preferences::linux`, `lifecycle::linux`); a loop holds them through the
//! hooks here, which each host's table fills, and names no feature
//! (ADR-0151 §10).

#[cfg(any(x11_host, wayland_host, all(test, unix)))]
pub(crate) mod single_instance;
#[cfg(any(wayland_host, test))]
pub(crate) mod wayland;
#[cfg(x11_host)]
pub(crate) mod x11;

#[cfg(any(wayland_host, x11_host))]
pub(crate) use session::{LinuxNotices, LinuxPreferences, PortalSlot, SessionLoop};

#[cfg(any(wayland_host, x11_host))]
mod session {
    use std::sync::{Arc, Weak};

    use flui_platform::{Brightness, SessionEndAnswer, SessionEndPhase, SystemNotice};
    use parking_lot::Mutex;

    use crate::owner::sources::SystemPreferenceSource;

    /// The Settings portal's preferences as a Linux loop keeps them: the
    /// snapshot, and the colour scheme every window follows.
    pub(crate) trait LinuxPreferences: SystemPreferenceSource + Send + Sync {
        /// The colour scheme the portal reports.
        fn appearance(&self) -> Brightness;
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
        /// running. The Wayland host's windows ask it when they open.
        #[cfg(wayland_host)]
        pub(crate) fn appearance(&self) -> Brightness {
            self.source()
                .map_or(Brightness::Light, |source| source.appearance())
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
}
