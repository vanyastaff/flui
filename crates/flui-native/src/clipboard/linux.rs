//! The Linux hosts' clipboards, one file for each.
//!
//! Both read and write their text in the forms the clipboard's model
//! (`clipboard::text`) names, which a Wayland selection names as MIME types
//! and an X11 selection as targets. Both primary selections hold the
//! application's lazily offered selection too ([`offered`]): a closure
//! asked for its text only when another client reads it, and asked and
//! dropped under containment (ADR-0281).

pub(crate) mod offered;
#[cfg(wayland_host)]
pub(crate) mod owners;
#[cfg(wayland_host)]
pub(crate) mod wayland;
#[cfg(x11_host)]
pub(crate) mod x11;

/// The most reads of one selection waiting at once, on either host. A
/// handle shares one read among all its readers, so each waiting read is
/// another handle's; more are refused at once as
/// [`ClipboardError::Unavailable`](flui_platform::ClipboardError::Unavailable).
#[cfg(any(wayland_host, x11_host))]
pub(crate) const MAX_READERS: usize = 4;
