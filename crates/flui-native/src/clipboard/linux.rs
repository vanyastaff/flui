//! The Linux hosts' clipboards: what the Wayland and X11 hosts share, and one
//! file for each.
//!
//! What both share is the text's form ([`text`]): the types a selection's
//! owner offers its text as, which one a reader asks for, and how its bytes
//! become text and back. A Wayland selection names them as MIME types, an X11
//! selection as targets, and both carry the same few.

pub(crate) mod text;
pub(crate) mod wayland;
