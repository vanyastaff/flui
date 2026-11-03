//! What the kinds of data transfer share beneath them: the clipboard's
//! selection and a drag go through the same OS objects on some hosts.
//!
//! - `linux::wayland`: the seats' data devices, which the clipboard and a
//!   drag both reach.

pub(crate) mod linux;
