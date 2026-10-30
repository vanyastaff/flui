//! The Linux hosts' clipboards, one file for each.
//!
//! Both read and write their text in the forms the clipboard's model
//! (`clipboard::text`) names, which a Wayland selection names as MIME types
//! and an X11 selection as targets.

#[cfg(wayland_host)]
pub(crate) mod owners;
#[cfg(wayland_host)]
pub(crate) mod wayland;
#[cfg(x11_host)]
pub(crate) mod x11;
