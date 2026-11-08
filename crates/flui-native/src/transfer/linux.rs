//! The Linux hosts' transfer objects.

#[cfg(wayland_host)]
pub(crate) mod wayland;

/// The X11 selection protocol shared by the clipboard and XDND.
#[cfg(x11_host)]
pub(crate) mod x11 {
    #![forbid(unsafe_code)]

    mod selection;

    pub(crate) use selection::{
        RESERVE_CAP, ReadProgress, ReadRefusal, SelectionRead, SelectionWrite,
    };
}
