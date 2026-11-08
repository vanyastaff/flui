//! What the kinds of data transfer share beneath them: the clipboard's
//! selection and a drag go through the same OS objects on some hosts.
//!
//! - `intake`: the resource policy every drag backend applies to an offer;
//! - `linux::wayland`: the seats' data devices, which the clipboard and a
//!   drag both reach.

pub(crate) mod intake;
#[cfg(any(wayland_host, x11_host))]
pub(crate) mod linux;
