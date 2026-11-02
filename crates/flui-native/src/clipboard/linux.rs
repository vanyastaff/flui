//! The Linux hosts' clipboards: what the Wayland and X11 hosts share, and one
//! file for each.
//!
//! What both share is the text's form ([`text`]): the types a selection's
//! owner offers its text as, which one a reader asks for, and how its bytes
//! become text and back. A Wayland selection names them as MIME types, an X11
//! selection as targets, and both carry the same few. Both primary selections
//! hold the application's lazily offered selection too ([`offered`]): a
//! closure asked for its text only when another client reads it, and asked
//! and dropped as ADR-0127 has a callback asked and dropped.

pub(crate) mod offered;
#[cfg(wayland_host)]
pub(crate) mod owners;
pub(crate) mod text;
#[cfg(wayland_host)]
pub(crate) mod wayland;
#[cfg(x11_host)]
pub(crate) mod x11;
