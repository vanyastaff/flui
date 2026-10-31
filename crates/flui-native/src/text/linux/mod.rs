//! The Linux hosts' text input: the focused field served to the input
//! method through `zwp_text_input_v3` on Wayland ([`text_input`], the
//! protocol's rules over the field's store, and `wayland`, the seats' text
//! inputs that drive it).
//!
//! The model is pure and built for tests on every host; the live half binds
//! it to a compositor's seats.

pub(crate) mod text_input;
#[cfg(wayland_host)]
pub(crate) mod wayland;
